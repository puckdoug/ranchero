// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use serde_json::Value;
use tokio::sync::{broadcast, mpsc};
use tokio::task::AbortHandle;

use crate::daemon::relay::GameEvent;
use crate::web::http::format_athlete;
use crate::web::state::WebState;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Returned to the WebSocket handler after a successful subscribe call.
/// The caller must keep `delegation` alive for the lifetime of the subscription.
pub struct SubscriptionHandle {
    pub sink:       mpsc::UnboundedReceiver<Value>,
    pub delegation: Arc<DelegationHandle>,
}

/// Shared per-(source, event) state.  The Arc strong-count equals the number
/// of active subscribers.  When it reaches zero the Drop impl aborts the
/// fanout task, releasing the upstream broadcast receiver.
pub struct DelegationHandle {
    sinks:        Arc<Mutex<Vec<mpsc::UnboundedSender<Value>>>>,
    abort_handle: AbortHandle,
}

impl DelegationHandle {
    /// Adds a new per-client sink and returns the receiving end.
    pub fn add_sink(&self) -> mpsc::UnboundedReceiver<Value> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.sinks.lock().unwrap().push(tx);
        rx
    }
}

impl Drop for DelegationHandle {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}

// ---------------------------------------------------------------------------
// Delegation map
// ---------------------------------------------------------------------------

/// Process-wide deduplication map.  One upstream listener per (source, event)
/// pair regardless of how many clients are subscribed.
pub struct DelegationMap {
    inner: Mutex<HashMap<String, Weak<DelegationHandle>>>,
}

impl DelegationMap {
    pub fn new() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }

    /// Returns a `SubscriptionHandle` whose sink receives formatted event values.
    /// Reuses a live delegation when one exists for the same (source, event) pair;
    /// creates a fresh one otherwise.
    pub fn subscribe(
        &self,
        source: &str,
        event:  &str,
        state:  &Arc<WebState>,
    ) -> Result<SubscriptionHandle, String> {
        let key = format!("{source}/{event}");
        let mut map = self.inner.lock().unwrap();

        // Reuse a live delegation.
        if let Some(weak) = map.get(&key) {
            if let Some(arc) = weak.upgrade() {
                let sink = arc.add_sink();
                return Ok(SubscriptionHandle { sink, delegation: arc });
            }
        }

        // No live delegation; create one (tokio::spawn is safe under a Mutex guard).
        let arc = create_delegation(source, event, state)?;
        let sink = arc.add_sink();
        map.insert(key, Arc::downgrade(&arc));
        Ok(SubscriptionHandle { sink, delegation: arc })
    }
}

// ---------------------------------------------------------------------------
// Delegation construction
// ---------------------------------------------------------------------------

fn create_delegation(
    source: &str,
    event:  &str,
    state:  &Arc<WebState>,
) -> Result<Arc<DelegationHandle>, String> {
    let sinks: Arc<Mutex<Vec<mpsc::UnboundedSender<Value>>>> =
        Arc::new(Mutex::new(Vec::new()));

    let task = match source {
        "stats" => {
            let game_tx = state.game_events_tx.as_ref()
                .ok_or_else(|| "stats source not available".to_string())?;
            let rx        = game_tx.subscribe();
            let sinks_ref = Arc::clone(&sinks);
            let state_ref = Arc::clone(state);
            let event_str = event.to_string();
            tokio::spawn(stats_fanout_task(rx, sinks_ref, state_ref, event_str))
        }
        "gameConnection" => {
            // No events emitted yet; task parks until aborted.
            tokio::spawn(std::future::pending::<()>())
        }
        _ => return Err(format!("unknown source: {source}")),
    };

    Ok(Arc::new(DelegationHandle { sinks, abort_handle: task.abort_handle() }))
}

// ---------------------------------------------------------------------------
// Stats fanout task
// ---------------------------------------------------------------------------

async fn stats_fanout_task(
    mut rx: broadcast::Receiver<GameEvent>,
    sinks:  Arc<Mutex<Vec<mpsc::UnboundedSender<Value>>>>,
    state:  Arc<WebState>,
    event:  String,
) {
    loop {
        match rx.recv().await {
            Ok(GameEvent::PlayerState { athlete_id, .. }) => {
                if !event_matches_athlete(&event, athlete_id, &state) {
                    continue;
                }
                let athlete_id_u32 = match u32::try_from(athlete_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let data = {
                    let registry = state.registry.read().unwrap();
                    registry.get(athlete_id_u32).map(|a| {
                        format_athlete(a, state.watching_id, state.self_athlete_id)
                    })
                };
                if let Some(data) = data {
                    let mut sinks = sinks.lock().unwrap();
                    sinks.retain(|tx| tx.send(data.clone()).is_ok());
                }
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Closed)    => break,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

/// Returns `true` when the `event` path indicates this `athlete_id` should
/// be forwarded to subscribers.
fn event_matches_athlete(event: &str, athlete_id: i64, state: &WebState) -> bool {
    if event == "athlete/watching" {
        return state.watching_id.map(|id| id as i64) == Some(athlete_id);
    }
    if let Some(id_str) = event.strip_prefix("athlete/") {
        if let Ok(id) = id_str.parse::<i64>() {
            return athlete_id == id;
        }
    }
    true
}
