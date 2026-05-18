// SPDX-License-Identifier: AGPL-3.0-only

/// Shared state threaded through every actix-web request handler.
///
/// Empty at 17.1-I; populated with `AthleteRegistry`, `Stores`, and
/// the stats broadcast receiver in later steps.
pub struct WebState {}

impl WebState {
    pub fn new() -> Self {
        Self {}
    }
}
