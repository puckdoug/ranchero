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

/// A pool entry returned by [`extract_udp_pools`], preserving the
/// `(lb_realm, lb_course)` discriminator that sauce uses to key
/// `_udpServerPools` (`zwift.mjs:2156`).
pub struct UdpPoolEntry {
    pub lb_realm: i32,
    pub lb_course: i32,
    pub addresses: Vec<zwift_proto::RelayAddress>,
}

/// Extract the UDP relay-server pools from a `ServerToClient` push,
/// preserving the per-pool `(lb_realm, lb_course)` discriminator.
///
/// Sauce keys its `_udpServerPools` map by `x.courseId` (= `lb_course`).
/// The initial UDP target is always `_udpServerPools.get(0).servers[0]`
/// — the **generic load-balancer pool at `lb_course=0`**. Per-course
/// pools (lb_course ≠ 0) are for direct-server routing after the daemon
/// knows the watched athlete's current course.
///
/// Priority order mirrors sauce:
/// 1. `udp_config_vod_1` — production Zwift.
/// 2. `udp_config_vod_2` — second slot (rare).
/// 3. Flat `udp_config` — treated as a single `lb_course=0` pool.
///
/// Returns `None` when the message carries no UDP server hints.
pub fn extract_udp_pools(
    stc: &zwift_proto::ServerToClient,
) -> Option<Vec<UdpPoolEntry>> {
    let pools_from_vod = |vod: &zwift_proto::UdpConfigVod| -> Vec<UdpPoolEntry> {
        vod.relay_addresses_vod
            .iter()
            .filter(|p| !p.relay_addresses.is_empty())
            .map(|p| UdpPoolEntry {
                lb_realm: p.lb_realm.unwrap_or(0),
                lb_course: p.lb_course.unwrap_or(0),
                addresses: p.relay_addresses.clone(),
            })
            .collect()
    };

    if let Some(vod) = &stc.udp_config_vod_1 {
        let pools = pools_from_vod(vod);
        if !pools.is_empty() {
            return Some(pools);
        }
    }
    if let Some(vod) = &stc.udp_config_vod_2 {
        let pools = pools_from_vod(vod);
        if !pools.is_empty() {
            return Some(pools);
        }
    }
    if let Some(cfg) = &stc.udp_config
        && !cfg.relay_addresses.is_empty()
    {
        // Flat UdpConfig has no per-pool course info; treat as generic.
        return Some(vec![UdpPoolEntry {
            lb_realm: 0,
            lb_course: 0,
            addresses: cfg.relay_addresses.clone(),
        }]);
    }
    None
}

/// Deprecated flat extractor kept for callers that don't need pool
/// discrimination. Prefer [`extract_udp_pools`] for new code.
pub fn extract_udp_servers(
    stc: &zwift_proto::ServerToClient,
) -> Option<Vec<zwift_proto::RelayAddress>> {
    extract_udp_pools(stc).map(|pools| {
        pools.into_iter().flat_map(|p| p.addresses).collect()
    })
}
