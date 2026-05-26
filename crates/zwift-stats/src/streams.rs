// SPDX-License-Identifier: AGPL-3.0-only

#[derive(Debug, Clone, PartialEq)]
pub struct LatLng {
    pub lat: f64,
    pub lng: f64,
}

#[derive(Debug)]
pub struct Streams {
    pub distance: Vec<f64>,
    pub altitude: Vec<f64>,
    pub latlng: Vec<LatLng>,
    pub wbal: Vec<Option<f64>>,
}

impl Streams {
    pub fn new() -> Self {
        Streams {
            distance: Vec::new(),
            altitude: Vec::new(),
            latlng: Vec::new(),
            wbal: Vec::new(),
        }
    }
}

impl Default for Streams {
    fn default() -> Self {
        Self::new()
    }
}
