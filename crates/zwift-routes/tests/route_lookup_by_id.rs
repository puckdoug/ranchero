// SPDX-License-Identifier: AGPL-3.0-only
//! Step 17 — `route_lookup_by_id`.
//!
//! A known route resolves by `routeId` to its distance, world, and segment
//! list. Source values match the row for "Lady Liberty" (id `5103974`) as
//! vendored from zwift-data's `src/routes.ts`.

use approx::assert_relative_eq;
use zwift_routes::Route;

const LADY_LIBERTY_ID: u64 = 5103974;

#[test]
fn lady_liberty_resolves_to_its_distance_world_and_segments() {
    let route = Route::by_id(LADY_LIBERTY_ID)
        .expect("Lady Liberty (id 5103974) must be present in the route table");

    assert_eq!(route.id, Some(LADY_LIBERTY_ID));
    assert_eq!(route.name, "Lady Liberty");
    assert_eq!(route.slug, "lady-liberty");
    assert_eq!(route.world, "new-york");
    assert!(route.lap);

    // Lap distance is 12.361 km in the upstream data.
    assert_relative_eq!(route.distance, 12.361, epsilon = 1e-9);

    // Both named segments are listed (order matches the source).
    assert_eq!(
        route.segments,
        vec![
            "new-york-kom-rev".to_string(),
            "new-york-sprint-rev".to_string(),
        ],
    );

    // Lady Liberty has one segment-on-route entry: new-york-kom-rev from
    // 2.751 km to 3.893 km.
    assert_eq!(route.segments_on_route.len(), 1);
    let seg = &route.segments_on_route[0];
    assert_eq!(seg.segment, "new-york-kom-rev");
    assert_relative_eq!(seg.from, 2.751, epsilon = 1e-9);
    assert_relative_eq!(seg.to,   3.893, epsilon = 1e-9);
}

#[test]
fn unknown_route_id_returns_none() {
    assert!(Route::by_id(0).is_none());
    assert!(Route::by_id(u64::MAX).is_none());
}
