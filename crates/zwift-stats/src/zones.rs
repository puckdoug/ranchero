// SPDX-License-Identifier: AGPL-3.0-only

#[derive(Debug, Clone)]
pub struct Zone {
    pub label: String,
    pub from: f64,
    pub to: Option<f64>,
    pub overlap: bool,
}

#[derive(Debug, Clone)]
pub struct ZoneTime {
    pub label: String,
    pub time: f64,
}

#[derive(Debug, Clone, Copy)]
pub enum SweetspotKind {
    Fascat,
    Coggan,
}

pub fn coggan_zones(ftp: f64) -> Vec<Zone> {
    vec![
        Zone {
            label: "Z1".to_string(),
            from: 0.0,
            to: Some(ftp * 0.55),
            overlap: false,
        },
        Zone {
            label: "Z2".to_string(),
            from: ftp * 0.55,
            to: Some(ftp * 0.75),
            overlap: false,
        },
        Zone {
            label: "Z3".to_string(),
            from: ftp * 0.75,
            to: Some(ftp * 0.90),
            overlap: false,
        },
        Zone {
            label: "Z4".to_string(),
            from: ftp * 0.90,
            to: Some(ftp * 1.05),
            overlap: false,
        },
        Zone {
            label: "Z5".to_string(),
            from: ftp * 1.05,
            to: Some(ftp * 1.20),
            overlap: false,
        },
        Zone {
            label: "Z6".to_string(),
            from: ftp * 1.20,
            to: Some(ftp * 1.50),
            overlap: false,
        },
        Zone {
            label: "Z7".to_string(),
            from: ftp * 1.50,
            to: None,
            overlap: false,
        },
    ]
}

pub fn polarized_zones(ftp: f64) -> Vec<Zone> {
    vec![
        Zone {
            label: "Z1".to_string(),
            from: ftp * 0.40,
            to: Some(ftp * 0.80),
            overlap: false,
        },
        Zone {
            label: "Z2".to_string(),
            from: ftp * 0.80,
            to: Some(ftp),
            overlap: false,
        },
        Zone {
            label: "Z3".to_string(),
            from: ftp,
            to: None,
            overlap: false,
        },
    ]
}

pub fn sweetspot_zone(ftp: f64, kind: SweetspotKind) -> Zone {
    match kind {
        SweetspotKind::Fascat => Zone {
            label: "SS".to_string(),
            from: ftp * 0.84,
            to: Some(ftp * 0.97),
            overlap: true,
        },
        SweetspotKind::Coggan => Zone {
            label: "SS".to_string(),
            from: ftp * 0.88,
            to: Some(ftp * 0.93),
            overlap: false,
        },
    }
}

#[derive(Debug)]
pub struct ZonesAccumulator {
    ftp: Option<f64>,
    zones: Vec<Zone>,
    value: Vec<ZoneTime>,
    time_offset: Option<f64>,
}

impl ZonesAccumulator {
    pub fn new() -> Self {
        ZonesAccumulator {
            ftp: None,
            zones: Vec::new(),
            value: Vec::new(),
            time_offset: None,
        }
    }

    pub fn ftp(&self) -> Option<f64> {
        self.ftp
    }

    pub fn configure(&mut self, ftp: f64, zones: Vec<Zone>) {
        self.ftp = Some(ftp);
        self.zones = zones;
        self.value = self
            .zones
            .iter()
            .map(|z| ZoneTime {
                label: z.label.clone(),
                time: 0.0,
            })
            .collect();
        self.time_offset = None;
    }

    pub fn accumulate(&mut self, time: f64, value: f64) {
        if self.ftp.is_none() {
            return;
        }

        if self.time_offset.is_none() {
            self.time_offset = Some(time);
            return;
        }

        let elapsed = time - self.time_offset.unwrap();
        self.time_offset = Some(time);

        // Iterate zones in reverse order (highest to lowest power)
        for i in (0..self.zones.len()).rev() {
            let zone = &self.zones[i];
            let from_match = value > zone.from;
            let to_match = match zone.to {
                Some(to) => value <= to,
                None => true,
            };

            if from_match && to_match {
                self.value[i].time += elapsed;
                if !zone.overlap {
                    break;
                }
            }
        }
    }

    pub fn value(&self) -> &[ZoneTime] {
        &self.value
    }

    pub fn reset(&mut self) {
        self.ftp = None;
        self.zones.clear();
        self.value.clear();
        self.time_offset = None;
    }

    pub fn clone_reset(&self) -> Self {
        ZonesAccumulator {
            ftp: self.ftp,
            zones: self.zones.clone(),
            value: self
                .zones
                .iter()
                .map(|z| ZoneTime {
                    label: z.label.clone(),
                    time: 0.0,
                })
                .collect(),
            time_offset: None,
        }
    }

    pub fn clone_continue(&self) -> Self {
        ZonesAccumulator {
            ftp: self.ftp,
            zones: self.zones.clone(),
            value: self.value.clone(),
            time_offset: self.time_offset,
        }
    }
}

impl Default for ZonesAccumulator {
    fn default() -> Self {
        Self::new()
    }
}
