// SPDX-License-Identifier: AGPL-3.0-only
//
// Protocol constants and enums. Mirrors `zwift.mjs:993-1010`
// (deviceTypes / channelTypes / headerFlags) and spec §7.4.

pub const IV_LEN: usize = 12;
pub const TAG_LEN: usize = 4;
pub const KEY_LEN: usize = 16;

// --- relay session (STEP 09) ----------------------------------------

/// Default relay-API host. Production: `us-or-rly101.zwift.com`.
pub const DEFAULT_RELAY_HOST: &str = "us-or-rly101.zwift.com";

/// Path of the relay login endpoint (POST `LoginRequest` →
/// `LoginResponse`).
pub const LOGIN_PATH: &str = "/api/users/login";

/// Path of the relay session refresh endpoint
/// (POST `RelaySessionRefreshRequest` → `RelaySessionRefreshResponse`).
pub const SESSION_REFRESH_PATH: &str = "/relay/session/refresh";

/// `Content-Type` Zwift's relay endpoints expect for protobuf bodies.
pub const PROTOBUF_CONTENT_TYPE: &str = "application/x-protobuf-lite";

/// Refresh fires at this fraction of the session's announced
/// lifetime. Matches `zwift.mjs:1926`
/// (`refreshDelay = (expires - now) * 0.90`).
pub const SESSION_REFRESH_FRACTION: f64 = 0.90;

/// Lower bound on refresh attempt cadence (back-off floor on
/// repeated failures). Spec §7.4.
pub const MIN_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

// --- UDP channel + time sync (STEP 10) ------------------------------

/// Zwift's "world time" epoch, in milliseconds since Unix epoch
/// (≈ 2014-10-22 19:34:34 UTC). All `worldTime` proto fields are
/// milliseconds since this point. Spec §4.3 / `zwift.mjs:92`.
pub const ZWIFT_EPOCH_MS: i64 = 1_414_016_074_400;

/// TCP port the relay server listens on. Hard-coded by sauce
/// (`zwift.mjs:1212`); the proto `TcpAddress.port` field is ignored.
pub const TCP_PORT_SECURE: u16 = 3025;

/// UDP port the secure (AES-GCM-encrypted) telemetry channel uses.
/// Spec §7.4.
pub const UDP_PORT_SECURE: u16 = 3024;

/// UDP port the plaintext telemetry channel would use. Not used by
/// this client. Listed for symmetry with [`UDP_PORT_SECURE`].
pub const UDP_PORT_PLAIN: u16 = 3022;

/// Inbound-silence watchdog timeout. After this much quiet on the
/// recv side, the channel emits a `Timeout` event so a supervisor
/// (STEP 12) can decide to reconnect. Spec §7.4 (`CHANNEL_TIMEOUT`).
pub const CHANNEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Hard cap on hello-loop attempts before declaring sync failure.
/// Matches `zwift.mjs:1378`.
pub const MAX_HELLOS: u32 = 25;

/// Minimum SNTP-style samples required before the time-sync filter
/// will *attempt* convergence. Matches sauce's `> 5` threshold at
/// `zwift.mjs:1359` (collected count must exceed this value).
pub const MIN_SYNC_SAMPLES: usize = 5;

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
