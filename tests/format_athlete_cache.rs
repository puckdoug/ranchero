// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 2 — failing tests for profile-cache integration with the v1 formatter.
//
// Red state: CachedProfile does not exist and format_athlete_data_v1 still
// takes ftp: Option<f64> as its fourth argument. All tests below fail to
// compile until Step 2 implementation:
//   a) defines CachedProfile in src/web/format.rs (or re-exports it from there)
//   b) replaces the ftp parameter with profile: Option<&CachedProfile>
//   c) uses profile.ftp to compute TSS and profile fields to populate `athlete`

mod support;

use ranchero::web::format::{format_athlete_data_v1, CachedProfile};
use support::{build_athlete, SampleRow};

fn make_power_script() -> Vec<SampleRow> {
    // 70 seconds of constant 200 W — enough for the 30 s NP window to fill
    // so that np() returns Some(value) and TSS can be computed.
    (0..70)
        .map(|i| SampleRow {
            ts:      i as f64,
            power:   200.0,
            hr:      140.0,
            speed:   30.0,
            cadence: 90.0,
            draft:   0.0,
        })
        .collect()
}

// S2-C: When a CachedProfile is provided, the v1 athlete block's `athlete`
// field is a non-null object containing the profile name and FTP.
//
// Resolves gap G1 (athlete always null) from 20.20 item 1.
#[test]
fn format_v1_populates_athlete_from_cache() {
    let ad = build_athlete(&[]);
    let profile = CachedProfile {
        first_name: Some("Test".into()),
        last_name:  Some("Rider".into()),
        ftp:        Some(220),
        weight_g:   Some(72_000),
    };

    let result = format_athlete_data_v1(&ad, None, None, Some(&profile), 0.0, 0.0);

    assert_ne!(
        result["athlete"],
        serde_json::Value::Null,
        "athlete must be non-null when a CachedProfile is provided; \
         this is gap G1 from 20.20 item 1",
    );
    assert_eq!(
        result["athlete"]["ftp"],
        serde_json::json!(220),
        "athlete.ftp must reflect the CachedProfile ftp value",
    );
}

// S2-D: With power data (NP calculable) and an FTP in the CachedProfile,
// stats.power.tss must be non-null.
//
// Resolves gap G2 (tss always null because ftp never reached the formatter).
#[test]
fn tss_computed_from_ftp() {
    let ad = build_athlete(&make_power_script());
    let profile = CachedProfile {
        first_name: None,
        last_name:  None,
        ftp:        Some(250),
        weight_g:   None,
    };

    let result = format_athlete_data_v1(&ad, None, None, Some(&profile), 0.0, 0.0);

    assert_ne!(
        result["stats"]["power"]["tss"],
        serde_json::Value::Null,
        "stats.power.tss must be non-null when the CachedProfile supplies \
         an FTP and there is enough power data for NP; this is gap G2",
    );
}

// S2-E: When no CachedProfile is available (cache miss), the `athlete`
// field serialises as null — matching sauce4zwift's parity behaviour while
// its background fetch is pending.
#[test]
fn athlete_null_on_cache_miss() {
    let ad = build_athlete(&[]);

    let result = format_athlete_data_v1(&ad, None, None, None, 0.0, 0.0);

    assert_eq!(
        result["athlete"],
        serde_json::Value::Null,
        "athlete must be null when no CachedProfile is available (cache miss); \
         this matches sauce4zwift's parity behaviour",
    );
}
