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
//   `UdpChannel`. STEP 10; currently stubs.
//
// Every public item is re-exported from this file so callers
// `use zwift_relay::{…}` without navigating internal module paths.

mod consts;
mod crypto;
mod frame;
mod header;
mod iv;
mod session;
pub mod udp;
mod world_timer;

pub use consts::{
    CHANNEL_TIMEOUT, ChannelType, DEFAULT_RELAY_HOST, DeviceType, IV_LEN, KEY_LEN, LOGIN_PATH,
    MAX_HELLOS, MIN_REFRESH_INTERVAL, MIN_SYNC_SAMPLES, PROTOBUF_CONTENT_TYPE,
    SESSION_REFRESH_FRACTION, SESSION_REFRESH_PATH, TAG_LEN, TCP_VERSION, UDP_PORT_PLAIN,
    UDP_PORT_SECURE, UDP_VERSION, ZWIFT_EPOCH_MS,
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
