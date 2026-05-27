// SPDX-License-Identifier: AGPL-3.0-only
//! Step 18 — altitude adjustment uses worldMeta values.
//!
//! Sauce computes
//! `altitude = (z - seaLevel + eleOffset) / 100 * physicsSlopeScale`
//! (`stats.mjs:3022`).  Today ranchero divides raw `z` by 100 and ignores the
//! sea-level offset and slope scale, so a player state on a world whose
//! `seaLevel` is non-zero records a wrong altitude.

use approx::assert_relative_eq;
use zwift_worlds::WorldMeta;

#[test]
fn adjust_altitude_applies_sea_level_and_slope_scale() {
    // A synthetic but realistic world: sea level 12000 cm, slope scale 100,
    // a +500 cm elevation offset.  z = 50000 cm:
    //   ((50000 - 12000 + 500) / 100) * 100 = 38500 m  (× 100 = 3850000)
    // Wait — the slope scale is dimensionless and large in sauce data
    // (worldMetas[real-earth] uses 100).  This test asserts the exact
    // arithmetic sauce performs at stats.mjs:3022, not its physical units.
    let meta = WorldMeta {
        world_id:            1,
        course_id:           6,
        name:                "Test".into(),
        lat_offset:          0.0,
        lon_offset:          0.0,
        lat_deg_dist:        1.0,
        lon_deg_dist:        1.0,
        physics_slope_scale: 1.0,
        sea_level:           12_000.0,
        ele_offset:          500.0,
        flipped_hack:        false,
        water_plane_level:   0.0,
    };
    // (50_000 - 12_000 + 500) / 100 * 1.0 = 385.0
    assert_relative_eq!(meta.adjust_altitude(50_000.0), 385.0, max_relative = 1e-9);
}

#[test]
fn adjust_altitude_with_zero_offsets_matches_raw_divided_by_100() {
    let meta = WorldMeta {
        world_id:            1,
        course_id:           6,
        name:                "Test".into(),
        lat_offset:          0.0,
        lon_offset:          0.0,
        lat_deg_dist:        1.0,
        lon_deg_dist:        1.0,
        physics_slope_scale: 1.0,
        sea_level:           0.0,
        ele_offset:          0.0,
        flipped_hack:        false,
        water_plane_level:   0.0,
    };
    assert_relative_eq!(meta.adjust_altitude(12_345.0), 123.45, max_relative = 1e-9);
}

#[test]
fn adjust_altitude_portal_uses_override_only() {
    // Portal branch (sauce stats.mjs:3020): state.altitude = z / 100 * slope_override.
    // The world's seaLevel/eleOffset are explicitly ignored — portals normalise
    // z separately.
    assert_relative_eq!(
        WorldMeta::adjust_altitude_portal(20_000.0, 1.0),
        200.0,
        max_relative = 1e-9
    );
    assert_relative_eq!(
        WorldMeta::adjust_altitude_portal(20_000.0, 0.5),
        100.0,
        max_relative = 1e-9
    );
}
