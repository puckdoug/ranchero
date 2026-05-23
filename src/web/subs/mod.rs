// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use serde_json::Value;
use tokio::sync::{broadcast, mpsc, Notify};
use tokio::task::AbortHandle;

use crate::daemon::relay::GameEvent;
use zwift_relay::ZWIFT_EPOCH_MS;
use crate::web::format::{format_athlete_data_v1, format_athlete_v2};
use crate::web::state::WebState;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Disconnect a client whose outbound queue exceeds this many bytes.
const MAX_BUFFERED_BYTES: usize = 8_388_608; // 8 MB

// ---------------------------------------------------------------------------
// v2 query types
// ---------------------------------------------------------------------------

/// The query parameters from a v2 WebSocket subscription or HTTP request.
#[derive(Clone, Default)]
pub struct AthleteV2Query {
    pub resources: Vec<String>,
    pub stats:     bool,
}

impl AthleteV2Query {
    /// A stable string suitable for use as a `HashMap` key.
    fn canonical_key(&self) -> String {
        let mut res = self.resources.clone();
        res.sort();
        res.dedup();
        format!("{}/{}", self.stats, res.join(","))
    }
}

/// A single subscriber with its associated query.
#[derive(Clone)]
pub struct QueryListener {
    pub query: AthleteV2Query,
}

/// One batch within a query strategy: a merged query covering a set of
/// listeners whose individual queries are all subsets of it.
pub struct QueryBatch {
    pub query:     AthleteV2Query,
    pub listeners: Vec<QueryListener>,
}

/// A group of listeners that all require the same filtering pass after a
/// shared formatting call.  `mask` names the resources present in the batch
/// but absent from every listener in this group (to be stripped).
/// `stats_mask` names the slice resources (laps/segments/events) that must
/// have their per-slice `stats` field set to null because the batch computed
/// stats but the listeners in this group did not request them.
pub struct FilterGroup {
    pub listeners:  Vec<QueryListener>,
    pub mask:       Vec<String>,
    pub stats_mask: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Query-reduction functions
// ---------------------------------------------------------------------------

/// Returns the estimated serialisation cost of a single `AthleteV2Query`.
/// Mirrors `computeQueryCost` in `stats.mjs:795`.
pub fn compute_query_cost(query: &AthleteV2Query) -> f64 {
    let stats_f = if query.stats { 5.0_f64 } else { 1.0_f64 };
    query.resources.iter().map(|r| match r.as_str() {
        "state"            => 0.5,
        "timeInPowerZones" => 0.4,
        "athlete"          => 1.5,
        "stats"            => 1.0 * stats_f,
        "lap"              => 1.0 * stats_f,
        "lastLap"          => 1.0 * stats_f,
        "laps"             => 2.0 * stats_f,
        "segments"         => 4.0 * stats_f,
        "events"           => 1.5 * stats_f,
        _                  => 1.0,
    }).sum()
}

/// Returns the candidate strategies for batching a set of listeners.
/// Each strategy is a `Vec<QueryBatch>`; the caller picks the cheapest.
/// Mirrors `createQueryStrategies` in `stats.mjs:750`.
///
/// - If all listeners have `stats: false`, one combined strategy is returned.
/// - If both stats and non-stats listeners are present, two strategies are
///   returned: a split strategy (two batches at different fidelity) and a
///   combined strategy (one stats=true batch covering everyone).
pub fn create_query_strategies(listeners: &[QueryListener]) -> Vec<Vec<QueryBatch>> {
    use std::collections::HashSet;

    let non_stats_resources: HashSet<String> = listeners.iter()
        .filter(|l| !l.query.stats)
        .flat_map(|l| l.query.resources.iter().cloned())
        .collect();
    let stats_resources: HashSet<String> = listeners.iter()
        .filter(|l| l.query.stats)
        .flat_map(|l| l.query.resources.iter().cloned())
        .collect();

    let mut strategies: Vec<Vec<QueryBatch>> = Vec::new();

    if !non_stats_resources.is_empty() && !stats_resources.is_empty() {
        let mut ns_res: Vec<String> = non_stats_resources.iter().cloned().collect();
        ns_res.sort();
        let mut s_res: Vec<String> = stats_resources.iter().cloned().collect();
        s_res.sort();
        strategies.push(vec![
            QueryBatch {
                query: AthleteV2Query { stats: false, resources: ns_res },
                listeners: listeners.iter().filter(|l| !l.query.stats).cloned().collect(),
            },
            QueryBatch {
                query: AthleteV2Query { stats: true, resources: s_res },
                listeners: listeners.iter().filter(|l| l.query.stats).cloned().collect(),
            },
        ]);
    }

    let has_stats = !stats_resources.is_empty();
    let mut all_res: Vec<String> = non_stats_resources.into_iter()
        .chain(stats_resources)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    all_res.sort();
    strategies.push(vec![QueryBatch {
        query: AthleteV2Query { stats: has_stats, resources: all_res },
        listeners: listeners.to_vec(),
    }]);

    strategies
}

/// Groups the listeners within a batch by the filter operation each needs
/// applied to the batch's formatted output.  Listeners with the same
/// (mask, stats_mask) signature share one group.
/// Mirrors `createFilterGroups` in `stats.mjs:809`.
pub fn create_filter_groups(batch: &QueryBatch) -> Vec<FilterGroup> {
    let batch_set: std::collections::HashSet<&str> =
        batch.query.resources.iter().map(String::as_str).collect();

    let mut sig_to_idx: HashMap<String, usize> = HashMap::new();
    let mut groups: Vec<FilterGroup> = Vec::new();

    for listener in &batch.listeners {
        let listener_set: std::collections::HashSet<&str> =
            listener.query.resources.iter().map(String::as_str).collect();

        let mut mask: Vec<String> = batch_set.difference(&listener_set)
            .map(|s| s.to_string())
            .collect();
        mask.sort();

        let stats_mask: Option<Vec<String>> = if !listener.query.stats && batch.query.stats {
            let mut sm: Vec<String> = listener.query.resources.iter()
                .filter(|r| matches!(r.as_str(), "laps" | "segments" | "events"))
                .cloned()
                .collect();
            sm.sort();
            Some(sm)
        } else {
            None
        };

        let sig = format!("{:?}|{:?}", mask, stats_mask);
        match sig_to_idx.get(&sig).copied() {
            Some(idx) => groups[idx].listeners.push(listener.clone()),
            None => {
                sig_to_idx.insert(sig, groups.len());
                groups.push(FilterGroup { listeners: vec![listener.clone()], mask, stats_mask });
            }
        }
    }

    groups
}

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
    /// The v2 query for this subscriber.  Empty (default) for v1 subscriptions
    /// where the fanout task ignores the query.
    query:          AthleteV2Query,
}

impl DelegationHandle {
    /// Adds a new per-client sink and returns the receiving end, the byte
    /// counter, and the close-signal notifier.  `query` is stored alongside
    /// the sink so the v2 fanout task can read all active queries at once.
    pub fn add_sink(
        &self,
        query: AthleteV2Query,
    ) -> (mpsc::UnboundedReceiver<(Value, usize)>, Arc<AtomicUsize>, Arc<Notify>) {
        let (tx, rx)       = mpsc::unbounded_channel();
        let buffered_bytes = Arc::new(AtomicUsize::new(0));
        let close_notify   = Arc::new(Notify::new());
        self.sinks.lock().unwrap().push(ClientSink {
            tx,
            buffered_bytes: Arc::clone(&buffered_bytes),
            close_notify:   Arc::clone(&close_notify),
            query,
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
    ///
    /// For v1 events (no `/v2` suffix) the delegation is keyed by
    /// `source/event/canonical_query` so each distinct query gets its own
    /// upstream receiver and its own fanout task.
    ///
    /// For v2 events (`/v2` suffix) the delegation is keyed by `source/event`
    /// only — all subscribers to the same event share one upstream receiver and
    /// the v2 fanout task merges their queries via `emit_v2`.
    pub fn subscribe(
        &self,
        source: &str,
        event:  &str,
        query:  &AthleteV2Query,
        state:  &Arc<WebState>,
    ) -> Result<SubscriptionHandle, String> {
        let key = if is_v2_event(event) {
            format!("{source}/{event}")
        } else {
            format!("{source}/{event}/{}", query.canonical_key())
        };
        let mut map = self.inner.lock().unwrap();

        // Reuse a live delegation.
        if let Some(weak) = map.get(&key) {
            if let Some(arc) = weak.upgrade() {
                let (sink, buffered_bytes, close_notify) = arc.add_sink(query.clone());
                return Ok(SubscriptionHandle {
                    sink, buffered_bytes, close_notify, delegation: arc,
                });
            }
        }

        // No live delegation; create one (tokio::spawn is safe under a Mutex guard).
        let arc = create_delegation(source, event, state)?;
        let (sink, buffered_bytes, close_notify) = arc.add_sink(query.clone());
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
            if is_v2_event(event) {
                tokio::spawn(stats_fanout_task_v2(rx, sinks_ref, state_ref, event_str))
            } else {
                tokio::spawn(stats_fanout_task(rx, sinks_ref, state_ref, event_str))
            }
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
/// be forwarded to subscribers.  Recognises the `/v2` suffix on all patterns.
fn event_matches_athlete(event: &str, athlete_id: i64, state: &WebState) -> bool {
    // Strip the optional /v2 suffix before pattern matching.
    let base = event.strip_suffix("/v2").unwrap_or(event);

    if base == "athlete/watching" {
        return state.watching_id.map(|id| id as i64) == Some(athlete_id);
    }
    if let Some(id_str) = base.strip_prefix("athlete/") {
        if let Ok(id) = id_str.parse::<i64>() {
            return athlete_id == id;
        }
    }
    true
}

/// Returns `true` when `event` is a v2 event path (ends with `/v2`).
fn is_v2_event(event: &str) -> bool {
    event.ends_with("/v2")
}

// ---------------------------------------------------------------------------
// v2 fanout task
// ---------------------------------------------------------------------------

/// Fanout task for v2 events.  All subscribers to the same event share this
/// single task regardless of their individual queries.  On each tick the
/// queries of all active sinks are batched via `emit_v2`, which picks the
/// cheapest formatting strategy, calls `format_athlete_v2` once per batch,
/// and applies per-group masks so each subscriber receives only the resources
/// it requested.
async fn stats_fanout_task_v2(
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

                // Hold the sinks lock for the entire operation so the list of
                // (query, sink) pairs is consistent between collecting queries
                // and distributing the per-query results.
                let mut sinks_guard = sinks.lock().unwrap();
                if sinks_guard.is_empty() {
                    continue;
                }

                let results: Option<Vec<Value>> = {
                    let registry = state.registry.read().unwrap();
                    registry.get(athlete_id_u32).map(|athlete| {
                        let listeners: Vec<QueryListener> = sinks_guard
                            .iter()
                            .map(|s| QueryListener { query: s.query.clone() })
                            .collect();
                        emit_v2(&listeners, |q| {
                            format_athlete_v2(
                                athlete,
                                &q.resources,
                                state.watching_id,
                                state.self_athlete_id,
                                q.stats,
                            )
                        })
                    })
                };

                let Some(results) = results else { continue };

                let mut i = 0;
                sinks_guard.retain(|sink| {
                    let data          = results[i].clone();
                    i += 1;
                    let byte_estimate = serde_json::to_string(&data)
                        .map(|s| s.len())
                        .unwrap_or(256)
                        + 50;
                    let queued = sink.buffered_bytes.load(Relaxed);
                    if queued + byte_estimate > MAX_BUFFERED_BYTES {
                        tracing::warn!(
                            buffered = queued,
                            "outbound buffer exceeded limit; disconnecting client"
                        );
                        sink.close_notify.notify_one();
                        false
                    } else {
                        sink.buffered_bytes.fetch_add(byte_estimate, Relaxed);
                        sink.tx.send((data, byte_estimate)).is_ok()
                    }
                });
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Closed)    => break,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

// ---------------------------------------------------------------------------
// apply_filter_group
// ---------------------------------------------------------------------------

/// Apply a filter group's mask to a formatted v2 payload.
///
/// - Keys listed in `group.mask` are removed from the returned object.
/// - For each slice resource listed in `group.stats_mask`, the `stats` field
///   inside every element of that array is set to `null`.
///
/// This is the Rust equivalent of the `filterData` closure produced by
/// `createFilterGroups` in `stats.mjs:809`.
pub fn apply_filter_group(mut value: Value, group: &FilterGroup) -> Value {
    if let Value::Object(ref mut map) = value {
        for key in &group.mask {
            map.remove(key);
        }
        if let Some(ref stats_mask) = group.stats_mask {
            for resource in stats_mask {
                if let Some(arr) = map.get_mut(resource) {
                    if let Value::Array(slices) = arr {
                        for slice in slices.iter_mut() {
                            if let Value::Object(slice_map) = slice {
                                slice_map.insert("stats".to_string(), Value::Null);
                            }
                        }
                    }
                }
            }
        }
    }
    value
}

// ---------------------------------------------------------------------------
// emit_v2
// ---------------------------------------------------------------------------

/// Format and fan out a v2 payload to a set of listeners, calling the
/// formatter at most once per batch.
///
/// The cheapest batching strategy is selected via `compute_query_cost`.
/// Within each batch, listeners are grouped by their required filter pass
/// (`create_filter_groups`); `apply_filter_group` strips keys and nulls
/// per-slice stats before the payload reaches each group.
///
/// Returns one `Value` per listener in the same order as the input slice.
///
/// Mirrors `QueryReductionEmitter.emit` / `_compileEventContext`
/// (`stats.mjs:652-748`).
pub fn emit_v2(
    listeners: &[QueryListener],
    format:    impl Fn(&AthleteV2Query) -> Value,
) -> Vec<Value> {
    if listeners.is_empty() {
        return Vec::new();
    }

    let strategies = create_query_strategies(listeners);
    let best = strategies
        .into_iter()
        .min_by(|a, b| {
            let cost = |s: &[QueryBatch]| -> f64 {
                s.iter().map(|batch| compute_query_cost(&batch.query)).sum()
            };
            cost(a).partial_cmp(&cost(b)).unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("non-empty listeners produce at least one strategy");

    // Map each canonical query key to its filtered payload.  Listeners with
    // the same canonical key (identical queries) share the same filtered value
    // and end up in the same filter group, so `or_insert_with` is correct.
    let mut key_to_value: HashMap<String, Value> = HashMap::new();

    for batch in &best {
        let super_payload = format(&batch.query);
        for group in create_filter_groups(batch) {
            let filtered = apply_filter_group(super_payload.clone(), &group);
            for listener in &group.listeners {
                key_to_value
                    .entry(listener.query.canonical_key())
                    .or_insert_with(|| filtered.clone());
            }
        }
    }

    listeners
        .iter()
        .map(|l| {
            key_to_value
                .get(&l.query.canonical_key())
                .cloned()
                .unwrap_or(Value::Null)
        })
        .collect()
}
