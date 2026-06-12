// SPDX-License-Identifier: AGPL-3.0-only
//! 18.18-T — v2 event subscriptions deliver v2-shaped payloads; bare event
//! subscriptions continue to deliver v1-shaped payloads.
//!
//! Two behaviours are verified:
//!
//! 1. `v2_event_subscription_delivers_v2_payload` — subscribing to
//!    `athlete/1001/v2` with `query:{resources:["stats"],stats:false}` must
//!    produce frames whose `data` object carries `version:2` and the requested
//!    `stats` resource, and must not carry the `lap` resource (not in the
//!    query).  Fails at runtime until 18.18-I routes `athlete/*/v2` events
//!    through `format_athlete_v2` in `stats_fanout_task`.
//!
//! 2. `bare_event_subscription_still_delivers_v1_payload` — subscribing to
//!    bare `athlete/1001` without a query must still produce v1-shaped frames
//!    (no `version` field; `lapCount`, `stats`, and `lap` present).  Must
//!    continue to pass after 18.18-I so the two event families do not regress
//!    one another.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.18-T.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ranchero::config::{EditingMode, ResolvedConfig, ZwiftEndpoints};
use ranchero::daemon::relay::GameEvent;
use ranchero::web::{start, AthleteRegistry, WebState};
use serde_json::json;
use tokio::sync::{broadcast, Notify};
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn test_config() -> ResolvedConfig {
    ResolvedConfig {
        main_email:            None,
        main_password:         None,
        monitor_email:         None,
        monitor_password:      None,
        server_bind:           "127.0.0.1".into(),
        server_port:           0,
        server_https:          false,
        log_level:             None,
        log_file:              PathBuf::from("/tmp/ranchero-subs-ws-v2-test.log"),
        pidfile:               PathBuf::from("/tmp/ranchero-subs-ws-v2-test.pid"),
        config_path:           None,
        editing_mode:          EditingMode::Default,
        zwift_endpoints:       ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base:  "http://127.0.0.1:1".into(),
        },
        relay_enabled:         false,
        watched_athlete_id:    None,
        server_pages_root:     PathBuf::from("pages"),
        server_https_cert_dir: PathBuf::from("https"),
        event_behavior:        Default::default(),
    }
}

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn ws_send(ws: &mut WsStream, payload: serde_json::Value) {
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .expect("ws send must succeed");
}

async fn ws_recv(ws: &mut WsStream) -> serde_json::Value {
    loop {
        let msg = ws.next().await
            .expect("ws stream must not end")
            .expect("ws frame must be valid");
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text)
                .expect("frame must be valid JSON");
        }
    }
}

fn player_state(athlete_id: i64) -> GameEvent {
    GameEvent::PlayerState { athlete_id }
}

/// Subscribing to `athlete/1001/v2` with a query must produce a v2 payload.
///
/// Fails until `stats_fanout_task` detects the `/v2` suffix and routes the
/// event through `format_athlete_v2` instead of `format_athlete_data_v1`.
#[tokio::test]
#[ignore = "slow: real socket"]
async fn v2_event_subscription_delivers_v2_payload() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);

    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);

    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),
    );

    let cfg      = test_config();
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": {
            "method": "subscribe",
            "arg": {
                "event":  "athlete/1001/v2",
                "source": "stats",
                "subId":  42,
                "query":  { "resources": ["stats"], "stats": false }
            }
        }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true, "subscribe to athlete/1001/v2 must succeed; got {r}");

    tx.send(player_state(1001)).expect("broadcast send");

    let ev = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("event must arrive within 2 s");

    assert_eq!(ev["type"],    "event", "frame type must be event; got {ev}");
    assert_eq!(ev["uid"],     42,      "uid must match subId; got {ev}");
    assert_eq!(ev["success"], true,    "got {ev}");

    let data = &ev["data"];

    // v2 payloads carry an explicit version field.
    assert_eq!(data["version"], 2,
        "v2 event subscription must deliver version:2 payload; got {ev}");

    // The queried resource must be present.
    assert!(data["stats"].is_object(),
        "v2 payload must include the requested 'stats' resource; got {ev}");

    // Resources not in the query must be absent (not carried at all).
    assert!(data.get("lap").is_none() || data["lap"].is_null(),
        "v2 payload must not include 'lap' when it is not in the query; got {ev}");

    shutdown.notify_one();
    handle.stop().await;
}

/// Subscribing to bare `athlete/1001` without a query must still deliver a
/// v1-shaped payload after 18.18-I.
///
/// Verifies that the two event families do not regress one another.
#[tokio::test]
#[ignore = "slow: real socket"]
async fn bare_event_subscription_still_delivers_v1_payload() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);

    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);

    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),
    );

    let cfg      = test_config();
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": {
            "method": "subscribe",
            "arg": { "event": "athlete/1001", "source": "stats", "subId": 11 }
        }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true, "subscribe to bare athlete/1001 must succeed; got {r}");

    tx.send(player_state(1001)).expect("broadcast send");

    let ev = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("event must arrive within 2 s");

    assert_eq!(ev["type"],    "event", "frame type must be event; got {ev}");
    assert_eq!(ev["uid"],     11,      "uid must match subId; got {ev}");

    let data = &ev["data"];

    // v1 payloads must not carry a version field.
    assert!(data.get("version").is_none() || data["version"].is_null(),
        "bare athlete/1001 subscription must deliver v1 payload (no version field); got {ev}");

    // v1 landmark fields must be present.
    assert!(data["lapCount"].is_number(),
        "v1 payload must include lapCount; got {ev}");
    assert!(data["stats"].is_object(),
        "v1 payload must include stats; got {ev}");
    assert!(data["lap"].is_object(),
        "v1 payload must include lap; got {ev}");

    shutdown.notify_one();
    handle.stop().await;
}
