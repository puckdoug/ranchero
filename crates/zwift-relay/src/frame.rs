// SPDX-License-Identifier: AGPL-3.0-only
//
// Plaintext envelopes (TCP `[2,hello?0:1,proto…]`, UDP `[1,proto…]`)
// and TCP frame wrapping (`[BE u16 size][header][ciphertext||tag]`).
// Mirrors `sendPacket` for both transports
// (`zwift.mjs:1292-1306` TCP, `zwift.mjs:1432-1448` UDP) and the
// stream demuxer at `zwift.mjs:1259-1289`.

use crate::CodecError;

/// Parsed TCP plaintext envelope: `[u8 version][u8 hello?0:1][proto bytes]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpPlain<'a> {
    pub version: u8,
    pub hello: bool,
    pub proto_bytes: &'a [u8],
}

/// Parsed UDP plaintext envelope: `[u8 version][proto bytes]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpPlain<'a> {
    pub version: u8,
    pub proto_bytes: &'a [u8],
}

pub fn tcp_plaintext(_proto_bytes: &[u8], _hello: bool) -> Vec<u8> {
    unimplemented!("STEP-08: build TCP plaintext envelope `[2, hello?0:1, …]`")
}

pub fn udp_plaintext(_proto_bytes: &[u8]) -> Vec<u8> {
    unimplemented!("STEP-08: build UDP plaintext envelope `[1, …]`")
}

pub fn parse_tcp_plaintext(_buf: &[u8]) -> Result<TcpPlain<'_>, CodecError> {
    unimplemented!("STEP-08: parse TCP plaintext envelope")
}

pub fn parse_udp_plaintext(_buf: &[u8]) -> Result<UdpPlain<'_>, CodecError> {
    unimplemented!("STEP-08: parse UDP plaintext envelope")
}

/// Build the on-wire TCP frame: prepend a `BE u16` length covering
/// `header_bytes.len() + ciphertext_with_tag.len()`.
pub fn frame_tcp(_header_bytes: &[u8], _ciphertext_with_tag: &[u8]) -> Vec<u8> {
    unimplemented!("STEP-08: prepend BE u16 size to header || ciphertext")
}

/// Stream demuxer for TCP. Returns `Ok(Some((payload, consumed)))` for
/// the next complete frame, `Ok(None)` if more bytes are needed,
/// `Err(_)` for unrecoverable framing errors. `payload` is the body
/// after the 2-byte size prefix; `consumed` is the total bytes the
/// caller should drop from its read buffer (`size + 2`).
pub fn next_tcp_frame(_buf: &[u8]) -> Result<Option<(&[u8], usize)>, CodecError> {
    unimplemented!("STEP-08: TCP stream demuxer per zwift.mjs:1259-1289")
}
