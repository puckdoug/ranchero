// SPDX-License-Identifier: AGPL-3.0-only
//! 19.3-T — Header codec round-trip, workspace-root entry point.
//!
//! Verifies that `Header::encode` / `decode_header` are inverses across all 8
//! subsets of `{RELAY_ID, CONN_ID, SEQNO}`, and that `consumed` equals the
//! encoded length in every case.  This is spec §7.11 compatibility test #2.
//!
//! The canonical crate-level pin is
//! `crates/zwift-relay/tests/header.rs::header_round_trip_all_flag_combinations`.
//!
//! See docs/planning/STEP-19-compatibility-tests.md, item 19.3-T.

use zwift_relay::{Header, HeaderFlags, ParsedHeader, decode_header};

const RELAY_ID: u32 = 0xDEAD_BEEF;
const CONN_ID:  u16 = 0x0042;
const SEQNO:    u32 = 0x0000_1234;

fn make_header(flags: HeaderFlags) -> Header {
    Header {
        flags,
        relay_id: flags.contains(HeaderFlags::RELAY_ID).then_some(RELAY_ID),
        conn_id:  flags.contains(HeaderFlags::CONN_ID).then_some(CONN_ID),
        seqno:    flags.contains(HeaderFlags::SEQNO).then_some(SEQNO),
    }
}

#[test]
fn compat_header_round_trip_all_flag_combinations() {
    let all = [
        HeaderFlags::empty(),
        HeaderFlags::RELAY_ID,
        HeaderFlags::CONN_ID,
        HeaderFlags::SEQNO,
        HeaderFlags::RELAY_ID | HeaderFlags::CONN_ID,
        HeaderFlags::RELAY_ID | HeaderFlags::SEQNO,
        HeaderFlags::CONN_ID  | HeaderFlags::SEQNO,
        HeaderFlags::RELAY_ID | HeaderFlags::CONN_ID | HeaderFlags::SEQNO,
    ];
    for flags in all {
        let header = make_header(flags);
        let bytes  = header.encode();
        let ParsedHeader { header: decoded, consumed } =
            decode_header(&bytes).unwrap_or_else(|e| panic!("decode {flags:?}: {e}"));
        assert_eq!(decoded, header,       "round-trip failed for {flags:?}");
        assert_eq!(consumed, bytes.len(), "consumed must equal encoded length for {flags:?}");
    }
}
