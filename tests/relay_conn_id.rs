//! Red-state tests for Defect 13: `conn_id` hardcoded to 0 in
//! `TcpChannelConfig` and `UdpChannelConfig`.
//!
//! This file fails to compile until `ranchero::daemon::relay::next_conn_id`
//! is introduced by the green-state implementation.
//!
//! `conn_id` is used directly in the AES-GCM IV construction inside
//! `zwift_relay`. The reference implementation assigns it from a per-process
//! counter that increments modulo 0xffff (`getConnInc()`). Ranchero
//! currently hardcodes 0, causing IV reuse on any reconnection within the
//! same process, which breaks AES-GCM.

use ranchero::daemon::relay::next_conn_id;

// D13-a: each call advances the counter, producing a unique value per channel.
#[test]
fn conn_id_counter_increments_on_successive_calls() {
    let a = next_conn_id();
    let b = next_conn_id();
    assert_ne!(
        a, b,
        "conn_id must be unique per channel to prevent AES-GCM IV reuse; \
         successive calls must return different values",
    );
}

// D13-b: the returned value fits within 16 bits (reference: modulo 0xffff).
#[test]
fn conn_id_fits_within_u16() {
    let id = next_conn_id();
    assert!(
        id <= 0xffff,
        "conn_id must not exceed 0xffff (reference: modulo 0xffff); got {id}",
    );
}
