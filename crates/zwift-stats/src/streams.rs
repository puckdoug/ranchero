// SPDX-License-Identifier: AGPL-3.0-only

<<<<<<< Updated upstream
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
=======
#[derive(Debug)]
pub struct Streams {
    distance_list: Vec<f64>,
>>>>>>> Stashed changes
}

impl Streams {
    pub fn new() -> Self {
        Streams {
<<<<<<< Updated upstream
            distance: Vec::new(),
            altitude: Vec::new(),
            latlng: Vec::new(),
            wbal: Vec::new(),
        }
    }
=======
            distance_list: Vec::new(),
        }
    }

    pub fn distance_list(&self) -> &[f64] {
        &self.distance_list
    }
>>>>>>> Stashed changes
}

impl Default for Streams {
    fn default() -> Self {
        Self::new()
    }
}
