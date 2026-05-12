// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::zones::{coggan_zones, sweetspot_zone, ZonesAccumulator, SweetspotKind};

#[test]
fn accumulate_credits_top_down_with_break_on_non_overlap() {
    let mut acc = ZonesAccumulator::new();
    let zones = coggan_zones(250.0);
    acc.configure(250.0, zones);

    acc.accumulate(0.0, 300.0);
    acc.accumulate(1.0, 300.0);

    let value = acc.value();
    // 300 W should hit Z5 (262.5, 300.0]
    assert_eq!(value[4].time, 1.0, "Z5 should have 1.0 s after two ticks");
    for i in 0..7 {
        if i != 4 {
            assert_eq!(value[i].time, 0.0, "Zone {} should have 0.0 s", i + 1);
        }
    }
}

#[test]
fn accumulate_continues_iteration_on_overlap_for_sweetspot() {
    let mut acc = ZonesAccumulator::new();
    let mut zones = coggan_zones(250.0);
    let sweetspot = sweetspot_zone(250.0, SweetspotKind::Fascat);
    zones.push(sweetspot);
    acc.configure(250.0, zones);

    acc.accumulate(0.0, 220.0);
    acc.accumulate(1.0, 220.0);

    let value = acc.value();
    // 220 W is within Z3 (187.5, 225.0] and the sweetspot (210.0, 242.5]
    // Since sweetspot has overlap: true, both should get credited
    let z3_idx = 2;
    let ss_idx = 7;
    assert_eq!(value[z3_idx].time, 1.0, "Z3 should have 1.0 s");
    assert_eq!(value[ss_idx].time, 1.0, "Sweetspot should have 1.0 s due to overlap");
}

#[test]
fn accumulate_handles_zero_and_top_bounds() {
    let mut acc = ZonesAccumulator::new();
    let zones = coggan_zones(250.0);
    acc.configure(250.0, zones);

    acc.accumulate(0.0, 0.0);
    acc.accumulate(1.0, 0.0);

    let value = acc.value();
    // 0.0 W should hit no zone (from = 0 is exclusive)
    for i in 0..7 {
        assert_eq!(value[i].time, 0.0, "Zone {} should have 0.0 s for 0.0 W", i + 1);
    }

    // Reset and test high value
    acc.reset();
    acc.configure(250.0, coggan_zones(250.0));

    acc.accumulate(0.0, 1e9);
    acc.accumulate(1.0, 1e9);

    let value = acc.value();
    // 1e9 W should hit Z7 (375.0, None]
    assert_eq!(value[6].time, 1.0, "Z7 should have 1.0 s for 1e9 W");
    for i in 0..6 {
        assert_eq!(value[i].time, 0.0, "Zone {} should have 0.0 s for 1e9 W", i + 1);
    }
}

#[test]
fn accumulate_first_tick_yields_zero_elapsed() {
    let mut acc = ZonesAccumulator::new();
    let zones = coggan_zones(250.0);
    acc.configure(250.0, zones);

    acc.accumulate(5.0, 200.0);
    let value_after_first = acc.value();
    // First call should add 0 to every zone
    for i in 0..7 {
        assert_eq!(value_after_first[i].time, 0.0, "Zone {} should have 0.0 s after first tick", i + 1);
    }

    acc.accumulate(6.0, 200.0);
    let value_after_second = acc.value();
    // 200 W is within Z2 (137.5, 187.5] — wait, that's wrong. 200 W is between 187.5 and 225.0, so it's Z3.
    // Second tick should add 1.0 s (6.0 - 5.0) to the matching zone
    assert_eq!(value_after_second[2].time, 1.0, "Z3 should have 1.0 s after second tick (200 W matches Z3)");
}

#[test]
fn reset_clears_value_and_ftp() {
    let mut acc = ZonesAccumulator::new();
    let zones = coggan_zones(250.0);
    acc.configure(250.0, zones);

    acc.accumulate(0.0, 200.0);
    acc.accumulate(1.0, 200.0);

    let value_before = acc.value();
    assert!(value_before.iter().any(|z| z.time > 0.0), "Should have credits before reset");

    acc.reset();
    let value_after = acc.value();
    assert!(value_after.is_empty(), "value() should be empty after reset");

    // After reset, FTP should be None
    acc.accumulate(5.0, 200.0);
    let value_after_accumulate = acc.value();
    assert!(value_after_accumulate.is_empty(), "accumulate should not credit when unconfigured after reset");
}

#[test]
fn clone_continue_carries_state() {
    let mut acc = ZonesAccumulator::new();
    let zones = coggan_zones(250.0);
    acc.configure(250.0, zones);

    acc.accumulate(0.0, 200.0);
    acc.accumulate(1.0, 200.0);

    let value_source = acc.value();
    let z3_time_source = value_source[2].time;

    let mut acc_clone = acc.clone_continue();

    // Clone should have the same state
    let value_clone = acc_clone.value();
    assert_eq!(value_clone[2].time, z3_time_source, "clone_continue should carry source state");

    // Subsequent writes should diverge
    acc.accumulate(2.0, 200.0);
    acc_clone.accumulate(2.0, 300.0);

    let value_source_after = acc.value();
    let value_clone_after = acc_clone.value();
    assert_eq!(value_source_after[2].time, z3_time_source + 1.0, "source should have additional Z3 time");
    assert!(value_clone_after[2].time < value_source_after[2].time || value_clone_after[4].time > 0.0, "clone should diverge");
}

#[test]
fn clone_reset_starts_fresh() {
    let mut acc = ZonesAccumulator::new();
    let zones = coggan_zones(250.0);
    acc.configure(250.0, zones);

    acc.accumulate(0.0, 200.0);
    acc.accumulate(1.0, 200.0);

    let mut acc_clone = acc.clone_reset();

    // Clone should have same FTP and zones but reset time
    let value_clone = acc_clone.value();
    for z in value_clone {
        assert_eq!(z.time, 0.0, "clone_reset should start with 0.0 s for all zones");
    }

    // Verify we can still accumulate
    acc_clone.accumulate(10.0, 200.0);
    acc_clone.accumulate(11.0, 200.0);
    let value_after_accumulate = acc_clone.value();
    assert_eq!(value_after_accumulate[2].time, 1.0, "clone_reset should accumulate normally");
}
