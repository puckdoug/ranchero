// SPDX-License-Identifier: AGPL-3.0-only
//! Zwift world-meta tables and position projection.
//!
//! Port target: sauce4zwift's `src/env.mjs` `worldMetas` table plus the
//! position-projection branch of `processState` (`stats.mjs:3005-3023`) and
//! `webMercatorProjection` (`env.mjs:261`).
//!
//! ## Coordinate transformations
//!
//! Sauce projects a player state's world XY into latitude/longitude using a
//! per-world affine map:
//!
//! ```text
//! lat = -(y / (latDegDist * 100)) + latOffset
//! lng =  (x / (lonDegDist * 100)) + lonOffset
//! ```
//!
//! Worlds flagged with `flippedHack = true` swap the X and Y roles and drop
//! the `lat` negation. Altitude is adjusted by `(z - seaLevel + eleOffset)
//! / 100 * physicsSlopeScale`, except inside portals where `z / 100 *
//! roadSlopeScaleOverride` is used instead (sauce `stats.mjs:3013-3022`).
//!
//! Ranchero deliberately emits `lat` and `lng` as separate scalars rather
//! than as a `latlng` array — this is the first intentional payload
//! divergence from sauce4zwift.

use std::sync::OnceLock;

use serde::Deserialize;

/// World-meta record for one Zwift course.
///
/// Field names match sauce4zwift's `worldMetas` entries verbatim except for
/// the rename to snake_case.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldMeta {
    pub world_id:             i32,
    pub course_id:            i32,
    pub name:                 String,
    pub lat_offset:           f64,
    pub lon_offset:           f64,
    pub lat_deg_dist:         f64,
    pub lon_deg_dist:         f64,
    pub physics_slope_scale:  f64,
    pub sea_level:            f64,
    #[serde(default)]
    pub ele_offset:           f64,
    #[serde(default)]
    pub flipped_hack:         bool,
    #[serde(default)]
    pub water_plane_level:    f64,
}

const WORLDS_JSON: &str = include_str!("../data/worlds.json");

fn worlds() -> &'static [WorldMeta] {
    static WORLDS: OnceLock<Vec<WorldMeta>> = OnceLock::new();
    WORLDS.get_or_init(|| {
        serde_json::from_str::<Vec<WorldMeta>>(WORLDS_JSON)
            .expect("vendored worlds.json must parse as a list of WorldMeta records")
    })
}

impl WorldMeta {
    /// Look up a world by its Zwift `courseId`.
    pub fn by_course_id(course_id: i32) -> Option<&'static WorldMeta> {
        worlds().iter().find(|w| w.course_id == course_id)
    }

    /// Project a player state's world XY (centimetres in Zwift units) to
    /// (latitude, longitude) degrees per sauce4zwift's
    /// `processState` branch (`stats.mjs:3005-3023`).
    pub fn project_lat_lng(&self, x: f64, y: f64) -> (f64, f64) {
        if self.flipped_hack {
            let lat = x / (self.lat_deg_dist * 100.0) + self.lat_offset;
            let lng = y / (self.lon_deg_dist * 100.0) + self.lon_offset;
            (lat, lng)
        } else {
            let lat = -(y / (self.lat_deg_dist * 100.0)) + self.lat_offset;
            let lng = x / (self.lon_deg_dist * 100.0) + self.lon_offset;
            (lat, lng)
        }
    }

    /// Adjust raw `z` (cm) to altitude in metres using this world's
    /// `seaLevel`, `eleOffset`, and `physicsSlopeScale`.
    pub fn adjust_altitude(&self, z: f64) -> f64 {
        (z - self.sea_level + self.ele_offset) / 100.0 * self.physics_slope_scale
    }

    /// Adjust raw `z` (cm) to altitude in metres for portal segments, using
    /// the road's `physicsSlopeScaleOverride`.
    pub fn adjust_altitude_portal(z: f64, slope_scale_override: f64) -> f64 {
        z / 100.0 * slope_scale_override
    }
}

/// All known world-metas.
pub fn all() -> Vec<&'static WorldMeta> {
    worlds().iter().collect()
}

/// Google Web-Mercator projection from (lat, lng) degrees to normalised
/// `(x, y)` tile coordinates in `[0, 1]`. Mirrors sauce4zwift's
/// `webMercatorProjection` (`env.mjs:261`).
pub fn web_mercator_projection(lat: f64, lng: f64) -> (f64, f64) {
    let siny = (lat * std::f64::consts::PI / 180.0).sin().clamp(-0.9999, 0.9999);
    let x = 0.5 + lng / 360.0;
    let y = 0.5 - ((1.0 + siny) / (1.0 - siny)).ln() / (4.0 * std::f64::consts::PI);
    (x, y)
}
