// SPDX-License-Identifier: AGPL-3.0-only
//! Step 18 — project_lat_lng mirrors sauce's processState branch.
//!
//! Sauce4zwift (`src/stats.mjs:3008-3012`):
//!
//! ```js
//! state.latlng = worldMeta.flippedHack ?
//!     [(state.x / (worldMeta.latDegDist * 100)) + worldMeta.latOffset,
//!         (state.y / (worldMeta.lonDegDist * 100)) + worldMeta.lonOffset] :
//!     [-(state.y / (worldMeta.latDegDist * 100)) + worldMeta.latOffset,
//!         (state.x / (worldMeta.lonDegDist * 100)) + worldMeta.lonOffset];
//! ```
//!
//! Ranchero returns `(lat, lng)` as a scalar tuple — the first intentional
//! payload divergence from sauce, which emits a `[lat, lng]` array.

use approx::assert_relative_eq;
use zwift_worlds::WorldMeta;

fn synthetic_world(flipped: bool) -> WorldMeta {
    WorldMeta {
        world_id:            1,
        course_id:           6,
        name:                "Test".into(),
        lat_offset:          10.0,
        lon_offset:          20.0,
        lat_deg_dist:        2.0,
        lon_deg_dist:        4.0,
        physics_slope_scale: 1.0,
        sea_level:           0.0,
        ele_offset:          0.0,
        flipped_hack:        flipped,
        water_plane_level:   0.0,
    }
}

#[test]
fn project_lat_lng_unflipped_negates_y_for_lat() {
    let meta = synthetic_world(false);
    // x = 200, y = 400, latDegDist = 2 (×100 = 200), lonDegDist = 4 (×100 = 400)
    // lat = -(400 / 200) + 10 = -2 + 10 = 8
    // lng =  (200 / 400) + 20 = 0.5 + 20 = 20.5
    let (lat, lng) = meta.project_lat_lng(200.0, 400.0);
    assert_relative_eq!(lat, 8.0,  max_relative = 1e-9);
    assert_relative_eq!(lng, 20.5, max_relative = 1e-9);
}

#[test]
fn project_lat_lng_flipped_drops_negation_and_swaps_axes() {
    let meta = synthetic_world(true);
    // Flipped: lat uses x / (latDegDist*100) + latOffset (no negation),
    //          lng uses y / (lonDegDist*100) + lonOffset.
    // lat = 200 / 200 + 10 = 11
    // lng = 400 / 400 + 20 = 21
    let (lat, lng) = meta.project_lat_lng(200.0, 400.0);
    assert_relative_eq!(lat, 11.0, max_relative = 1e-9);
    assert_relative_eq!(lng, 21.0, max_relative = 1e-9);
}

#[test]
fn project_lat_lng_returns_scalars_not_array() {
    // The contract is a 2-tuple of f64; this guards against a regression
    // that re-introduces an array shape to match sauce (see QE1 — payload
    // divergence is deliberate).
    let meta = synthetic_world(false);
    let result: (f64, f64) = meta.project_lat_lng(0.0, 0.0);
    let _ = result.0;
    let _ = result.1;
}
