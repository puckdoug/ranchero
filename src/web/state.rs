// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::RwLock;
use zwift_stats::AthleteRegistry;

use super::rpc::RpcRegistry;

/// Shared state threaded through every actix-web request handler.
pub struct WebState {
    pub registry:        RwLock<AthleteRegistry>,
    pub watching_id:     Option<u32>,
    pub self_athlete_id: Option<u32>,
    pub rpc:             Option<RpcRegistry>,
}

impl WebState {
    pub fn new() -> Self {
        Self {
            registry:        RwLock::new(AthleteRegistry::new()),
            watching_id:     None,
            self_athlete_id: None,
            rpc:             None,
        }
    }

    pub fn with_registry(
        registry:        AthleteRegistry,
        watching_id:     Option<u32>,
        self_athlete_id: Option<u32>,
    ) -> Self {
        Self {
            registry: RwLock::new(registry),
            watching_id,
            self_athlete_id,
            rpc: None,
        }
    }

    pub fn with_rpc(rpc: RpcRegistry) -> Self {
        Self {
            registry:        RwLock::new(AthleteRegistry::new()),
            watching_id:     None,
            self_athlete_id: None,
            rpc:             Some(rpc),
        }
    }
}
