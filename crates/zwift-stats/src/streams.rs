// SPDX-License-Identifier: AGPL-3.0-only

#[derive(Debug)]
pub struct Streams {
    distance_list: Vec<f64>,
}

impl Streams {
    pub fn new() -> Self {
        Streams {
            distance_list: Vec::new(),
        }
    }

    pub fn distance_list(&self) -> &[f64] {
        &self.distance_list
    }
}

impl Default for Streams {
    fn default() -> Self {
        Self::new()
    }
}
