// SPDX-License-Identifier: AGPL-3.0-only
//
// `RelayIv::to_bytes()` layout vectors. Per spec §7.4 and the JS
// reference at `zwift.mjs:1019-1026`. The first vector specifically
// pins the spec §7.12 "explicit zero bytes 0..2" footgun.

use zwift_relay::{ChannelType, DeviceType, IV_LEN, RelayIv};

#[test]
fn iv_layout_zero_bytes_at_offsets_0_and_1() {
    // Catches the spec §7.12 footgun directly: the JS reference uses
    // `Buffer.allocUnsafe(12)` and only writes bytes 2-11, leaving
    // 0-1 as whatever the buffer pool happens to hold (usually zero).
    // The Zwift server expects bytes 0-1 to be zero. A Rust port
    // *must* zero them explicitly.
    let iv = RelayIv {
        device: DeviceType::Relay,
        channel: ChannelType::UdpClient,
        conn_id: 0,
        seqno: 0,
    };
    let bytes = iv.to_bytes();
    assert_eq!(bytes.len(), IV_LEN);
    assert_eq!(&bytes[0..2], &[0u8, 0u8]);
}

#[test]
fn iv_layout_known_vector() {
    // Hand-computed against the layout in spec §7.4 / zwift.mjs:1019.
    let iv = RelayIv {
        device: DeviceType::Relay,        // 1
        channel: ChannelType::TcpClient,  // 3
        conn_id: 0x0042,                  // 0x00 0x42
        seqno: 0x0000_0001,               // 0x00 0x00 0x00 0x01
    };
    let expected: [u8; IV_LEN] = [
        0x00, 0x00, // bytes 0-1: explicit zero
        0x00, 0x01, // bytes 2-3: device  BE u16 = 1
        0x00, 0x03, // bytes 4-5: channel BE u16 = 3
        0x00, 0x42, // bytes 6-7: conn_id BE u16
        0x00, 0x00, 0x00, 0x01, // bytes 8-11: seqno BE u32
    ];
    assert_eq!(iv.to_bytes(), expected);
}

#[test]
fn iv_byte_order_is_big_endian() {
    // Distinct non-zero / non-symmetric values for every field so an
    // accidental little-endian write fails the assertion immediately.
    let iv = RelayIv {
        device: DeviceType::Companion,    // 2
        channel: ChannelType::UdpClient,  // 1
        conn_id: 0xABCD,
        seqno: 0x1234_5678,
    };
    let expected: [u8; IV_LEN] = [
        0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xAB, 0xCD, 0x12, 0x34, 0x56, 0x78,
    ];
    assert_eq!(iv.to_bytes(), expected);
}
