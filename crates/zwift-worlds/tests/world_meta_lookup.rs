// SPDX-License-Identifier: AGPL-3.0-only
//! Step 18 — world-meta tables resolve a known course id.
//!
//! Watopia is `worldId = 1`, `courseId = 6` in sauce4zwift's vendored
//! `worldlist.json`. A bare-minimum vendoring of the worlds table must at
//! least produce a Watopia entry, since the bulk of Zwift's player states
//! arrive on this course and every other Step-18 path depends on the lookup
//! succeeding.

use zwift_worlds::WorldMeta;

#[test]
fn watopia_world_meta_resolves_by_course_id() {
    let meta = WorldMeta::by_course_id(6)
        .expect("Watopia (course 6) must be present in the vendored worlds table");
    assert_eq!(meta.course_id, 6);
    assert_eq!(meta.world_id, 1);
    assert!(
        meta.name.to_lowercase().contains("watopia"),
        "expected Watopia name, got {:?}",
        meta.name
    );
    // The projection map must be configured for every world; a zeroed
    // `latDegDist` or `lonDegDist` would silently divide by zero in
    // `project_lat_lng`.
    assert!(meta.lat_deg_dist != 0.0, "latDegDist must be non-zero");
    assert!(meta.lon_deg_dist != 0.0, "lonDegDist must be non-zero");
}

#[test]
fn unknown_course_id_returns_none() {
    assert!(
        WorldMeta::by_course_id(-9999).is_none(),
        "an unknown course id must resolve to None, not a placeholder"
    );
}
