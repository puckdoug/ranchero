// SPDX-License-Identifier: AGPL-3.0-only
//
// One-time sanitiser: converts a raw ranchero wire-capture file (which
// contains plaintext OAuth credentials, AES session keys, and real athlete
// IDs) into a clean fixture safe to commit.
//
// What this tool does:
//   - Drops every HTTP record (OAuth tokens, relay login request/response,
//     REST profile bodies — all credentials).
//   - Drops outbound relay frames (ClientToServer heartbeats are not needed
//     in the metric-parity fixture).
//   - Decrypts the inbound TCP and UDP relay frames using the session key
//     from the embedded SessionManifest.
//   - Remaps every athlete ID (ServerToClient.player_id, PlayerState.id)
//     to a stable synthetic value starting at 10001.
//   - Clears zc_local_ip, zc_local_port, and zc_key from each frame.
//   - Emits the sanitised frames as plaintext ServerToClient protobuf
//     records (content_type = ProtobufLite) with a zeroed manifest so the
//     fixture never ships an AES key.
//
// Cipher parity is covered separately by the known-answer vector in
// crates/zwift-relay/tests/crypto.rs (goal item 1), so excluding
// decryption from the committed fixture loses no coverage.
//
// Usage:
//   cargo run --bin sanitize_capture -- <input.cap> <output.bin> [<basic_fixture.bin>]
//
// Defaults:
//   input  = tmp/output.cap
//   output = compat/fixtures/server_to_client/recorded_ride.bin
//   basic  = crates/zwift-proto/tests/fixtures/server_to_client_basic.bin

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use prost::Message as _;
use zwift_proto::{PlayerState, ServerToClient};
use zwift_relay::{ChannelType, DeviceType, RelayIv, decode_header, decrypt};
use zwift_relay::capture::{
    CaptureItem, CaptureReader, Direction, TransportKind,
    MAGIC, MANIFEST_PAYLOAD_LEN, VERSION,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = args.get(1).map(String::as_str).unwrap_or("tmp/output.cap");
    let output = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("compat/fixtures/server_to_client/recorded_ride.bin");
    let basic = args
        .get(3)
        .map(String::as_str)
        .unwrap_or("crates/zwift-proto/tests/fixtures/server_to_client_basic.bin");

    if let Err(e) = run(input, output, basic) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(input: &str, output: &str, basic: &str) -> Result<(), Box<dyn std::error::Error>> {
    // --- read and sanitise ---

    let mut reader = CaptureReader::open(input)?;

    // Per-(direction, transport) seqno counters; updated from each manifest
    // and incremented after every successfully decrypted frame.
    let mut seqno_in_tcp: u32 = 0;
    let mut seqno_in_udp: u32 = 0;
    let mut manifest: Option<zwift_relay::capture::SessionManifest> = None;

    // Deterministic athlete-ID remap: real i64 → synthetic i64 starting at 10001.
    let mut id_map: HashMap<i64, i64> = HashMap::new();
    let mut next_id: i64 = 10001;

    // Accumulated output: (ts_unix_ns, transport_byte, proto_bytes)
    // transport_byte: 0 = UDP, 1 = TCP (matches capture format)
    let mut frames: Vec<(u64, u8, Vec<u8>)> = Vec::new();
    // First frame that carries at least one PlayerState — used for the
    // zwift-proto basic-fixture test, which requires a non-empty states list.
    let mut first_proto_with_state: Option<Vec<u8>> = None;

    let mut http_dropped: usize = 0;
    let mut outbound_skipped: usize = 0;
    let mut decrypt_errors: usize = 0;
    let mut decode_errors: usize = 0;

    while let Some(item) = reader.next_item() {
        match item? {
            CaptureItem::Manifest(m) => {
                seqno_in_tcp = m.recv_iv_seqno_tcp;
                seqno_in_udp = m.recv_iv_seqno_udp;
                manifest = Some(m);
            }

            CaptureItem::Frame(record) => {
                if record.transport == TransportKind::Http {
                    http_dropped += 1;
                    continue;
                }

                if record.direction == Direction::Outbound {
                    outbound_skipped += 1;
                    continue;
                }

                let Some(ref mf) = manifest else {
                    eprintln!("warning: relay frame before manifest, skipping");
                    continue;
                };

                let frame_body = record.payload.as_slice();

                let parsed = match decode_header(frame_body) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("header parse error: {e}");
                        continue;
                    }
                };

                let current_seqno = match record.transport {
                    TransportKind::Tcp => seqno_in_tcp,
                    TransportKind::Udp => seqno_in_udp,
                    TransportKind::Http => unreachable!(),
                };
                let effective_seqno = parsed.header.seqno.unwrap_or(current_seqno);

                // Inbound TCP = server speaking on the TCP channel.
                // Inbound UDP = server speaking on the UDP channel.
                let channel = match record.transport {
                    TransportKind::Tcp => ChannelType::TcpServer,
                    TransportKind::Udp => ChannelType::UdpServer,
                    TransportKind::Http => unreachable!(),
                };

                let iv = RelayIv {
                    device: DeviceType::Relay,
                    channel,
                    conn_id: mf.conn_id as u16,
                    seqno: effective_seqno,
                };
                let aad    = &frame_body[..parsed.consumed];
                let cipher = &frame_body[parsed.consumed..];

                // Advance the seqno counter before acting on the result so
                // it stays in sync even when we skip a frame.
                let next_seqno = effective_seqno.wrapping_add(1);
                match record.transport {
                    TransportKind::Tcp => seqno_in_tcp = next_seqno,
                    TransportKind::Udp => seqno_in_udp = next_seqno,
                    TransportKind::Http => unreachable!(),
                }

                let plaintext = match decrypt(&mf.aes_key, &iv.to_bytes(), aad, cipher) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("decrypt error: {e}");
                        decrypt_errors += 1;
                        continue;
                    }
                };

                // Inbound frames carry the raw proto bytes directly — no envelope.
                let mut stc = match ServerToClient::decode(plaintext.as_slice()) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("proto decode error: {e}");
                        decode_errors += 1;
                        continue;
                    }
                };

                // Remap player identifiers.
                stc.player_id = stc.player_id.map(|id| remap(&mut id_map, &mut next_id, id));
                for ps in stc.states.iter_mut().chain(stc.player_states.iter_mut()) {
                    ps.id = ps.id.map(|id| remap(&mut id_map, &mut next_id, id));
                }

                // Strip network info that isn't telemetry.
                stc.zc_local_ip   = None;
                stc.zc_local_port = None;
                stc.zc_key        = None;

                let proto_bytes = stc.encode_to_vec();

                if first_proto_with_state.is_none()
                    && (!stc.states.is_empty() || !stc.player_states.is_empty())
                {
                    first_proto_with_state = Some(proto_bytes.clone());
                }

                let transport_byte = match record.transport {
                    TransportKind::Tcp => 1u8,
                    TransportKind::Udp => 0u8,
                    TransportKind::Http => unreachable!(),
                };
                frames.push((record.ts_unix_ns, transport_byte, proto_bytes));
            }
        }
    }

    // --- write sanitised capture ---

    create_parent(output)?;
    write_capture(output, &frames)?;

    println!(
        "wrote {output}: {} frames ({} TCP / {} UDP), {} IDs remapped",
        frames.len(),
        frames.iter().filter(|f| f.1 == 1).count(),
        frames.iter().filter(|f| f.1 == 0).count(),
        id_map.len(),
    );
    if http_dropped > 0 {
        println!("  dropped {http_dropped} HTTP records (credentials/tokens)");
    }
    if outbound_skipped > 0 {
        println!("  skipped {outbound_skipped} outbound frames");
    }
    if decrypt_errors > 0 {
        println!("  WARN: {decrypt_errors} decrypt errors");
    }
    if decode_errors > 0 {
        println!("  WARN: {decode_errors} proto decode errors");
    }

    // --- write proto basic fixture ---

    let basic_bytes = first_proto_with_state.unwrap_or_else(|| {
        // The capture contained no frames with PlayerState (monitor sessions
        // often receive only config and world-time frames). Synthesise a
        // minimal fixture so the codec test always has something to decode.
        println!("note: no PlayerState frame found; writing synthetic basic fixture");
        synthetic_basic_fixture()
    });
    create_parent(basic)?;
    std::fs::write(basic, &basic_bytes)?;
    println!("wrote {basic}: {} bytes (ServerToClient basic fixture)", basic_bytes.len());

    Ok(())
}

// --- helpers ---

fn remap(map: &mut HashMap<i64, i64>, next: &mut i64, real: i64) -> i64 {
    *map.entry(real).or_insert_with(|| {
        let v = *next;
        *next += 1;
        v
    })
}

fn create_parent(path: &str) -> std::io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Build a minimal synthetic ServerToClient fixture used when the capture
/// contains no real PlayerState frames (e.g. monitor-only sessions).
fn synthetic_basic_fixture() -> Vec<u8> {
    let stc = ServerToClient {
        player_id: Some(10001),
        world_time: Some(0),
        seqno: Some(1),
        player_states: vec![PlayerState {
            id: Some(10001),
            power: Some(200),
            heartrate: Some(140),
            speed: Some(10_000_000),
            distance: Some(5_000),
            cadence_u_hz: Some(1_333_333),
            ..Default::default()
        }],
        ..Default::default()
    };
    stc.encode_to_vec()
}

/// Write a clean capture file: file header + zeroed manifest + plaintext frames.
///
/// Record header layout (v3, 17 bytes):
///   ts_unix_ns(8) + kind(1) + direction(1) + transport(1) + flags(1)
///   + content_type(1) + len(4)
fn write_capture(path: &str, frames: &[(u64, u8, Vec<u8>)]) -> std::io::Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);

    // File header.
    out.write_all(MAGIC)?;
    out.write_all(&VERSION.to_le_bytes())?;

    // Zeroed manifest so the file is structurally valid but ships no key.
    // kind=1/Manifest, direction=0/Inbound, transport=0/Udp, flags=0,
    // content_type=0/Unspecified, len=MANIFEST_PAYLOAD_LEN.
    let ts0: u64 = frames.first().map(|f| f.0).unwrap_or(0);
    write_record_hdr(&mut out, ts0, 1, 0, 0, 0, 0, MANIFEST_PAYLOAD_LEN as u32)?;
    out.write_all(&[0u8; MANIFEST_PAYLOAD_LEN])?;

    // Plaintext ServerToClient frames.
    // kind=0/Frame, direction=0/Inbound, content_type=3/ProtobufLite.
    for (ts, transport, payload) in frames {
        write_record_hdr(&mut out, *ts, 0, 0, *transport, 0, 3, payload.len() as u32)?;
        out.write_all(payload)?;
    }

    out.flush()
}

fn write_record_hdr(
    out: &mut impl Write,
    ts_unix_ns: u64,
    kind: u8,
    direction: u8,
    transport: u8,
    flags: u8,
    content_type: u8,
    len: u32,
) -> std::io::Result<()> {
    out.write_all(&ts_unix_ns.to_le_bytes())?;
    out.write_all(&[kind, direction, transport, flags, content_type])?;
    out.write_all(&len.to_le_bytes())
}
