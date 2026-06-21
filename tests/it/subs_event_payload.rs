// SPDX-License-Identifier: AGPL-3.0-only
//! 17.22-T — Subscription event payload shape and athlete-specific filtering.
//!
//! Two behaviours are verified:
//!
//! 1. `athlete_watching_event_payload_matches_v1_shape` — an `athlete/watching`
//!    subscription emits frames whose `data` object contains all fields produced
//!    by the v1 athlete formatter (`athleteId`, `courseId`, `lapCount`, `stats`,
//!    `lap`).  The values update as the underlying `AthleteData` changes between
//!    emitted events.
//!
//! 2. `athlete_id_subscription_filters_to_named_athlete` — a subscription with
//!    `event:"athlete/1001"` must only emit frames for athlete 1001.  Events
//!    carrying athlete 1002's `PlayerState` must be suppressed even when athlete
//!    1002 is present in the registry.
//!
//! Test 2 fails at runtime with the current implementation, which does not
//! filter by the athlete ID embedded in the event path (17.22-I).
//!
//! See docs/plans/STEP-17-web-server.md, item 17.22-T.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ranchero::daemon::relay::GameEvent;
use ranchero::web::{start, AthleteRegistry, WebState};
use serde_json::json;
use tokio::sync::{broadcast, Notify};
use tokio_tungstenite::{connect_async, tungstenite::Message};

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

#[tokio::test]
#[ignore = "slow: real socket"]
async fn athlete_watching_event_payload_matches_v1_shape() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);

    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);

    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),
    );
    let state_ref = Arc::clone(&state);

    let cfg      = super::common::test_config("subs-payload");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": { "method": "subscribe",
                  "arg": { "event": "athlete/watching", "source": "stats", "subId": 7 } }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true, "subscribe must succeed; got {r}");

    // First event: athlete on course 5.
    tx.send(player_state(1001)).expect("broadcast send");
    let ev = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("event must arrive within 2 s");

    assert_eq!(ev["type"],    "event", "frame type must be event; got {ev}");
    assert_eq!(ev["uid"],     7,       "uid must match subId; got {ev}");
    assert_eq!(ev["success"], true,    "got {ev}");

    let data = &ev["data"];
    assert!(data.is_object(), "data must be an object; got {ev}");
    assert_eq!(data["athleteId"], 1001, "data.athleteId must be 1001; got {ev}");
    assert_eq!(data["courseId"],  5,    "data.courseId must be 5; got {ev}");
    assert!(data["lapCount"].is_number(), "data.lapCount must be present; got {ev}");
    assert!(data["stats"].is_object(),   "data.stats must be present; got {ev}");
    assert!(data["lap"].is_object(),     "data.lap must be present; got {ev}");

    // Update the athlete's course in the shared registry, then inject a second event.
    // The next frame must reflect the updated value.
    state_ref.registry.write().unwrap()
        .get_mut(1001).unwrap()
        .course_id = 9;

    tx.send(player_state(1001)).expect("broadcast send");
    let ev2 = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("updated event must arrive within 2 s");
    assert_eq!(ev2["data"]["courseId"], 9,
        "data.courseId must reflect updated registry value; got {ev2}");

    shutdown.notify_one();
    handle.stop().await;
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn athlete_id_subscription_filters_to_named_athlete() {
    // Fails until the stats source filters events by the athlete ID in the
    // event path (e.g. "athlete/1001" only emits for athlete 1001).
    let (tx, _) = broadcast::channel::<GameEvent>(16);

    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);
    registry.upsert(1002, 5, 0, 0.0, 0.0);

    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),
    );

    let cfg      = super::common::test_config("subs-payload");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    // Subscribe specifically to athlete 1001.
    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": { "method": "subscribe",
                  "arg": { "event": "athlete/1001", "source": "stats", "subId": 11 } }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true, "subscribe to athlete/1001 must succeed; got {r}");

    // Emit a PlayerState for athlete 1002; it must not reach the subscriber.
    tx.send(player_state(1002)).ok();
    let nothing = tokio::time::timeout(
        Duration::from_millis(200),
        ws_recv(&mut ws),
    ).await;
    assert!(nothing.is_err(),
        "athlete/1001 subscription must not receive events for athlete 1002");

    // Emit a PlayerState for athlete 1001; the subscriber must receive it.
    tx.send(player_state(1001)).expect("broadcast send");
    let ev = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("athlete/1001 subscription must receive events for athlete 1001");
    assert_eq!(ev["uid"],              11,   "event uid must match subId; got {ev}");
    assert_eq!(ev["data"]["athleteId"], 1001, "data must describe athlete 1001; got {ev}");

    shutdown.notify_one();
    handle.stop().await;
}
