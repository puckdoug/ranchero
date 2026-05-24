// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use zwift_stats::{AthleteRegistry, EventBehavior, EventSubgroup};
use zwift_store::AthletesDb;
use crate::web::format::CachedProfile;

use crate::daemon::relay::GameEvent;
use super::rpc::RpcRegistry;
use super::subs::DelegationMap;

/// Two-layer athlete profile cache: in-memory live data takes precedence over
/// the SQLite `athletes.sqlite` fallback.
pub struct ProfileCache {
    live: Mutex<HashMap<u32, CachedProfile>>,
    db:   AthletesDb,
}

impl ProfileCache {
    pub fn new(db: AthletesDb) -> Self {
        Self { live: Mutex::new(HashMap::new()), db }
    }

    pub fn insert_live(&self, id: u32, profile: CachedProfile) {
        self.live.lock().unwrap().insert(id, profile);
    }

    pub fn get(&self, id: u32) -> Option<CachedProfile> {
        if let Some(p) = self.live.lock().unwrap().get(&id).cloned() {
            return Some(p);
        }
        self.db.get(id as i64).ok().flatten().map(|rec| CachedProfile {
            first_name: rec.data["firstName"].as_str().map(|s| s.to_owned()),
            last_name:  rec.data["lastName"].as_str().map(|s| s.to_owned()),
            ftp:        rec.data["ftp"].as_u64().map(|v| v as u32),
            weight_g:   rec.data["weight"].as_u64().map(|v| v as u32),
        })
    }
}

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
    /// Count of live WebSocket client connections. Incremented when a
    /// client task starts and decremented when it ends; reported by
    /// `ranchero status` as the web server's connection count.
    pub active_connections: Arc<AtomicUsize>,
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
            active_connections: Arc::new(AtomicUsize::new(0)),
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
            active_connections: Arc::new(AtomicUsize::new(0)),
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
            active_connections: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Builder method: attach a game-event broadcast sender so the stats
    /// subscription source can create receivers.
    pub fn and_game_events(mut self, tx: broadcast::Sender<GameEvent>) -> Self {
        self.game_events_tx = Some(tx);
        self
    }
}

/// Periodically run the athlete-registry garbage collector and emit one
/// `tracing::debug!` event per tick with the drop counts from `GcReport`.
///
/// The first tick is skipped so the GC does not run at startup before
/// any athletes have been seen.  Every subsequent tick fires after
/// `interval`, matching the cadence sauce4zwift uses for `_gcAthleteData`.
pub async fn gc_tick_loop(state: Arc<WebState>, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    ticker.tick().await; // skip the immediate first tick
    loop {
        ticker.tick().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let report = state.registry.write().unwrap().gc(now);
        tracing::debug!(
            athletes_dropped = report.athletes_dropped,
            groups_dropped   = report.groups_dropped,
            "gc_tick",
        );
    }
}
