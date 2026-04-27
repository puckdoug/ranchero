// SPDX-License-Identifier: AGPL-3.0-only
//
// `zwift-relay` — pure no-I/O codec for the Zwift relay protocol.
//
// Public API surface lives across small modules and is re-exported
// here so callers `use zwift_relay::{…}` without navigating internal
// paths. This file currently exposes the surface as stubs so the
// `tests/*.rs` suites compile; behavior is implemented in a later
// pass. Until then every entry point panics via `unimplemented!()`
// and tests fail loudly. This is the TDD scaffold, not the
// implementation. See `docs/plans/STEP-08-relay-codec.md`.

mod consts;
mod crypto;
mod frame;
mod header;
mod iv;

pub use consts::{
    ChannelType, DeviceType, IV_LEN, KEY_LEN, TAG_LEN, TCP_VERSION, UDP_VERSION,
};
pub use crypto::{decrypt, encrypt};
pub use frame::{
    TcpPlain, UdpPlain, frame_tcp, next_tcp_frame, parse_tcp_plaintext, parse_udp_plaintext,
    tcp_plaintext, udp_plaintext,
};
pub use header::{Header, HeaderFlags, ParsedHeader, decode_header};
pub use iv::RelayIv;

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
