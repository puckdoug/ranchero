// SPDX-License-Identifier: AGPL-3.0-only

//! Read-only RPC surface.
//!
//! `RpcRegistry::new()` registers only the no-state handlers (today, just
//! `getVersion`).  `RpcRegistry::with_state(Arc<WebState>)` additionally
//! registers the full read-only getter set that widgets call over
//! WebSocket and HTTP (`getAthlete`, `getAthleteData`, `getNearbyData`,
//! `getEventSubgroup`, `getChatHistory`, `getGameState`, etc.).
//!
//! Write RPCs that would mutate Zwift-side state (`setFollowing`,
//! `giveRideon`, `updateAthlete`, …) are NOT registered: ranchero is
//! read-only by decision QA1 in
//! `docs/planning/STEP-20-additional-considerations.md`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::{json, Value};
use zwift_stats::zones::get_power_zones;

use super::format::{
    format_athlete_data_v1,
    format_athlete_streams,
    format_data_slice,
    format_segment_slice,
    format_state,
};
use super::state::WebState;

type BoxFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'static>>;
type RpcHandler = Box<dyn Fn(Vec<Value>) -> BoxFuture + Send + Sync + 'static>;

pub struct RpcRegistry {
    handlers: HashMap<String, RpcHandler>,
}

impl RpcRegistry {
    pub fn new() -> Self {
        let mut r = Self { handlers: HashMap::new() };
        r.register("getVersion", |_args: Vec<Value>| async {
            Ok::<Value, String>(Value::String(env!("CARGO_PKG_VERSION").to_string()))
        });
        r
    }

    /// Build a registry whose handlers resolve against the supplied
    /// `WebState`.  Each handler captures a clone of the `Arc` so the
    /// registry can outlive any particular handler invocation without
    /// borrowing back to the state.
    pub fn with_state(state: Arc<WebState>) -> Self {
        let mut r = Self::new();
        register_read_only_handlers(&mut r, state);
        r
    }

    pub fn register<F, Fut>(&mut self, name: &str, f: F)
    where
        F: Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, String>> + Send + 'static,
    {
        self.handlers.insert(
            name.to_string(),
            Box::new(move |args| Box::pin(f(args))),
        );
    }

    pub fn names(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
    }

    pub async fn dispatch(&self, name: &str, args: Vec<Value>) -> Option<Result<Value, String>> {
        let fut = {
            let f = self.handlers.get(name)?;
            f(args)
        };
        Some(fut.await)
    }
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

fn arg_u32(args: &[Value], idx: usize) -> Result<u32, String> {
    let v = args.get(idx).ok_or_else(|| format!("missing arg {idx}"))?;
    v.as_u64()
        .map(|n| n as u32)
        .ok_or_else(|| format!("arg {idx} is not an integer: {v}"))
}

fn arg_f64(args: &[Value], idx: usize) -> Result<f64, String> {
    let v = args.get(idx).ok_or_else(|| format!("missing arg {idx}"))?;
    v.as_f64()
        .ok_or_else(|| format!("arg {idx} is not a number: {v}"))
}

fn arg_str(args: &[Value], idx: usize) -> Result<String, String> {
    let v = args.get(idx).ok_or_else(|| format!("missing arg {idx}"))?;
    v.as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("arg {idx} is not a string: {v}"))
}

fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// Read-only handler registration
// ---------------------------------------------------------------------------

fn register_read_only_handlers(r: &mut RpcRegistry, state: Arc<WebState>) {
    register_athlete_handlers(r, &state);
    register_nearby_groups_handlers(r, &state);
    register_event_handlers(r, &state);
    register_segment_handlers(r, &state);
    register_chat_game_state_handlers(r, &state);
    register_geometry_handlers(r, &state);
    register_settings_handlers(r, &state);
    register_info_handlers(r, &state);
}

// ── Athlete getters ─────────────────────────────────────────────────────────

fn register_athlete_handlers(r: &mut RpcRegistry, state: &Arc<WebState>) {
    let s = state.clone();
    r.register("getAthlete", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let id = arg_u32(&args, 0)?;
            let reg = s.registry.read().unwrap();
            Ok(athlete_profile_json(&s, &reg, id))
        }
    });

    let s = state.clone();
    r.register("getAthletes", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let ids = args.get(0)
                .and_then(|v| v.as_array())
                .ok_or_else(|| "getAthletes: arg 0 must be an array of ids".to_string())?
                .clone();
            let reg = s.registry.read().unwrap();
            let out: Vec<Value> = ids.iter()
                .map(|v| match v.as_u64() {
                    Some(n) => athlete_profile_json(&s, &reg, n as u32),
                    None    => Value::Null,
                })
                .collect();
            Ok(Value::Array(out))
        }
    });

    let s = state.clone();
    r.register("getAthleteData", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let id = arg_u32(&args, 0)?;
            let reg = s.registry.read().unwrap();
            let ad = match reg.get(id) {
                Some(ad) => ad,
                None     => return Ok(Value::Null),
            };
            let game_state = s.game_state.lock().unwrap().clone();
            Ok(format_athlete_data_v1(
                ad,
                s.watching_id,
                s.self_athlete_id,
                None,
                now_seconds(),
                0.0,
                game_state,
            ))
        }
    });

    let s = state.clone();
    r.register("getAthletesData", move |_args: Vec<Value>| {
        let s = s.clone();
        async move {
            let reg = s.registry.read().unwrap();
            let game_state = s.game_state.lock().unwrap().clone();
            let now = now_seconds();
            let out: Vec<Value> = reg.iter()
                .map(|(_, ad)| format_athlete_data_v1(
                    ad,
                    s.watching_id,
                    s.self_athlete_id,
                    None,
                    now,
                    0.0,
                    game_state.clone(),
                ))
                .collect();
            Ok(Value::Array(out))
        }
    });

    let s = state.clone();
    r.register("getAthleteLaps", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let id = arg_u32(&args, 0)?;
            let reg = s.registry.read().unwrap();
            let out = match reg.get(id) {
                Some(ad) => ad.lap_slices.iter()
                    .map(|slice| format_data_slice(slice, ad))
                    .collect::<Vec<Value>>(),
                None => Vec::new(),
            };
            Ok(Value::Array(out))
        }
    });

    let s = state.clone();
    r.register("getAthleteSegments", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let id = arg_u32(&args, 0)?;
            let reg = s.registry.read().unwrap();
            let out = match reg.get(id) {
                Some(ad) => ad.segment_slices.iter()
                    .map(|slice| format_segment_slice(slice, ad))
                    .collect::<Vec<Value>>(),
                None => Vec::new(),
            };
            Ok(Value::Array(out))
        }
    });

    let s = state.clone();
    r.register("getAthleteStreams", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let id = arg_u32(&args, 0)?;
            let reg = s.registry.read().unwrap();
            let out = match reg.get(id) {
                Some(ad) => format_athlete_streams(ad),
                None     => json!({
                    "time": [], "power": [], "speed": [], "hr": [],
                    "cadence": [], "draft": [], "active": [],
                    "distance": [], "altitude": [], "latlng": [], "wbal": [],
                }),
            };
            Ok(out)
        }
    });

    let s = state.clone();
    r.register("getPlayerState", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let id = arg_u32(&args, 0)?;
            let reg = s.registry.read().unwrap();
            let out = reg.get(id)
                .and_then(|ad| ad.most_recent_state.as_ref().map(format_state))
                .unwrap_or(Value::Null);
            Ok(out)
        }
    });

    r.register("getPowerZones", move |args: Vec<Value>| async move {
        let ftp = arg_f64(&args, 0)?;
        let zones = get_power_zones(ftp);
        let out: Vec<Value> = zones.into_iter()
            .map(|z| json!({
                "zone":    z.label,
                "label":   z.label,
                "from":    z.from,
                "to":      z.to,
                "overlap": z.overlap,
            }))
            .collect();
        Ok(Value::Array(out))
    });

    // No power-profile source yet (would be backed by athlete peaks from
    // sauce `getPowerProfile`); return null until a producer lands.
    r.register("getPowerProfile", |_args: Vec<Value>| async move {
        Ok(Value::Null)
    });
}

fn athlete_profile_json(
    _state: &WebState,
    reg:    &zwift_stats::AthleteRegistry,
    id:     u32,
) -> Value {
    // Profile cache is not yet wired into WebState (Step 2 builds it).
    // For now, return a minimal identity record sourced from the registry
    // so consumers can confirm an athlete is known; widgets that need
    // FTP/name will get them when the profile cache is attached.
    match reg.get(id) {
        Some(ad) => json!({
            "id":       id,
            "courseId": ad.course_id,
            "sport":    ad.sport,
        }),
        None => Value::Null,
    }
}

// ── Nearby / Groups ─────────────────────────────────────────────────────────

fn register_nearby_groups_handlers(r: &mut RpcRegistry, _state: &Arc<WebState>) {
    // Producers for the sorted nearby/groups arrays land in Step 12.
    // Until then, the RPCs return an empty array so widgets see "no
    // riders nearby" instead of an `unknown rpc handler` error.
    r.register("getNearbyData", |_args: Vec<Value>| async move {
        Ok(Value::Array(Vec::new()))
    });
    r.register("getGroupsData", |_args: Vec<Value>| async move {
        Ok(Value::Array(Vec::new()))
    });
}

// ── Event getters ───────────────────────────────────────────────────────────

fn register_event_handlers(r: &mut RpcRegistry, state: &Arc<WebState>) {
    let s = state.clone();
    r.register("getCachedEvents", move |_args: Vec<Value>| {
        let s = s.clone();
        async move {
            let m = s.event_subgroups.read().unwrap();
            let out: Vec<Value> = m.values().map(event_subgroup_json).collect();
            Ok(Value::Array(out))
        }
    });

    let s = state.clone();
    r.register("getCachedEvent", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let id = arg_u32(&args, 0)?;
            let m = s.event_subgroups.read().unwrap();
            Ok(m.get(&id).map(event_subgroup_json).unwrap_or(Value::Null))
        }
    });

    let s = state.clone();
    r.register("getEventSubgroup", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let id = arg_u32(&args, 0)?;
            let m = s.event_subgroups.read().unwrap();
            Ok(m.get(&id).map(event_subgroup_json).unwrap_or(Value::Null))
        }
    });

    // Live REST fetch hook for `getEvent` arrives with the event-chain
    // pipeline; until a fetcher is plumbed through, callers see null on
    // cache miss.
    r.register("getEvent", |_args: Vec<Value>| async move {
        Ok(Value::Null)
    });

    r.register("getEventSubgroupEntrants", |_args: Vec<Value>| async move {
        Ok(Value::Array(Vec::new()))
    });
    r.register("getEventSubgroupResults", |_args: Vec<Value>| async move {
        Ok(Value::Array(Vec::new()))
    });
}

fn event_subgroup_json(sg: &zwift_stats::athlete::EventSubgroup) -> Value {
    json!({
        "id":              sg.id,
        "courseId":        sg.course_id,
        "allTags":         sg.all_tags,
        "endTs":           sg.end_ts,
        "endDistance":     sg.end_distance,
        "invitedLeaders":  sg.invited_leaders,
        "invitedSweepers": sg.invited_sweepers,
    })
}

// ── Segment leaderboards ────────────────────────────────────────────────────

fn register_segment_handlers(r: &mut RpcRegistry, _state: &Arc<WebState>) {
    // The leaderboard cache (`segments.sqlite`) is owned by the daemon's
    // store bundle, not WebState; until a query helper threads through,
    // this returns an empty array so callers see "no entries" rather
    // than a missing handler.
    r.register("getSegmentResults", |_args: Vec<Value>| async move {
        Ok(Value::Array(Vec::new()))
    });
}

// ── Chat / game state ───────────────────────────────────────────────────────

fn register_chat_game_state_handlers(r: &mut RpcRegistry, state: &Arc<WebState>) {
    let s = state.clone();
    r.register("getChatHistory", move |_args: Vec<Value>| {
        let s = s.clone();
        async move {
            let h = s.chat_history.lock().unwrap();
            Ok(Value::Array(h.clone()))
        }
    });

    let s = state.clone();
    r.register("getGameState", move |_args: Vec<Value>| {
        let s = s.clone();
        async move {
            Ok(s.game_state.lock().unwrap().clone().unwrap_or(Value::Null))
        }
    });
}

// ── Geometry (route / world meta — backing data lands in Steps 17/18) ──────

fn register_geometry_handlers(r: &mut RpcRegistry, _state: &Arc<WebState>) {
    r.register("getWorldMetas", |_args: Vec<Value>| async move {
        Ok(Value::Array(Vec::new()))
    });
    r.register("getCourseId",  |_args: Vec<Value>| async move { Ok(Value::Null) });
    r.register("getRoad",      |_args: Vec<Value>| async move { Ok(Value::Null) });
    r.register("getRoute",     |_args: Vec<Value>| async move { Ok(Value::Null) });
    r.register("getSegment",   |_args: Vec<Value>| async move { Ok(Value::Null) });
}

// ── Settings (in-memory; UI-only) ───────────────────────────────────────────

fn register_settings_handlers(r: &mut RpcRegistry, state: &Arc<WebState>) {
    let s = state.clone();
    r.register("getSetting", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let key = arg_str(&args, 0)?;
            let m = s.settings.lock().unwrap();
            Ok(m.get(&key).cloned().unwrap_or(Value::Null))
        }
    });

    let s = state.clone();
    r.register("setSetting", move |args: Vec<Value>| {
        let s = s.clone();
        async move {
            let key = arg_str(&args, 0)?;
            let val = args.get(1).cloned().unwrap_or(Value::Null);
            let mut m = s.settings.lock().unwrap();
            m.insert(key, val.clone());
            Ok(val)
        }
    });
}

// ── Info / connection introspection ─────────────────────────────────────────

fn register_info_handlers(r: &mut RpcRegistry, state: &Arc<WebState>) {
    let s = state.clone();
    r.register("getDebugInfo", move |_args: Vec<Value>| {
        let s = s.clone();
        async move {
            let reg = s.registry.read().unwrap();
            Ok(json!({
                "version":     env!("CARGO_PKG_VERSION"),
                "watchingId":  s.watching_id,
                "selfId":      s.self_athlete_id,
                "athletes":    reg.len(),
                "groups":      reg.groups_len(),
            }))
        }
    });

    r.register("getWebServerURL", |_args: Vec<Value>| async move {
        // The bound URL is owned by the daemon (`WebServerHandle`); the
        // RPC registry doesn't see it.  Returning null is the read-side
        // contract: callers that need a URL look at the daemon status.
        Ok(Value::Null)
    });

    let s = state.clone();
    r.register("getZwiftLoginInfo", move |_args: Vec<Value>| {
        let s = s.clone();
        async move {
            Ok(json!({ "selfId": s.self_athlete_id }))
        }
    });

    let s = state.clone();
    r.register("getZwiftConnectionInfo", move |_args: Vec<Value>| {
        let s = s.clone();
        async move {
            Ok(json!({
                "watchingId":       s.watching_id,
                "selfId":           s.self_athlete_id,
                "activeWebClients": s.active_connections
                                       .load(std::sync::atomic::Ordering::Relaxed),
            }))
        }
    });
}
