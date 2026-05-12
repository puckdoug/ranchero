// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::{HashMap, HashSet};
use zwift_stats::{compute_groups, NearbyEntry, GroupMeta};

fn make_entry(athlete_id: u32, gap: f64) -> NearbyEntry {
    NearbyEntry {
        athlete_id,
        gap,
        draft: 50.0,
        weight: None,
        power: 0.0,
        heartrate: None,
        speed: 10.0,
        is_gap_est: false,
    }
}

fn make_prior(id: u32, members: &[u32]) -> GroupMeta {
    GroupMeta {
        id,
        identity_set: members.iter().copied().collect::<HashSet<u32>>(),
    }
}

// 15.24-T: compute_groups — Jaccard identity continuity

#[test]
fn group_retains_id_when_jaccard_above_half() {
    // prior group 7 had members {1, 2, 3}; new group has members {1, 2, 3}
    // Jaccard = 3/3 = 1.0 > 0.5 → same id
    let nearby = vec![
        make_entry(1, 0.0),
        make_entry(2, 0.5),
        make_entry(3, 1.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    prior_groups.insert(7, make_prior(7, &[1, 2, 3]));
    let mut next_id = 100u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, Some(7), "Jaccard=1.0 → retains prior id 7");
}

#[test]
fn group_gets_new_id_when_jaccard_at_or_below_half() {
    // prior group 7 had members {1, 2, 3, 4}; new group has only {1, 2}
    // Jaccard = 2/4 = 0.5 → not strictly > 0.5 → new id
    let nearby = vec![
        make_entry(1, 0.0),
        make_entry(2, 0.5),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    prior_groups.insert(7, make_prior(7, &[1, 2, 3, 4]));
    let mut next_id = 100u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1);
    assert_ne!(groups[0].id, Some(7), "Jaccard=0.5 is not strictly > 0.5 → new id assigned");
}

#[test]
fn new_multi_rider_group_gets_fresh_id_when_no_prior_matches() {
    // no prior groups at all → new id allocated from next_id counter
    let nearby = vec![
        make_entry(1, 0.0),
        make_entry(2, 0.5),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 42u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, Some(42), "first fresh id should be 42");
    assert_eq!(next_id, 43, "next_id incremented after allocation");
}

#[test]
fn greedy_first_match_wins_prior_meta() {
    // Two separate groups both overlap prior group 7, but only the first (in gap order)
    // can claim it. Requires two groups to independently score Jaccard > 0.5 against prior 7,
    // which is impossible with disjoint member sets — so this test proves the case where
    // the first group claims the prior and the second gets a new id.
    //
    // prior 7 = {1, 2, 3}; group A = {1, 2, 3} → Jaccard=1.0; group B = {4, 5, 6} → Jaccard=0.
    // Group A claims id=7; group B gets next_id=100.
    let nearby = vec![
        make_entry(1, 0.0),
        make_entry(2, 0.5),
        make_entry(3, 1.0),
        make_entry(4, 5.0),   // gap > 2 s → separate group
        make_entry(5, 5.5),
        make_entry(6, 6.0),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    prior_groups.insert(7, make_prior(7, &[1, 2, 3]));
    let mut next_id = 100u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert_eq!(groups.len(), 2);
    let g_a = groups.iter().find(|g| g.members.contains(&1)).expect("group A");
    let g_b = groups.iter().find(|g| g.members.contains(&4)).expect("group B");
    assert_eq!(g_a.id, Some(7), "group A retains prior id 7");
    assert_ne!(g_b.id, Some(7), "group B cannot reuse prior id 7");
    assert!(g_b.id.is_some(), "group B (multi-rider) should get a new id");
}

#[test]
fn prior_groups_map_updated_to_reflect_surviving_groups() {
    // After compute_groups, prior_groups should contain only the groups that survived
    // this tick (so stale entries from prior ticks can be GC'd by the caller).
    let nearby = vec![
        make_entry(1, 0.0),
        make_entry(2, 0.5),
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    prior_groups.insert(99, make_prior(99, &[10, 11])); // stale — members 10/11 not present
    let mut next_id = 1u32;

    let _groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert!(!prior_groups.contains_key(&99), "stale prior group should be removed");
}

#[test]
fn singleton_group_does_not_consume_next_id() {
    // A singleton (one rider) gets id=None and must not increment next_id.
    let nearby = vec![
        make_entry(1, 0.0),
        make_entry(2, 5.0),  // gap 5 s > 2 s → split; both singletons
    ];
    let mut prior_groups: HashMap<u32, GroupMeta> = HashMap::new();
    let mut next_id = 50u32;

    let groups = compute_groups(&nearby, 0, &mut prior_groups, &mut next_id, 0.0);

    assert!(groups.iter().all(|g| g.id.is_none()), "singletons have no id");
    assert_eq!(next_id, 50, "next_id must not be incremented for singletons");
}
