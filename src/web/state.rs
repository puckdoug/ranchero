// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use zwift_stats::{AthleteRegistry, EventBehavior, EventSubgroup};

use crate::daemon::relay::GameEvent;
use super::rpc::RpcRegistry;
use super::subs::DelegationMap;

/// Shared state threaded through every actix-web request handler.
pub struct WebState {
    pub registry:        RwLock<AthleteRegistry>,
    pub watching_id:     Option<u32>,
    pub self_athlete_id: Option<u32>,
    pub rpc:             Option<RpcRegistry>,
    pub game_events_tx:  Option<broadcast::Sender<GameEvent>>,
    pub delegations:     DelegationMap,
    /// Event-subgroup cache.  Populated by a background fetch (a later
    /// step); empty for now so `apply_event_state` returns `Idle` on
    /// every lookup miss, matching sauce4zwift's behaviour while its
    /// background fetch is pending.
    pub event_subgroups: Arc<RwLock<HashMap<u32, EventSubgroup>>>,
    pub event_behavior:  EventBehavior,
}

impl WebState {
    pub fn new() -> Self {
        Self {
            registry:        RwLock::new(AthleteRegistry::new()),
            watching_id:     None,
            self_athlete_id: None,
            rpc:             None,
            game_events_tx:  None,
            delegations:     DelegationMap::new(),
            event_subgroups: Arc::new(RwLock::new(HashMap::new())),
            event_behavior:  EventBehavior::default(),
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
            rpc:             None,
            game_events_tx:  None,
            delegations:     DelegationMap::new(),
            event_subgroups: Arc::new(RwLock::new(HashMap::new())),
            event_behavior:  EventBehavior::default(),
        }
    }

    pub fn with_rpc(rpc: RpcRegistry) -> Self {
        Self {
            registry:        RwLock::new(AthleteRegistry::new()),
            watching_id:     None,
            self_athlete_id: None,
            rpc:             Some(rpc),
            game_events_tx:  None,
            delegations:     DelegationMap::new(),
            event_subgroups: Arc::new(RwLock::new(HashMap::new())),
            event_behavior:  EventBehavior::default(),
        }
    }

    /// Builder method: attach a game-event broadcast sender so the stats
    /// subscription source can create receivers.
    pub fn and_game_events(mut self, tx: broadcast::Sender<GameEvent>) -> Self {
        self.game_events_tx = Some(tx);
        self
    }
}
