// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::calc_tss;

#[test]
fn tss_at_threshold() {
    // TSS = (seconds * np * (np / ftp)) / (ftp * 3600) * 100
    // At threshold where np == ftp, TSS = (seconds * ftp * 1) / (ftp * 3600) * 100
    //                                    = seconds / 3600 * 100
    // For 3600 seconds at ftp: TSS = 3600 / 3600 * 100 = 100
    // For 1800 seconds at ftp: TSS = 1800 / 3600 * 100 = 50

    let tss = calc_tss(250.0, 3600.0, 250.0);
    assert!(tss.is_some());
    assert!((tss.unwrap() - 100.0).abs() < 0.1, "TSS at ftp for 3600s should be 100");
}

#[test]
fn tss_zero_seconds() {
    // Zero seconds should yield zero TSS.

    let tss = calc_tss(200.0, 0.0, 250.0);
    assert!(tss.is_some());
    assert!(tss.unwrap() < 0.01, "TSS for 0 seconds should be ~0");
}

#[test]
fn tss_zero_ftp_returns_none() {
    // FTP of 0 (invalid) should return None.

    let tss = calc_tss(200.0, 3600.0, 0.0);
    assert!(tss.is_none(), "TSS with zero FTP should return None");
}

#[test]
fn tss_known_vector() {
    // Known example: 300 seconds at 300W normalized power, with 250W FTP.
    // TSS = (300 * 300 * (300/250)) / (250 * 3600) * 100
    //     = (300 * 300 * 1.2) / 900000 * 100
    //     = 108000 / 900000 * 100
    //     = 0.12 * 100
    //     = 12

    let tss = calc_tss(300.0, 300.0, 250.0);
    assert!(tss.is_some());
    assert!((tss.unwrap() - 12.0).abs() < 0.1, "TSS should be ~12 for known vector");
}
