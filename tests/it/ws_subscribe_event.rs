// SPDX-License-Identifier: AGPL-3.0-only
//! 17.17-T — WebSocket subscribe / unsubscribe.
//!
//! A subscribe request with `arg:{event:"athlete/watching", source:"stats",
//! subId:7}` produces a `{type:"response", success:true, uid:N}` reply and
//! then receives `{type:"event", uid:7, success:true, data:...}` frames as
//! the registry changes.  An unsubscribe request with `arg:{subId:7}` ends
//! the stream.
//!
//! Fails to compile until `WebState::and_game_events` is defined.
//! Fails at runtime until the /api/ws/events route, subscription engine,
//! and stats source are implemented.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.17-T.

use std::sync::Arc;

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

// S12-6: a "nearby" subscription must deliver a sorted array, not a single-athlete object.
#[tokio::test]
#[ignore = "slow: real socket"]
async fn nearby_ws_emits_sorted_array_not_single_athlete() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);

    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);
    registry.upsert(1002, 5, 0, 0.0, 0.0);

    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),
    );

    let cfg      = super::common::test_config("ws-sub");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request",
        "uid":  1,
        "data": { "method": "subscribe", "arg": { "event": "nearby", "source": "stats", "subId": 9 } }
    })).await;

    let sub_resp = ws_recv(&mut ws).await;
    assert_eq!(sub_resp["success"], true, "subscribe must succeed; got {sub_resp}");

    tx.send(GameEvent::PlayerState { athlete_id:    1001 }).expect("broadcast send must succeed");

    let event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ws_recv(&mut ws),
    ).await.expect("event frame must arrive within 2 s");

    assert!(event["data"].is_array(),
        "S12-6: nearby subscription must deliver an array; got {event}");

    shutdown.notify_one();
    handle.stop().await;
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_subscribe_receives_event_then_unsubscribe_stops_stream() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);

    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);

    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),  // fails to compile until and_game_events is added
    );

    let cfg      = super::common::test_config("ws-sub");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    // Subscribe to athlete/watching with subId 7.
    ws_send(&mut ws, json!({
        "type": "request",
        "uid":  1,
        "data": {
            "method": "subscribe",
            "arg": { "event": "athlete/watching", "source": "stats", "subId": 7 }
        }
    })).await;

    let sub_resp = ws_recv(&mut ws).await;
    assert_eq!(sub_resp["type"],    "response", "subscribe must get a response; got {sub_resp}");
    assert_eq!(sub_resp["success"], true,        "subscribe must succeed; got {sub_resp}");
    assert_eq!(sub_resp["uid"],     1,           "got {sub_resp}");

    // Inject a PlayerState event for athlete 1001 to trigger a stats update.
    tx.send(GameEvent::PlayerState { athlete_id:    1001 }).expect("broadcast send must succeed");

    // Receive the resulting event frame.
    let event = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ws_recv(&mut ws),
    ).await.expect("event frame must arrive within 2 s");

    assert_eq!(event["type"],    "event", "must receive an event frame; got {event}");
    assert_eq!(event["uid"],     7,       "event uid must match subId; got {event}");
    assert_eq!(event["success"], true,    "got {event}");
    assert!(event["data"].is_object(),    "data must be an object; got {event}");
    assert_eq!(event["data"]["athleteId"], 1001, "data must describe athlete 1001; got {event}");

    // Unsubscribe.
    ws_send(&mut ws, json!({
        "type": "request",
        "uid":  2,
        "data": { "method": "unsubscribe", "arg": { "subId": 7 } }
    })).await;

    let unsub_resp = ws_recv(&mut ws).await;
    assert_eq!(unsub_resp["type"],    "response", "unsubscribe must get a response; got {unsub_resp}");
    assert_eq!(unsub_resp["success"], true,        "unsubscribe must succeed; got {unsub_resp}");

    // Send another event; no further event frame should arrive.
    // After unsubscribe the subscription task is aborted, leaving no receivers —
    // the send failure is expected and intentional.
    tx.send(GameEvent::PlayerState { athlete_id:    1001 }).ok();

    let nothing = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        ws_recv(&mut ws),
    ).await;
    assert!(nothing.is_err(), "no event frame must arrive after unsubscribe");

    shutdown.notify_one();
    handle.stop().await;
}
