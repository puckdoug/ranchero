// SPDX-License-Identifier: AGPL-3.0-only

use zwift_stats::zones::{coggan_zones, polarized_zones, sweetspot_zone, SweetspotKind};

#[test]
fn coggan_zones_at_ftp_250_match_js_table() {
    let zones = coggan_zones(250.0);
    assert_eq!(zones.len(), 7);

    assert_eq!(zones[0].label, "Z1");
    assert_eq!(zones[0].from, 0.0);
    assert_eq!(zones[0].to, Some(137.5));

    assert_eq!(zones[1].label, "Z2");
    assert_eq!(zones[1].from, 137.5);
    assert_eq!(zones[1].to, Some(187.5));

    assert_eq!(zones[2].label, "Z3");
    assert_eq!(zones[2].from, 187.5);
    assert_eq!(zones[2].to, Some(225.0));

    assert_eq!(zones[3].label, "Z4");
    assert_eq!(zones[3].from, 225.0);
    assert_eq!(zones[3].to, Some(262.5));

    assert_eq!(zones[4].label, "Z5");
    assert_eq!(zones[4].from, 262.5);
    assert_eq!(zones[4].to, Some(300.0));

    assert_eq!(zones[5].label, "Z6");
    assert_eq!(zones[5].from, 300.0);
    assert_eq!(zones[5].to, Some(375.0));

    assert_eq!(zones[6].label, "Z7");
    assert_eq!(zones[6].from, 375.0);
    assert_eq!(zones[6].to, None);
}

#[test]
fn polarized_zones_at_ftp_250() {
    let zones = polarized_zones(250.0);
    assert_eq!(zones.len(), 3);

    assert_eq!(zones[0].label, "Z1");
    assert_eq!(zones[0].from, 100.0);
    assert_eq!(zones[0].to, Some(200.0));

    assert_eq!(zones[1].label, "Z2");
    assert_eq!(zones[1].from, 200.0);
    assert_eq!(zones[1].to, Some(250.0));

    assert_eq!(zones[2].label, "Z3");
    assert_eq!(zones[2].from, 250.0);
    assert_eq!(zones[2].to, None);
}

#[test]
fn sweetspot_zone_fascat_and_coggan() {
    let fascat = sweetspot_zone(250.0, SweetspotKind::Fascat);
    assert_eq!(fascat.label, "SS");
    assert_eq!(fascat.from, 210.0);
    assert_eq!(fascat.to, Some(242.5));
    assert_eq!(fascat.overlap, true);

    let coggan = sweetspot_zone(250.0, SweetspotKind::Coggan);
    assert_eq!(coggan.label, "SS");
    assert_eq!(coggan.from, 220.0);
    assert_eq!(coggan.to, Some(232.5));
    assert_eq!(coggan.overlap, false);
}
