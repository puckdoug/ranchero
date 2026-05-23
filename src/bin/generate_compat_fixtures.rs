// SPDX-License-Identifier: AGPL-3.0-only
//
// Generates the committed compat/ fixture files from source descriptions and
// from the existing sanitised capture.
//
// For each compat/fixtures/server_to_client/*.source.json:
//   1. Reads the segment description.
//   2. Writes a companion .bin file (plaintext ServerToClient frames,
//      one per tick, in the standard ranchero capture format).
//   3. Runs the decode → route → format pipeline on the .bin.
//   4. Writes the pipeline output to compat/expected/<stem>.metrics.json.
//
// Additionally processes compat/fixtures/server_to_client/recorded_ride.bin:
//   - Runs the same pipeline.
//   - Writes the output to compat/expected/recorded_ride.golden.json.
//
// Run with:
//   cargo run --bin generate_compat_fixtures
//
// The generator is idempotent: re-running it overwrites the same files.
// Commit both the .bin and the .json outputs after running.

use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

use prost::Message as _;
use serde::Deserialize;
use serde_json::Value;
use zwift_proto::{PlayerState, ServerToClient};
use zwift_relay::capture::{
    CaptureItem, CaptureReader, ContentType, Direction,
    MAGIC, MANIFEST_PAYLOAD_LEN, VERSION,
};
use ranchero::web::{format::format_athlete_data_v1, proto_to_stats::route_player_state, WebState};

// ---------------------------------------------------------------------------
// Source JSON schema
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SourceFile {
    athlete_id:  i64,
    world:       i32,
    segments:    Vec<Segment>,
}

#[derive(Deserialize)]
struct Segment {
    ticks:        u32,
    power:        i32,
    heartrate:    i32,
    speed_mm_h:   u32,
    cadence_u_hz: i32,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let manifest_dir = std::env!("CARGO_MANIFEST_DIR");
    let fixtures_dir = format!("{manifest_dir}/compat/fixtures/server_to_client");
    let expected_dir = format!("{manifest_dir}/compat/expected");
    std::fs::create_dir_all(&expected_dir).expect("create compat/expected");

    // Process each *.source.json.
    let mut source_count = 0;
    let entries = std::fs::read_dir(&fixtures_dir).expect("read fixtures dir");
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy();
        if !name.ends_with(".source.json") {
            continue;
        }
        source_count += 1;
        let stem = name.trim_end_matches(".source.json").to_string();
        let bin_path = format!("{fixtures_dir}/{stem}.bin");
        let oracle_path = format!("{expected_dir}/{stem}.metrics.json");

        println!("processing {name}...");
        let src: SourceFile = serde_json::from_slice(
            &std::fs::read(&path).expect("read source json"),
        ).expect("parse source json");

        let ticks = build_ticks(&src);
        write_bin(&bin_path, &ticks);
        println!("  wrote {bin_path} ({} frames)", ticks.len());

        let result = run_pipeline(&bin_path);
        let json = serde_json::to_string_pretty(&result).expect("serialize");
        std::fs::write(&oracle_path, json.as_bytes()).expect("write oracle");
        println!("  wrote {oracle_path}");
    }

    if source_count == 0 {
        eprintln!("warning: no *.source.json files found in {fixtures_dir}");
    }

    // Process recorded_ride.bin as the regression golden.
    let recorded = format!("{fixtures_dir}/recorded_ride.bin");
    if Path::new(&recorded).exists() {
        println!("processing recorded_ride.bin...");
        let result = run_pipeline(&recorded);
        let golden_path = format!("{expected_dir}/recorded_ride.golden.json");
        let json = serde_json::to_string_pretty(&result).expect("serialize");
        std::fs::write(&golden_path, json.as_bytes()).expect("write golden");
        println!("  wrote {golden_path}");
    } else {
        eprintln!("warning: recorded_ride.bin not found, skipping golden");
    }

    println!("done.");
}

// ---------------------------------------------------------------------------
// Build tick list from source description
// ---------------------------------------------------------------------------

/// Each tick is `(ts_unix_ns, proto_bytes)`.
fn build_ticks(src: &SourceFile) -> Vec<(u64, Vec<u8>)> {
    let mut ticks = Vec::new();
    let mut world_time_ms: i64 = 1000; // start at t=1 s

    for seg in &src.segments {
        for _ in 0..seg.ticks {
            let ps = PlayerState {
                id:           Some(src.athlete_id),
                world:        Some(src.world),
                world_time:   Some(world_time_ms),
                power:        Some(seg.power),
                heartrate:    Some(seg.heartrate),
                speed:        Some(seg.speed_mm_h),
                cadence_u_hz: Some(seg.cadence_u_hz),
                ..Default::default()
            };
            let stc = ServerToClient {
                player_id:    Some(src.athlete_id),
                world_time:   Some(world_time_ms),
                player_states: vec![ps],
                ..Default::default()
            };
            let ts_ns = (world_time_ms as u64) * 1_000_000;
            ticks.push((ts_ns, stc.encode_to_vec()));
            world_time_ms += 1000;
        }
    }
    ticks
}

// ---------------------------------------------------------------------------
// Write a .bin file in the standard ranchero plaintext capture format
// ---------------------------------------------------------------------------

fn write_bin(path: &str, ticks: &[(u64, Vec<u8>)]) {
    let mut out = std::io::BufWriter::new(
        std::fs::File::create(path).expect("create bin"),
    );

    // File header.
    out.write_all(MAGIC).unwrap();
    out.write_all(&VERSION.to_le_bytes()).unwrap();

    // Zeroed manifest record (structurally required; no key shipped).
    let ts0 = ticks.first().map(|t| t.0).unwrap_or(0);
    write_record_hdr(&mut out, ts0, 1, 0, 0, 0, 0, MANIFEST_PAYLOAD_LEN as u32);
    out.write_all(&[0u8; MANIFEST_PAYLOAD_LEN]).unwrap();

    // Plaintext ServerToClient frames.
    // kind=0/Frame, direction=0/Inbound, transport=1/Tcp,
    // flags=0, content_type=3/ProtobufLite.
    for (ts, payload) in ticks {
        write_record_hdr(&mut out, *ts, 0, 0, 1, 0, 3, payload.len() as u32);
        out.write_all(payload).unwrap();
    }
    out.flush().unwrap();
}

fn write_record_hdr(
    out:        &mut impl std::io::Write,
    ts_unix_ns: u64,
    kind:       u8,
    direction:  u8,
    transport:  u8,
    flags:      u8,
    content_type: u8,
    len:        u32,
) {
    out.write_all(&ts_unix_ns.to_le_bytes()).unwrap();
    out.write_all(&[kind, direction, transport, flags, content_type]).unwrap();
    out.write_all(&len.to_le_bytes()).unwrap();
}

// ---------------------------------------------------------------------------
// Stats pipeline: decode → route → flush → format
// ---------------------------------------------------------------------------

/// Read every frame in the capture file, route each PlayerState through the
/// stats engine, flush deferred buffers, and format all athletes.
///
/// Returns a JSON object keyed by athlete ID string.
pub fn run_pipeline(fixture_path: &str) -> Value {
    let state = Arc::new(WebState::new());

    let mut reader = CaptureReader::open(fixture_path).expect("open fixture");
    while let Some(item) = reader.next_item() {
        match item.expect("read record") {
            CaptureItem::Manifest(_) => {}
            CaptureItem::Frame(record) => {
                if record.direction != Direction::Inbound {
                    continue;
                }
                if record.content_type != ContentType::ProtobufLite {
                    continue;
                }
                let stc = match ServerToClient::decode(record.payload.as_slice()) {
                    Ok(s)  => s,
                    Err(_) => continue,
                };
                for ps in stc.states.iter().chain(stc.player_states.iter()) {
                    route_player_state(ps, &state, 0.0, 0);
                }
            }
        }
    }

    // Flush deferred buffers so the last tick's data is visible.
    {
        let mut reg = state.registry.write().unwrap();
        let ids: Vec<u32> = reg.iter().map(|(id, _)| *id).collect();
        for id in ids {
            if let Some(ad) = reg.get_mut(id) {
                ad.bucket.flush_all();
            }
        }
    }

    // Format all athletes and collect into a JSON object keyed by athlete ID.
    let reg = state.registry.read().unwrap();
    let mut map = serde_json::Map::new();
    for (id, athlete) in reg.iter() {
        let json = format_athlete_data_v1(athlete, None, None, None, 0.0, 0.0);
        map.insert(id.to_string(), json);
    }
    Value::Object(map)
}
