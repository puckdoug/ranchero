// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use serde_json::Value;
use tokio::sync::broadcast;
use zwift_stats::{AthleteRegistry, AutoLapConfig, EventBehavior, EventSubgroup, SegmentLookup};
use zwift_store::{AthleteRecord, AthletesDb, SegmentsDb};
use crate::daemon::Stores;
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

    /// Insert a freshly-fetched profile into the live layer and write through
    /// to `athletes.sqlite` so the row survives a daemon restart. Live data is
    /// authoritative; the SQLite fallback is consulted only on cache miss.
    pub fn insert_live(&self, id: u32, profile: CachedProfile) {
        self.live.lock().unwrap().insert(id, profile.clone());

        let mut data = serde_json::Map::new();
        if let Some(s) = &profile.first_name { data.insert("firstName".to_string(), serde_json::Value::String(s.clone())); }
        if let Some(s) = &profile.last_name  { data.insert("lastName".to_string(),  serde_json::Value::String(s.clone())); }
        if let Some(v) = profile.ftp         { data.insert("ftp".to_string(),       serde_json::Value::from(v)); }
        if let Some(v) = profile.weight_g    { data.insert("weight".to_string(),    serde_json::Value::from(v)); }

        let last_seen = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let rec = AthleteRecord {
            id:        id as i64,
            data:      serde_json::Value::Object(data),
            last_seen,
        };
        if let Err(e) = self.db.upsert(&rec) {
            tracing::warn!(
                target: "ranchero::web::profile_cache",
                error = %e,
                athlete_id = id,
                "profile_cache.upsert.failed",
            );
        }
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
    /// When `Some`, each call to `route_player_state` checks whether the
    /// watched athlete's metric has advanced past the threshold and fires a
    /// lap if so (sauce `stats.mjs:3092`).  `None` disables auto-lap.
    pub auto_lap_config: Option<AutoLapConfig>,
    /// Count of live WebSocket client connections. Incremented when a
    /// client task starts and decremented when it ends; reported by
    /// `ranchero status` as the web server's connection count.
    pub active_connections: Arc<AtomicUsize>,
    /// Current game-state snapshot (sauce `stats.mjs:1250`).  `None` until
    /// the first `game-state` event is received from the relay.  Used by the
    /// formatter to populate the `gameState` field for the self athlete only.
    pub game_state: Mutex<Option<Value>>,
    /// Segment-geometry source consulted by `route_player_state` to drive
    /// `active_segment_check`. `None` disables active-segment detection,
    /// which is the production default until Step 18's world-meta tables
    /// land; tests attach an in-memory lookup directly.
    pub segment_env: Option<Arc<dyn SegmentLookup + Send + Sync>>,
    /// In-memory key/value store backing the `getSetting` / `setSetting`
    /// read-only RPC pair.  Widgets persist UI-only preferences here; values
    /// do not propagate to Zwift (QA1).
    pub settings: Mutex<HashMap<String, Value>>,
    /// Ring of recent chat messages exposed by the `getChatHistory` RPC.
    /// Producers (the `chat` subscription source) push entries onto the back;
    /// readers receive a clone of the current buffer.
    pub chat_history: Mutex<Vec<Value>>,
    /// Read-through profile cache backed by `athletes.sqlite`. Populated by
    /// `with_stores`; `None` for the legacy test constructors that don't
    /// open the daemon's on-disk SQLite databases.
    profile_cache: Option<Arc<ProfileCache>>,
    /// Handle to `segments.sqlite` for the segment-leaderboard fetchers and
    /// the periodic evictor. Populated by `with_stores`.
    segments_db: Option<Arc<SegmentsDb>>,
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
            auto_lap_config: None,
            active_connections: Arc::new(AtomicUsize::new(0)),
            game_state:      Mutex::new(None),
            segment_env:     None,
            settings:        Mutex::new(HashMap::new()),
            chat_history:    Mutex::new(Vec::new()),
            profile_cache:   None,
            segments_db:     None,
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
            auto_lap_config: None,
            active_connections: Arc::new(AtomicUsize::new(0)),
            game_state:      Mutex::new(None),
            segment_env:     None,
            settings:        Mutex::new(HashMap::new()),
            chat_history:    Mutex::new(Vec::new()),
            profile_cache:   None,
            segments_db:     None,
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
            auto_lap_config: None,
            active_connections: Arc::new(AtomicUsize::new(0)),
            game_state:      Mutex::new(None),
            segment_env:     None,
            settings:        Mutex::new(HashMap::new()),
            chat_history:    Mutex::new(Vec::new()),
            profile_cache:   None,
            segments_db:     None,
        }
    }

    /// Builder method: attach a game-event broadcast sender so the stats
    /// subscription source can create receivers.
    pub fn and_game_events(mut self, tx: broadcast::Sender<GameEvent>) -> Self {
        self.game_events_tx = Some(tx);
        self
    }

    /// Builder method: attach a segment-geometry source so
    /// `route_player_state` can drive `active_segment_check`. Returning
    /// `Self` lets test setups chain `WebState::new().with_segment_env(env)`.
    pub fn with_segment_env(
        mut self,
        env: Arc<dyn SegmentLookup + Send + Sync>,
    ) -> Self {
        self.segment_env = Some(env);
        self
    }

    /// Builder method: set the watched athlete id. Used by tests that need
    /// `WebState::new()` plus a non-default watcher; production paths set
    /// this in `with_registry`.
    pub fn with_watching(mut self, id: u32) -> Self {
        self.watching_id = Some(id);
        self
    }

    /// Builder method: attach the daemon's on-disk SQLite stores. Wraps the
    /// athletes DB in a `ProfileCache` (write-through, two-layer) and keeps a
    /// handle to the segments DB so the leaderboard fetchers and the
    /// periodic evictor reach the same file the daemon opened at startup.
    /// Step 19 wires this from `run_daemon` so `Stores::open` is no longer
    /// inert (20.28 item 3).
    pub fn with_stores(mut self, stores: Stores) -> Self {
        let Stores { kv: _, athletes, segments } = stores;
        self.profile_cache = Some(Arc::new(ProfileCache::new(athletes)));
        self.segments_db   = Some(Arc::new(segments));
        self
    }

    /// Read-through profile cache backed by `athletes.sqlite`. Panics if
    /// the cache wasn't attached via `with_stores`; production wiring in
    /// `run_daemon` always attaches it.
    pub fn profile_cache(&self) -> &ProfileCache {
        self.profile_cache
            .as_deref()
            .expect("profile_cache: WebState built without with_stores()")
    }

    /// Handle to `segments.sqlite`. `None` for the legacy test
    /// constructors; production wiring attaches it via `with_stores`.
    pub fn segments_db(&self) -> Option<&Arc<SegmentsDb>> {
        self.segments_db.as_ref()
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
