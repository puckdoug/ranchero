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
        unimplemented!("STEP-08: encode flags + present fields per zwift.mjs:1112-1135")
    }
}

/// Parse a header from the front of `bytes`. Returns the structured
/// header plus how many bytes it consumed (the AAD length).
pub fn decode_header(_bytes: &[u8]) -> Result<ParsedHeader, CodecError> {
    unimplemented!("STEP-08: decode flags + walk fields per zwift.mjs:1071-1090")
}
