// SPDX-License-Identifier: AGPL-3.0-only
//! Step 18 — webMercatorProjection from `env.mjs:261`.
//!
//! ```js
//! export function webMercatorProjection([lat, lng]) {
//!     let siny = Math.sin((lat * Math.PI) / 180);
//!     siny = Math.min(Math.max(siny, -0.9999), 0.9999);
//!     return [
//!         0.5 + lng / 360,
//!         0.5 - Math.log((1 + siny) / (1 - siny)) / (4 * Math.PI),
//!     ];
//! }
//! ```

use approx::assert_relative_eq;
use zwift_worlds::web_mercator_projection;

#[test]
fn origin_maps_to_centre_of_tile() {
    let (x, y) = web_mercator_projection(0.0, 0.0);
    assert_relative_eq!(x, 0.5, max_relative = 1e-12);
    assert_relative_eq!(y, 0.5, max_relative = 1e-12);
}

#[test]
fn lng_shifts_x_linearly() {
    let (x, _) = web_mercator_projection(0.0, 180.0);
    assert_relative_eq!(x, 1.0, max_relative = 1e-12);
    let (x, _) = web_mercator_projection(0.0, -180.0);
    assert_relative_eq!(x, 0.0, max_relative = 1e-12);
}

#[test]
fn extreme_latitudes_are_clamped() {
    // sin(90°) = 1 is clamped to 0.9999 so the log doesn't go to -infinity.
    // The exact value is 0.5 - ln((1+0.9999)/(1-0.9999)) / (4π).
    let expected = 0.5 - ((1.0 + 0.9999_f64) / (1.0 - 0.9999_f64)).ln() / (4.0 * std::f64::consts::PI);
    let (_, y_north) = web_mercator_projection(90.0, 0.0);
    let (_, y_south) = web_mercator_projection(-90.0, 0.0);
    assert_relative_eq!(y_north, expected,       max_relative = 1e-12);
    assert_relative_eq!(y_south, 1.0 - expected, max_relative = 1e-12);
}

#[test]
fn known_reference_point() {
    // London (51.5074°N, -0.1278°E) — well-known reference.
    // x = 0.5 + (-0.1278 / 360)
    // siny = sin(51.5074 * pi / 180); y = 0.5 - ln((1+siny)/(1-siny)) / (4π)
    let (x, y) = web_mercator_projection(51.5074, -0.1278);
    let expected_x = 0.5 + (-0.1278_f64) / 360.0;
    let siny = (51.5074_f64 * std::f64::consts::PI / 180.0).sin().clamp(-0.9999, 0.9999);
    let expected_y = 0.5 - ((1.0 + siny) / (1.0 - siny)).ln() / (4.0 * std::f64::consts::PI);
    assert_relative_eq!(x, expected_x, max_relative = 1e-12);
    assert_relative_eq!(y, expected_y, max_relative = 1e-12);
}
