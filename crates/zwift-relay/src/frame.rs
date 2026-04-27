// SPDX-License-Identifier: AGPL-3.0-only
//
// Plaintext envelopes (TCP `[2,hello?0:1,proto…]`, UDP `[1,proto…]`)
// and TCP frame wrapping (`[BE u16 size][header][ciphertext||tag]`).
// Mirrors `sendPacket` for both transports
// (`zwift.mjs:1292-1306` TCP, `zwift.mjs:1432-1448` UDP) and the
// stream demuxer at `zwift.mjs:1259-1289`.

use crate::CodecError;
use crate::consts::{TCP_VERSION, UDP_VERSION};

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

pub fn tcp_plaintext(proto_bytes: &[u8], hello: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + proto_bytes.len());
    out.push(TCP_VERSION);
    out.push(if hello { 0 } else { 1 });
    out.extend_from_slice(proto_bytes);
    out
}

pub fn udp_plaintext(proto_bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + proto_bytes.len());
    out.push(UDP_VERSION);
    out.extend_from_slice(proto_bytes);
    out
}

pub fn parse_tcp_plaintext(buf: &[u8]) -> Result<TcpPlain<'_>, CodecError> {
    if buf.len() < 2 {
        return Err(CodecError::TooShort {
            needed: 2,
            got: buf.len(),
        });
    }
    let version = buf[0];
    if version != TCP_VERSION {
        return Err(CodecError::BadVersion { got: version });
    }
    Ok(TcpPlain {
        version,
        hello: buf[1] == 0,
        proto_bytes: &buf[2..],
    })
}

pub fn parse_udp_plaintext(buf: &[u8]) -> Result<UdpPlain<'_>, CodecError> {
    if buf.is_empty() {
        return Err(CodecError::TooShort {
            needed: 1,
            got: 0,
        });
    }
    let version = buf[0];
    if version != UDP_VERSION {
        return Err(CodecError::BadVersion { got: version });
    }
    Ok(UdpPlain {
        version,
        proto_bytes: &buf[1..],
    })
}

/// Build the on-wire TCP frame: prepend a `BE u16` length covering
/// `header_bytes.len() + ciphertext_with_tag.len()`.
pub fn frame_tcp(header_bytes: &[u8], ciphertext_with_tag: &[u8]) -> Vec<u8> {
    let body_len = header_bytes.len() + ciphertext_with_tag.len();
    let size = u16::try_from(body_len).expect("frame body fits u16");
    let mut out = Vec::with_capacity(2 + body_len);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(header_bytes);
    out.extend_from_slice(ciphertext_with_tag);
    out
}

/// Stream demuxer for TCP. Returns `Ok(Some((payload, consumed)))` for
/// the next complete frame, `Ok(None)` if more bytes are needed,
/// `Err(_)` for unrecoverable framing errors. `payload` is the body
/// after the 2-byte size prefix; `consumed` is the total bytes the
/// caller should drop from its read buffer (`size + 2`).
pub fn next_tcp_frame(buf: &[u8]) -> Result<Option<(&[u8], usize)>, CodecError> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let size = u16::from_be_bytes([buf[0], buf[1]]);
    let total = 2 + size as usize;
    if buf.len() < total {
        return Ok(None);
    }
    Ok(Some((&buf[2..total], total)))
}
