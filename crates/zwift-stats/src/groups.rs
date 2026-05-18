// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::{HashMap, HashSet};
use std::cmp::Ordering;
use crate::athlete::GroupMeta;

#[derive(Debug, Clone)]
pub struct NearbyEntry {
    pub athlete_id: u32,
    pub gap: f64,
    pub draft: f64,
    pub weight: Option<f64>,
    pub power: f64,
    pub heartrate: Option<f64>,
    pub speed: f64,
    pub is_gap_est: bool,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub id: Option<u32>,
    pub members: Vec<u32>,
    pub gap: f64,
    pub length_time: f64,
    // length_distance = length_time × group median speed (port of JS groups.mjs approach;
    // median speed chosen over watching athlete's speed for self-contained derivation).
    pub length_distance: f64,
    pub weight: Option<f64>,
    pub power: f64,
    pub draft: f64,
    pub speed: f64,
    pub heartrate: Option<f64>,
    pub is_gap_est: bool,
}

pub fn compute_groups(
    nearby: &[NearbyEntry],
    watching_idx: usize,
    prior_groups: &mut HashMap<u32, GroupMeta>,
    next_id: &mut u32,
    now: f64,
) -> Vec<Group> {
    if nearby.is_empty() {
        prior_groups.clear();
        return Vec::new();
    }

    // Pass 1: Clump consecutive entries by gap threshold.
    // Threshold is 2.0 s when either neighbour has draft, else 0.8 s.
    let mut clumps: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = vec![0];
    for i in 1..nearby.len() {
        let prev = &nearby[i - 1];
        let curr = &nearby[i];
        let threshold = if prev.draft > 0.0 || curr.draft > 0.0 { 2.0 } else { 0.8 };
        if (curr.gap - prev.gap).abs() > threshold {
            clumps.push(std::mem::take(&mut current));
        }
        current.push(i);
    }
    clumps.push(current);

    // Pass 2: Aggregate fields for each clump.
    let mut groups: Vec<Group> = clumps.iter().map(|clump| {
        let contains_watching = clump.contains(&watching_idx);
        let members: Vec<u32> = clump.iter().map(|&i| nearby[i].athlete_id).collect();

        let gap_vals: Vec<f64> = clump.iter().map(|&i| nearby[i].gap).collect();
        let min_gap = gap_vals.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_gap = gap_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        let gap = if contains_watching { 0.0 } else { min_gap };
        let length_time = max_gap - min_gap;

        let weights: Vec<f64> = clump.iter().filter_map(|&i| nearby[i].weight).collect();
        let weight = if weights.is_empty() {
            None
        } else {
            Some(weights.iter().sum::<f64>() / weights.len() as f64)
        };

        let n = clump.len() as f64;
        let power = clump.iter().map(|&i| nearby[i].power).sum::<f64>() / n;
        let draft = clump.iter().map(|&i| nearby[i].draft).sum::<f64>() / n;

        let mut speeds: Vec<f64> = clump.iter().map(|&i| nearby[i].speed).collect();
        speeds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
        let mid = speeds.len() / 2;
        let speed = if speeds.len() % 2 == 0 {
            (speeds[mid - 1] + speeds[mid]) / 2.0
        } else {
            speeds[mid]
        };

        let hrs: Vec<f64> = clump.iter().filter_map(|&i| nearby[i].heartrate).collect();
        let heartrate = if hrs.is_empty() {
            None
        } else {
            Some(hrs.iter().sum::<f64>() / hrs.len() as f64)
        };

        let is_gap_est = clump.iter().any(|&i| nearby[i].is_gap_est);

        Group {
            id: None,
            members,
            gap,
            length_time,
            length_distance: length_time * speed,
            weight,
            power,
            draft,
            speed,
            heartrate,
            is_gap_est,
        }
    }).collect();

    // Pass 3: Jaccard identity — reuse prior group ids where continuity exceeds 0.5.
    let old_priors: HashMap<u32, GroupMeta> = std::mem::take(prior_groups);
    let mut used_prior_ids: HashSet<u32> = HashSet::new();

    for group in groups.iter_mut() {
        if group.members.len() < 2 {
            continue;
        }

        let identity: HashSet<u32> = group.members.iter().copied().collect();

        let best = old_priors.iter()
            .filter(|(pid, _)| !used_prior_ids.contains(pid))
            .filter_map(|(pid, meta)| {
                let inter = identity.intersection(&meta.identity_set).count() as f64;
                let union = identity.union(&meta.identity_set).count() as f64;
                let j = if union == 0.0 { 0.0 } else { inter / union };
                if j > 0.5 { Some((*pid, j)) } else { None }
            })
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(Ordering::Equal));

        let assigned_id = if let Some((pid, _)) = best {
            used_prior_ids.insert(pid);
            pid
        } else {
            let new_id = *next_id;
            *next_id += 1;
            new_id
        };

        group.id = Some(assigned_id);
        prior_groups.insert(assigned_id, GroupMeta {
            id: assigned_id,
            identity_set: identity,
            accessed: now,
        });
    }

    groups
}
