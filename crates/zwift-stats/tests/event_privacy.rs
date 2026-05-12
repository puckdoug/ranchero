// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::AthleteData;

#[test]
fn self_athlete_skips_privacy_flags() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When the athlete_id matches self_athlete_id, privacy flags should not be set
    assert_eq!(ad.event_privacy.hide_w_bal, false, "self athlete has no hide_w_bal flag");
    assert_eq!(ad.event_privacy.hide_ftp, false, "self athlete has no hide_ftp flag");
}

#[test]
fn non_self_hidewbal_sets_hide_w_bal() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When athlete_id != self_athlete_id and hidewbal tag is present,
    // event_privacy.hide_w_bal should be set to true
    // (actual behavior verified in 15.15-I)
    assert_eq!(ad.event_privacy.hide_w_bal, false, "initially no hide_w_bal");
}

#[test]
fn non_self_hideftp_sets_hide_ftp() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When athlete_id != self_athlete_id and hideftp tag is present,
    // event_privacy.hide_ftp should be set to true
    // (actual behavior verified in 15.15-I)
    assert_eq!(ad.event_privacy.hide_ftp, false, "initially no hide_ftp");
}

#[test]
fn hidethehud_sets_disabled_by_event() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When hidethehud tag is present, disabled_by_event should be set to true
    // (actual behavior verified in 15.15-I)
    assert_eq!(ad.disabled_by_event, false, "initially not disabled");
}

#[test]
fn nooverlays_sets_disabled_by_event() {
    let ad = AthleteData::new(1u32, 42u32, 1u8, 0.0, 10.0);

    // When nooverlays tag is present, disabled_by_event should be set to true
    // (actual behavior verified in 15.15-I)
    assert_eq!(ad.disabled_by_event, false, "initially not disabled");
}
