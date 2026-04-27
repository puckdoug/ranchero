// SPDX-License-Identifier: AGPL-3.0-only
//
// Protocol constants and enums. Mirrors `zwift.mjs:993-1010`
// (deviceTypes / channelTypes / headerFlags) and spec §7.4.

pub const IV_LEN: usize = 12;
pub const TAG_LEN: usize = 4;
pub const KEY_LEN: usize = 16;

/// Plaintext envelope version byte for TCP. Followed by a `hello?0:1`
/// byte and then the `ClientToServer` proto bytes.
pub const TCP_VERSION: u8 = 2;

/// Plaintext envelope version byte for UDP. Followed directly by the
/// `ClientToServer` proto bytes (no hello byte). See STEP-08
/// "Open verification points" §1.
pub const UDP_VERSION: u8 = 1;

/// IV byte 2-3 — what kind of device this client is identifying as.
/// Sauce / ranchero are `Relay`; companion-app code paths use
/// `Companion`.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Relay = 1,
    Companion = 2,
}

/// IV byte 4-5 — the channel direction & transport. Note `Client`
/// variants are for the *send* IV, `Server` variants for the *recv*
/// IV.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelType {
    UdpClient = 1,
    UdpServer = 2,
    TcpClient = 3,
    TcpServer = 4,
}
