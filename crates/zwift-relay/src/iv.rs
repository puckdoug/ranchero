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
        unimplemented!("STEP-08: build the 12-byte GCM IV per spec §7.4 / zwift.mjs:1019")
    }
}
