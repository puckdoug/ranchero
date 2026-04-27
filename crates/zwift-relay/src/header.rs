// SPDX-License-Identifier: AGPL-3.0-only
//
// Variable-length packet header. Mirrors `encodeHeader` at
// `zwift.mjs:1112-1135` and the inline decode loop at
// `zwift.mjs:1071-1090`.

use crate::CodecError;

bitflags::bitflags! {
    /// Header flag bitmap. Bits indicate which optional fields the
    /// packet header carries; values match `zwift.mjs:1005-1009`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct HeaderFlags: u8 {
        const RELAY_ID = 0x4;
        const CONN_ID  = 0x2;
        const SEQNO    = 0x1;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub flags: HeaderFlags,
    pub relay_id: Option<u32>,
    pub conn_id: Option<u16>,
    pub seqno: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHeader {
    pub header: Header,
    /// Number of bytes consumed by the header (== length of the AAD
    /// slice the caller will pass to `decrypt`).
    pub consumed: usize,
}

impl Header {
    /// Encode `flags` byte then the present fields in order
    /// `relay_id` (BE u32) → `conn_id` (BE u16) → `seqno` (BE u32).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(11);
        out.push(self.flags.bits());
        if self.flags.contains(HeaderFlags::RELAY_ID) {
            out.extend_from_slice(&self.relay_id.unwrap_or(0).to_be_bytes());
        }
        if self.flags.contains(HeaderFlags::CONN_ID) {
            out.extend_from_slice(&self.conn_id.unwrap_or(0).to_be_bytes());
        }
        if self.flags.contains(HeaderFlags::SEQNO) {
            out.extend_from_slice(&self.seqno.unwrap_or(0).to_be_bytes());
        }
        out
    }
}

/// Parse a header from the front of `bytes`. Returns the structured
/// header plus how many bytes it consumed (the AAD length).
pub fn decode_header(bytes: &[u8]) -> Result<ParsedHeader, CodecError> {
    if bytes.is_empty() {
        return Err(CodecError::TooShort {
            needed: 1,
            got: 0,
        });
    }
    let flag_byte = bytes[0];
    // `from_bits` returns None if any unknown bit is set; surface the
    // raw value so callers can see what they got.
    let flags = HeaderFlags::from_bits(flag_byte).ok_or(CodecError::UnknownFlagBits(flag_byte))?;

    let mut offset = 1usize;
    let mut header = Header {
        flags,
        relay_id: None,
        conn_id: None,
        seqno: None,
    };

    fn need(bytes: &[u8], end: usize) -> Result<(), CodecError> {
        if end > bytes.len() {
            Err(CodecError::TooShort {
                needed: end,
                got: bytes.len(),
            })
        } else {
            Ok(())
        }
    }

    if flags.contains(HeaderFlags::RELAY_ID) {
        let end = offset + 4;
        need(bytes, end)?;
        header.relay_id = Some(u32::from_be_bytes(bytes[offset..end].try_into().unwrap()));
        offset = end;
    }
    if flags.contains(HeaderFlags::CONN_ID) {
        let end = offset + 2;
        need(bytes, end)?;
        header.conn_id = Some(u16::from_be_bytes(bytes[offset..end].try_into().unwrap()));
        offset = end;
    }
    if flags.contains(HeaderFlags::SEQNO) {
        let end = offset + 4;
        need(bytes, end)?;
        header.seqno = Some(u32::from_be_bytes(bytes[offset..end].try_into().unwrap()));
        offset = end;
    }

    Ok(ParsedHeader {
        header,
        consumed: offset,
    })
}
