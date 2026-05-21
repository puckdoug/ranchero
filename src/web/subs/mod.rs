// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use serde_json::Value;
use tokio::sync::{broadcast, mpsc, Notify};
use tokio::task::AbortHandle;

use crate::daemon::relay::GameEvent;
use zwift_relay::ZWIFT_EPOCH_MS;
use crate::web::format::format_athlete_data_v1;
use crate::web::state::WebState;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Disconnect a client whose outbound queue exceeds this many bytes.
const MAX_BUFFERED_BYTES: usize = 8_388_608; // 8 MB

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Returned to the WebSocket handler after a successful subscribe call.
/// The caller must keep `delegation` alive for the lifetime of the subscription.
pub struct SubscriptionHandle {
    pub sink:           mpsc::UnboundedReceiver<(Value, usize)>,
    pub buffered_bytes: Arc<AtomicUsize>,
    /// Fires when the outbound buffer exceeds `MAX_BUFFERED_BYTES`.
    pub close_notify:   Arc<Notify>,
    pub delegation:     Arc<DelegationHandle>,
}

/// Shared per-(source, event) state.  The Arc strong-count equals the number
/// of active subscribers.  When it reaches zero the Drop impl aborts the
/// fanout task, releasing the upstream broadcast receiver.
pub struct DelegationHandle {
    sinks:        Arc<Mutex<Vec<ClientSink>>>,
    abort_handle: AbortHandle,
}

struct ClientSink {
    tx:             mpsc::UnboundedSender<(Value, usize)>,
    buffered_bytes: Arc<AtomicUsize>,
    close_notify:   Arc<Notify>,
}

impl DelegationHandle {
    /// Adds a new per-client sink and returns the receiving end, the byte
    /// counter, and the close-signal notifier.
    pub fn add_sink(
        &self,
    ) -> (mpsc::UnboundedReceiver<(Value, usize)>, Arc<AtomicUsize>, Arc<Notify>) {
        let (tx, rx)       = mpsc::unbounded_channel();
        let buffered_bytes = Arc::new(AtomicUsize::new(0));
        let close_notify   = Arc::new(Notify::new());
        self.sinks.lock().unwrap().push(ClientSink {
            tx,
            buffered_bytes: Arc::clone(&buffered_bytes),
            close_notify:   Arc::clone(&close_notify),
        });
        (rx, buffered_bytes, close_notify)
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
                let (sink, buffered_bytes, close_notify) = arc.add_sink();
                return Ok(SubscriptionHandle {
                    sink, buffered_bytes, close_notify, delegation: arc,
                });
            }
        }

        // No live delegation; create one (tokio::spawn is safe under a Mutex guard).
        let arc = create_delegation(source, event, state)?;
        let (sink, buffered_bytes, close_notify) = arc.add_sink();
        map.insert(key, Arc::downgrade(&arc));
        Ok(SubscriptionHandle { sink, buffered_bytes, close_notify, delegation: arc })
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
    let sinks: Arc<Mutex<Vec<ClientSink>>> = Arc::new(Mutex::new(Vec::new()));

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
    sinks:  Arc<Mutex<Vec<ClientSink>>>,
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
                        let now   = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs_f64())
                            .unwrap_or(0.0);
                        let ts_ms = a.wt_offset * 1000.0 + ZWIFT_EPOCH_MS as f64;
                        format_athlete_data_v1(a, state.watching_id, state.self_athlete_id, None, now, ts_ms)
                    })
                };
                if let Some(data) = data {
                    // Estimate the serialized byte size of the complete event frame.
                    // The actual frame wraps `data` with ~50 bytes of event envelope.
                    let byte_estimate = serde_json::to_string(&data)
                        .map(|s| s.len())
                        .unwrap_or(256)
                        + 50;

                    let mut sinks = sinks.lock().unwrap();
                    sinks.retain(|sink| {
                        let queued = sink.buffered_bytes.load(Relaxed);
                        if queued + byte_estimate > MAX_BUFFERED_BYTES {
                            // Queue too deep: fire the out-of-band close signal so
                            // the subscription_task closes the session immediately,
                            // regardless of how many frames are still in the sink.
                            tracing::warn!(
                                buffered = queued,
                                "outbound buffer exceeded limit; disconnecting client"
                            );
                            sink.close_notify.notify_one();
                            false // remove this sink
                        } else {
                            sink.buffered_bytes.fetch_add(byte_estimate, Relaxed);
                            sink.tx.send((data.clone(), byte_estimate)).is_ok()
                        }
                    });
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
