// SPDX-License-Identifier: AGPL-3.0-only
//
// 12-byte AES-GCM IV builder. Mirrors `RelayIV.toBuffer()` at
// `zwift.mjs:1019-1026`. Bytes 0-1 are explicitly zero — see spec
// §7.12 footgun about `Buffer.allocUnsafe`.

use crate::consts::{ChannelType, DeviceType, IV_LEN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayIv {
    pub device: DeviceType,
    pub channel: ChannelType,
    pub conn_id: u16,
    pub seqno: u32,
}

impl RelayIv {
    /// 12-byte GCM IV layout:
    ///
    /// ```text
    /// offset 0-1 : 0x00 0x00          (explicit zero — see spec §7.12)
    /// offset 2-3 : device  BE u16
    /// offset 4-5 : channel BE u16
    /// offset 6-7 : conn_id BE u16
    /// offset 8-11: seqno   BE u32
    /// ```
    pub fn to_bytes(&self) -> [u8; IV_LEN] {
        let mut out = [0u8; IV_LEN];
        out[2..4].copy_from_slice(&(self.device as u16).to_be_bytes());
        out[4..6].copy_from_slice(&(self.channel as u16).to_be_bytes());
        out[6..8].copy_from_slice(&self.conn_id.to_be_bytes());
        out[8..12].copy_from_slice(&self.seqno.to_be_bytes());
        out
    }
}
