// SPDX-License-Identifier: AGPL-3.0-only
//! 20.S6 — W′ balance and power zones configured from an athlete profile.
//!
//! **Red state**: two symbols are missing:
//! - `WBalAccumulator::configure_from_profile` — all four tests that touch
//!   W′ balance fail to compile.
//! - `zwift_stats::zones::get_power_zones` — tests 3 and 4 fail to compile.
//!
//! **Green state** (Step 6 implementation):
//! - `configure_from_profile(ftp, cp, w_prime)` implements the sauce
//!   `_updateAthleteDataFromDatabase` defaults: `cp || ftp`, `w_prime || 20000`.
//! - `get_power_zones(ftp)` wraps `coggan_zones` (the sauce default path).

use zwift_stats::{Sample, WBalAccumulator};
use zwift_stats::zones::{get_power_zones, ZonesAccumulator};

// S6-1: After profile-based configure with CP = FTP and W′ = 20 000,
// the accumulator depletes above CP and recovers below CP.
//
// Fails to compile in red state: `configure_from_profile` is absent on
// `WBalAccumulator`.
#[test]
fn wbal_configured_from_profile() {
    let ftp = 250.0;
    let mut acc = WBalAccumulator::new();
    acc.configure_from_profile(Some(ftp), None, None);

    assert_eq!(acc.cp(),      Some(ftp),     "CP must equal FTP when no explicit CP");
    assert_eq!(acc.w_prime(), Some(20_000.0), "W′ must default to 20 000");

    // Two ticks above FTP: W′ must decrease from 20 000.
    acc.accumulate(0.0, Sample::Value(ftp + 50.0));
    let depleted = acc.accumulate(1.0, Sample::Value(ftp + 50.0)).unwrap();
    assert!(
        depleted < 20_000.0,
        "W′ must deplete when power exceeds CP; got {depleted}",
    );

    // One tick below FTP: W′ must recover from the depleted value.
    let recovered = acc.accumulate(2.0, Sample::Value(ftp - 50.0)).unwrap();
    assert!(
        recovered > depleted,
        "W′ must recover when power is below CP; depleted={depleted}, recovered={recovered}",
    );
}

// S6-2: The profile-based configure fallback rules:
//   - missing CP  → use FTP as CP (sauce: `athlete.cp || athlete.ftp`)
//   - missing W′  → use 20 000   (sauce: `athlete.wPrime || wPrimeDefault`)
//   - no FTP, no CP → do not configure (accumulator stays unconfigured)
//   - explicit W′ → use the provided value instead of 20 000
//
// Fails to compile in red state: `configure_from_profile` is absent.
#[test]
fn wbal_defaults_when_no_cp() {
    // Missing CP → falls back to FTP.
    let mut acc = WBalAccumulator::new();
    acc.configure_from_profile(Some(300.0), None, None);
    assert_eq!(acc.cp(),      Some(300.0),   "CP must fall back to FTP when absent");
    assert_eq!(acc.w_prime(), Some(20_000.0), "W′ must default to 20 000 when absent");

    // Explicit CP takes precedence over FTP.
    let mut acc2 = WBalAccumulator::new();
    acc2.configure_from_profile(Some(300.0), Some(280.0), None);
    assert_eq!(acc2.cp(), Some(280.0), "explicit CP must take precedence over FTP");

    // No FTP and no CP → accumulator remains unconfigured.
    let mut acc3 = WBalAccumulator::new();
    acc3.configure_from_profile(None, None, None);
    assert_eq!(acc3.cp(), None, "no FTP and no CP must leave accumulator unconfigured");
    assert!(
        acc3.accumulate(0.0, Sample::Value(300.0)).is_none(),
        "unconfigured accumulator must return None from accumulate",
    );

    // Explicit W′ overrides the 20 000 default.
    let mut acc4 = WBalAccumulator::new();
    acc4.configure_from_profile(Some(300.0), None, Some(25_000.0));
    assert_eq!(acc4.w_prime(), Some(25_000.0), "explicit W′ must override the default");
}

// S6-3: `get_power_zones(ftp)` fed into `ZonesAccumulator::configure` must
// produce zones that accumulate time; an unconfigured accumulator stays empty.
//
// Fails to compile in red state: `get_power_zones` is absent from
// `zwift_stats::zones`.
#[test]
fn zones_configured_from_ftp() {
    // With valid FTP: zones accumulate time.
    let mut za = ZonesAccumulator::new();
    za.configure(250.0, get_power_zones(250.0));

    za.accumulate(0.0, 300.0); // establishes time_offset
    za.accumulate(1.0, 300.0); // 300 W is in Z5 (262.5, 300.0]
    assert!(
        za.value().iter().any(|z| z.time > 0.0),
        "at least one zone must have accumulated time after two ticks at 300 W",
    );

    // Without FTP (parity with sauce null-ftp path): zones stay empty.
    let za_no_ftp = ZonesAccumulator::new();
    assert!(
        za_no_ftp.value().is_empty(),
        "unconfigured ZonesAccumulator must report empty zones",
    );
}

// S6-4: `get_power_zones(ftp)` returns the Coggan 7-zone boundaries (sauce
// `getPowerZones` defaults to `cogganZones` when no custom type is set).
//
// Fails to compile in red state: `get_power_zones` is absent from
// `zwift_stats::zones`.
#[test]
fn get_power_zones_matches_coggan() {
    let ftp = 200.0;
    let zones = get_power_zones(ftp);

    assert_eq!(zones.len(), 7, "Coggan must have 7 zones");

    // Z1: 0 – 55 % of FTP
    assert!((zones[0].to.unwrap() - ftp * 0.55).abs() < 1e-9, "Z1 top must be 55 % of FTP");

    // Z4: 90 % – 105 % of FTP
    assert!((zones[3].from - ftp * 0.90).abs() < 1e-9, "Z4 bottom must be 90 % of FTP");
    assert!((zones[3].to.unwrap() - ftp * 1.05).abs() < 1e-9, "Z4 top must be 105 % of FTP");

    // Z7: 150 % of FTP to ∞
    assert!((zones[6].from - ftp * 1.50).abs() < 1e-9, "Z7 bottom must be 150 % of FTP");
    assert_eq!(zones[6].to, None, "Z7 must have no upper bound");
}
