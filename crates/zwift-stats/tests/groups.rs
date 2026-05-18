// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use zwift_stats::{compute_groups, NearbyEntry, GroupMeta};

fn make_entry(athlete_id: u32, gap: f64, draft: f64) -> NearbyEntry {
    NearbyEntry {
        athlete_id,
        gap,
        draft,
        weight: None,
        power: 0.0,
        heartrate: None,
        speed: 10.0,
        is_gap_est: false,
    }
}

// 15.22-T: compute_groups — clumping by gap threshold

#[test]
fn singleton_riders_get_group_id_none() {
    // gap 5.0 s > 2.0 s (draft present) → both riders are singletons
    let nearby = vec![
        make_entry(1, 0.0, 50.0),
        make_entry(2, 5.0, 50.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert!(
        groups.iter().all(|g| g.id.is_none()),
        "singleton groups should have no id"
    );
}

#[test]
fn two_riders_within_2_second_gap_form_one_group() {
    // gap 1.5 s < 2.0 s (draft present) → same group
    let nearby = vec![
        make_entry(1, 0.0, 50.0),
        make_entry(2, 1.5, 50.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1, "should form one group");
    assert!(groups[0].id.is_some(), "multi-rider group should have an id");
    assert_eq!(groups[0].members.len(), 2, "group should contain both riders");
}

#[test]
fn gap_above_2_seconds_splits_group() {
    // gap 2.1 s > 2.0 s (draft present) → two separate groups
    let nearby = vec![
        make_entry(1, 0.0, 50.0),
        make_entry(2, 2.1, 50.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 2, "gap > 2 s should split into two groups");
    let id0 = groups[0].id;
    let id1 = groups[1].id;
    assert!(
        id0.is_none() || id1.is_none() || id0 != id1,
        "split groups must not share an id"
    );
}

#[test]
fn gap_above_0_8_without_draft_splits_group() {
    // no draft, gap 1.0 s > 0.8 s → split
    let nearby = vec![
        make_entry(1, 0.0, 0.0),
        make_entry(2, 1.0, 0.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 2, "no draft + gap > 0.8 s should split");
}

#[test]
fn gap_at_or_below_0_8_with_no_draft_keeps_group() {
    // no draft, gap 0.7 s ≤ 0.8 s → same group
    let nearby = vec![
        make_entry(1, 0.0, 0.0),
        make_entry(2, 0.7, 0.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1, "no draft + gap ≤ 0.8 s should stay in same group");
    assert!(groups[0].id.is_some(), "multi-rider group should have an id");
}

// 15.23-T: compute_groups — aggregation

#[test]
fn aggregate_weight_skips_none_entries() {
    // weights [Some(70.0), None, Some(80.0)] → group.weight = Some(75.0)
    let nearby = vec![
        NearbyEntry { athlete_id: 1, gap: 0.0, draft: 0.0, weight: Some(70.0), power: 0.0, heartrate: None, speed: 10.0, is_gap_est: false },
        NearbyEntry { athlete_id: 2, gap: 0.4, draft: 0.0, weight: None,        power: 0.0, heartrate: None, speed: 10.0, is_gap_est: false },
        NearbyEntry { athlete_id: 3, gap: 0.7, draft: 0.0, weight: Some(80.0), power: 0.0, heartrate: None, speed: 10.0, is_gap_est: false },
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1, "all within 0.8 s → single group");
    let w = groups[0].weight.expect("group should have aggregate weight");
    assert!((w - 75.0).abs() < 1e-9, "average of 70 and 80 = 75.0, got {w}");
}

#[test]
fn aggregate_power_uses_mean_of_members() {
    // power [200.0, 300.0] → group.power = 250.0
    let nearby = vec![
        NearbyEntry { athlete_id: 1, gap: 0.0, draft: 0.0, weight: None, power: 200.0, heartrate: None, speed: 10.0, is_gap_est: false },
        NearbyEntry { athlete_id: 2, gap: 0.5, draft: 0.0, weight: None, power: 300.0, heartrate: None, speed: 10.0, is_gap_est: false },
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1);
    assert!((groups[0].power - 250.0).abs() < 1e-9, "mean power = 250.0, got {}", groups[0].power);
}

#[test]
fn aggregate_speed_uses_median_not_mean() {
    // speeds [10.0, 100.0, 20.0] → sorted [10.0, 20.0, 100.0] → median = 20.0
    // mean would be 43.3…; if mean is returned this test fails
    let nearby = vec![
        NearbyEntry { athlete_id: 1, gap: 0.0,  draft: 0.0, weight: None, power: 0.0, heartrate: None, speed: 10.0,  is_gap_est: false },
        NearbyEntry { athlete_id: 2, gap: 0.3,  draft: 0.0, weight: None, power: 0.0, heartrate: None, speed: 100.0, is_gap_est: false },
        NearbyEntry { athlete_id: 3, gap: 0.6,  draft: 0.0, weight: None, power: 0.0, heartrate: None, speed: 20.0,  is_gap_est: false },
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1);
    assert!((groups[0].speed - 20.0).abs() < 1e-9, "median speed = 20.0, got {}", groups[0].speed);
}

#[test]
fn aggregate_heartrate_skips_none_entries() {
    // hr [None, Some(150.0)] → group.heartrate = Some(150.0)
    let nearby = vec![
        NearbyEntry { athlete_id: 1, gap: 0.0, draft: 0.0, weight: None, power: 0.0, heartrate: None,          speed: 10.0, is_gap_est: false },
        NearbyEntry { athlete_id: 2, gap: 0.5, draft: 0.0, weight: None, power: 0.0, heartrate: Some(150.0), speed: 10.0, is_gap_est: false },
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1);
    let hr = groups[0].heartrate.expect("group heartrate should aggregate non-None entries");
    assert!((hr - 150.0).abs() < 1e-9, "only entry is 150.0, got {hr}");
}

#[test]
fn group_gap_is_zero_for_watching_group() {
    // watching_idx=0 → the group containing index 0 should have gap=0.0
    let nearby = vec![
        make_entry(1, 0.0, 50.0),
        make_entry(2, 1.0, 50.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].gap, 0.0, "watching group gap should be 0.0");
}

#[test]
fn group_gap_is_head_for_ahead_and_tail_for_behind() {
    // watching at 0.0; ahead group [5.0, 5.5] → gap=5.0; behind group [−4.0, −3.0] → gap=−4.0
    // Riders 10 and 11 use draft=50 so their 1.0 s delta stays below the 2.0 s threshold.
    let nearby = vec![
        NearbyEntry { athlete_id: 10, gap: -4.0, draft: 50.0, weight: None, power: 0.0, heartrate: None, speed: 10.0, is_gap_est: false },
        NearbyEntry { athlete_id: 11, gap: -3.0, draft: 50.0, weight: None, power: 0.0, heartrate: None, speed: 10.0, is_gap_est: false },
        make_entry(1,   0.0, 0.0),  // watching (index 2)
        make_entry(2,   5.0, 0.0),
        make_entry(3,   5.5, 0.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    // watching_idx=2 (athlete_id=1 is at index 2)
    let groups = compute_groups(&nearby, 2, &mut prior_groups, &mut next_id, 0.0);

    // expect 3 groups: behind, watching, ahead (in gap order)
    assert_eq!(groups.len(), 3, "three distinct groups expected");
    let behind = groups.iter().find(|g| g.gap < 0.0).expect("behind group");
    let ahead  = groups.iter().find(|g| g.gap > 0.0).expect("ahead group");
    assert_eq!(behind.gap, -4.0, "behind group gap = tail = -4.0");
    assert_eq!(ahead.gap,   5.0, "ahead group gap = head = 5.0");
}

#[test]
fn group_length_time_is_span_between_head_and_tail() {
    // group members at gaps [10.0, 11.5] → length_time = 11.5 - 10.0 = 1.5
    let nearby = vec![
        make_entry(1, 0.0,  0.0),  // watching
        make_entry(2, 10.0, 0.0),
        make_entry(3, 11.5, 0.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    // watching_idx=0; with no draft and gap > 0.8, watching is alone
    // riders 2 and 3 would split too (delta 1.5 > 0.8), so use draft=50 to keep them together
    let _nearby = nearby; // shadow to satisfy unused-variable lint
    let nearby = vec![
        make_entry(1,  0.0, 0.0),   // watching alone
        NearbyEntry { athlete_id: 2, gap: 10.0, draft: 50.0, weight: None, power: 0.0, heartrate: None, speed: 10.0, is_gap_est: false },
        NearbyEntry { athlete_id: 3, gap: 11.5, draft: 50.0, weight: None, power: 0.0, heartrate: None, speed: 10.0, is_gap_est: false },
    ];
    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    let ahead = groups.iter().find(|g| g.gap > 0.0).expect("ahead group");
    assert!((ahead.length_time - 1.5).abs() < 1e-9, "length_time = 11.5 - 10.0 = 1.5, got {}", ahead.length_time);
}

// 15.23-T: length_distance = length_time × median speed of the group
#[test]
fn last_group_length_distance_is_nonzero_and_matches_time_times_speed() {
    // Two riders in the ahead group at gaps 10.0 and 11.5 s, both at speed 20.0 m/s.
    // length_time = 1.5 s, median speed = 20.0 m/s → length_distance = 30.0 m.
    let nearby = vec![
        make_entry(1, 0.0, 0.0),   // watching, alone (gap > 0.8 s to next)
        NearbyEntry { athlete_id: 2, gap: 10.0, draft: 50.0, weight: None, power: 0.0, heartrate: None, speed: 20.0, is_gap_est: false },
        NearbyEntry { athlete_id: 3, gap: 11.5, draft: 50.0, weight: None, power: 0.0, heartrate: None, speed: 20.0, is_gap_est: false },
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 1u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    let ahead = groups.iter().find(|g| g.gap > 0.0).expect("ahead group");
    let expected = ahead.length_time * ahead.speed;
    assert!(
        (ahead.length_distance - expected).abs() < 1e-9,
        "length_distance = length_time × speed = {expected}, got {}",
        ahead.length_distance,
    );
    assert!(ahead.length_distance > 0.0, "length_distance must be nonzero for a multi-rider group");
}
