// SPDX-License-Identifier: AGPL-3.0-only
//
// `zwift-relay` — Zwift relay protocol.
//
// - Codec layer (no I/O): `RelayIv`, `Header`, AES-128-GCM-4,
//   TCP/UDP frame wrapping. STEP 08; implemented.
// - Session layer (HTTPS, async): `RelaySession` POD,
//   `login`/`refresh` single-shots, `RelaySessionSupervisor` long-
//   running task. STEP 09; implemented.
// - UDP channel + time sync (async): `WorldTimer`, `UdpTransport`,
//   `UdpChannel`. STEP 10; implemented.
// - TCP channel (async): `TcpTransport`, `TcpChannel`.
//   STEP 11; implemented.
// - Wire capture / replay: `CaptureWriter`, `CaptureReader`.
//   STEP 11.5; implemented.
//
// Every public item is re-exported from this file so callers
// `use zwift_relay::{…}` without navigating internal module paths.

pub mod capture;
mod consts;
mod crypto;
mod frame;
mod header;
mod iv;
mod session;
mod tcp;
pub mod udp;
mod world_timer;

pub use consts::{
    CHANNEL_TIMEOUT, ChannelType, DEFAULT_RELAY_HOST, DeviceType, IV_LEN, KEY_LEN, LOGIN_PATH,
    MAX_HELLOS, MIN_REFRESH_INTERVAL, MIN_SYNC_SAMPLES, PROTOBUF_CONTENT_TYPE,
    SESSION_REFRESH_FRACTION, SESSION_REFRESH_PATH, TAG_LEN, TCP_PORT_SECURE, TCP_VERSION,
    UDP_PORT_PLAIN, UDP_PORT_SECURE, UDP_VERSION, ZWIFT_EPOCH_MS,
};
pub use crypto::{decrypt, encrypt};
pub use frame::{
    TcpPlain, UdpPlain, frame_tcp, next_tcp_frame, parse_tcp_plaintext, parse_udp_plaintext,
    tcp_plaintext, udp_plaintext,
};
pub use header::{Header, HeaderFlags, ParsedHeader, decode_header};
pub use iv::RelayIv;
pub use session::{
    Error as SessionError, RelaySession, RelaySessionConfig, RelaySessionSupervisor,
    Result as SessionResult, SessionEvent, TcpServer, login, refresh,
};
pub use tcp::{
    Error as TcpError, TcpChannel, TcpChannelConfig, TcpChannelEvent, TcpTransport,
    TokioTcpTransport,
};
pub use udp::{
    ChannelEvent, Error as UdpError, TokioUdpTransport, UdpChannel, UdpChannelConfig, UdpTransport,
};
pub use world_timer::WorldTimer;

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum CodecError {
    #[error("input too short: need {needed} bytes, got {got}")]
    TooShort { needed: usize, got: usize },

    #[error("unrecognized header flag bits: 0x{0:02x}")]
    UnknownFlagBits(u8),

    #[error("AES-GCM auth tag mismatch (decrypt rejected)")]
    AuthTagMismatch,

    #[error("frame size {0} exceeds buffer length")]
    FrameSizeExceedsBuffer(u16),

    #[error("plaintext envelope: bad version byte {got}")]
    BadVersion { got: u8 },
}

/// Extract the UDP relay-server list from a `ServerToClient` push,
/// flattening across the three fields Zwift uses to announce them.
///
/// The TCP `ServerToClient` stream carries UDP server pools in three
/// variants (proto field tags, in priority order):
///
/// 1. `udp_config_vod_1` — per-realm/per-course `RelayAddressesVod`
///    pool list. Production Zwift uses this in the steady state.
/// 2. `udp_config_vod_2` — same shape; second slot. Reserved /
///    rarely populated.
/// 3. `udp_config` — flat `RelayAddress` list (legacy / fallback).
///
/// Returns `Some(addrs)` when at least one variant is non-empty;
/// `None` when the message carries no UDP server hints. The
/// per-pool `(lb_realm, lb_course)` from `RelayAddressesVod` is
/// dropped: each `RelayAddress` already carries its own
/// `lb_realm` / `lb_course`, which is what the daemon's
/// `UdpPoolRouter` keys on.
pub fn extract_udp_servers(
    stc: &zwift_proto::ServerToClient,
) -> Option<Vec<zwift_proto::RelayAddress>> {
    if let Some(vod) = &stc.udp_config_vod_1 {
        let addrs: Vec<_> = vod
            .relay_addresses_vod
            .iter()
            .flat_map(|p| p.relay_addresses.iter().cloned())
            .collect();
        if !addrs.is_empty() {
            return Some(addrs);
        }
    }
    if let Some(vod) = &stc.udp_config_vod_2 {
        let addrs: Vec<_> = vod
            .relay_addresses_vod
            .iter()
            .flat_map(|p| p.relay_addresses.iter().cloned())
            .collect();
        if !addrs.is_empty() {
            return Some(addrs);
        }
    }
    if let Some(cfg) = &stc.udp_config
        && !cfg.relay_addresses.is_empty()
    {
        return Some(cfg.relay_addresses.clone());
    }
    None
}
