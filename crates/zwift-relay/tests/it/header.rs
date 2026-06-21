// SPDX-License-Identifier: AGPL-3.0-only
//
// Variable-length packet header round-trip and edge cases. The
// "all 8 flag combinations" test is spec §7.11 compatibility test #2.

use zwift_relay::{CodecError, Header, HeaderFlags, ParsedHeader, decode_header};

const RELAY_ID: u32 = 0xDEAD_BEEF;
const CONN_ID: u16 = 0x0042;
const SEQNO: u32 = 0x0000_1234;

fn make_header(flags: HeaderFlags) -> Header {
    Header {
        flags,
        relay_id: flags.contains(HeaderFlags::RELAY_ID).then_some(RELAY_ID),
        conn_id: flags.contains(HeaderFlags::CONN_ID).then_some(CONN_ID),
        seqno: flags.contains(HeaderFlags::SEQNO).then_some(SEQNO),
    }
}

#[test]
fn header_round_trip_all_flag_combinations() {
    // Spec §7.11 compat test #2. Every subset of {RELAY_ID, CONN_ID,
    // SEQNO} must encode and decode back to the same Header, with
    // `consumed` matching the encoded length.
    let all = [
        HeaderFlags::empty(),
        HeaderFlags::RELAY_ID,
        HeaderFlags::CONN_ID,
        HeaderFlags::SEQNO,
        HeaderFlags::RELAY_ID | HeaderFlags::CONN_ID,
        HeaderFlags::RELAY_ID | HeaderFlags::SEQNO,
        HeaderFlags::CONN_ID | HeaderFlags::SEQNO,
        HeaderFlags::RELAY_ID | HeaderFlags::CONN_ID | HeaderFlags::SEQNO,
    ];
    for flags in all {
        let header = make_header(flags);
        let bytes = header.encode();
        let ParsedHeader {
            header: decoded,
            consumed,
        } = decode_header(&bytes).unwrap_or_else(|e| panic!("decode {flags:?}: {e}"));
        assert_eq!(decoded, header, "round-trip for {flags:?}");
        assert_eq!(
            consumed,
            bytes.len(),
            "consumed should equal encoded length for {flags:?}",
        );
    }
}

#[test]
fn header_steady_state_is_one_byte() {
    // The "all None" / empty-flags steady-state header is just the
    // single flags byte. Cited in spec §4.4 ("a channel can therefore
    // send a 1-byte header").
    let header = Header {
        flags: HeaderFlags::empty(),
        relay_id: None,
        conn_id: None,
        seqno: None,
    };
    let bytes = header.encode();
    assert_eq!(bytes, vec![0x00]);

    let parsed = decode_header(&bytes).expect("decode");
    assert_eq!(parsed.header, header);
    assert_eq!(parsed.consumed, 1);
}

#[test]
fn header_field_order_relay_id_conn_id_seqno() {
    // Encode order per `zwift.mjs:1112-1135`: flags, then present
    // fields in declaration order relay_id (BE u32), conn_id (BE u16),
    // seqno (BE u32). Total 1 + 4 + 2 + 4 = 11 bytes when all set.
    let header = make_header(HeaderFlags::all());
    let bytes = header.encode();
    let expected: Vec<u8> = vec![
        0x07, // flags = RELAY_ID | CONN_ID | SEQNO
        0xDE, 0xAD, 0xBE, 0xEF, // relay_id BE u32
        0x00, 0x42, // conn_id BE u16
        0x00, 0x00, 0x12, 0x34, // seqno BE u32
    ];
    assert_eq!(bytes, expected);
    assert_eq!(bytes.len(), 11);

    let parsed = decode_header(&bytes).expect("decode");
    assert_eq!(parsed.consumed, 11);
}

#[test]
fn header_decode_short_input_errors() {
    // Flags say RELAY_ID is present, but we only supply the flags
    // byte with no room for the u32. Must surface TooShort, not
    // panic, not silently return junk.
    let buf = [HeaderFlags::RELAY_ID.bits()]; // [0x04]
    let err = decode_header(&buf).expect_err("must reject truncated input");
    match err {
        CodecError::TooShort { .. } => {}
        other => panic!("expected TooShort, got {other:?}"),
    }
}

#[test]
fn header_decode_unknown_flag_bits_errors() {
    // Bit 0x08 is not a valid HeaderFlags member; decoder must reject
    // rather than silently mask it off.
    let buf = [0x08u8];
    let err = decode_header(&buf).expect_err("must reject unknown flag bits");
    assert_eq!(err, CodecError::UnknownFlagBits(0x08));
}

#[test]
fn header_decode_returns_remainder_via_consumed() {
    // The codec slices the AAD off the front of an inbound packet
    // using `parsed.consumed`. Verify that index leaves the trailing
    // bytes intact for the caller.
    let header = make_header(HeaderFlags::CONN_ID);
    let mut buf = header.encode(); // [0x02, 0x00, 0x42] = 3 bytes
    buf.extend_from_slice(&[0xFF, 0xFE, 0xFD]); // tail belongs to caller

    let parsed = decode_header(&buf).expect("decode");
    assert_eq!(parsed.consumed, 3);
    assert_eq!(&buf[parsed.consumed..], &[0xFF, 0xFE, 0xFD]);
}
