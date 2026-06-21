// SPDX-License-Identifier: AGPL-3.0-only
//! 17.20-T — WebSocket source lookup.
//!
//! A subscribe request with `source:"stats"` succeeds.
//! A subscribe request with `source:"gameConnection"` succeeds.
//! A subscribe request with an unknown source produces `success:false`
//! and an error message that names the unknown source.
//!
//! `game_connection_source_is_recognized` fails at runtime until
//! `gameConnection` is added to the source registry (17.20-I).
//!
//! See docs/plans/STEP-17-web-server.md, item 17.20-T.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use ranchero::daemon::relay::GameEvent;
use ranchero::web::{start, WebState};
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

/// Returns a basic subscribe payload for the given source.
fn subscribe_msg(uid: i64, source: &str, sub_id: i64) -> serde_json::Value {
    json!({
        "type": "request",
        "uid":  uid,
        "data": {
            "method": "subscribe",
            "arg": { "event": "athlete/watching", "source": source, "subId": sub_id }
        }
    })
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn stats_source_is_recognized() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);
    let state = Arc::new(WebState::new().and_game_events(tx));

    let cfg      = super::common::test_config("subs-source");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, subscribe_msg(1, "stats", 1)).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true,
        "subscribe with source:stats must succeed; got {r}");

    shutdown.notify_one();
    handle.stop().await;
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn game_connection_source_is_recognized() {
    // Fails until gameConnection is registered in the source registry (17.20-I).
    let (tx, _) = broadcast::channel::<GameEvent>(16);
    let state = Arc::new(WebState::new().and_game_events(tx));

    let cfg      = super::common::test_config("subs-source");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request",
        "uid":  1,
        "data": {
            "method": "subscribe",
            "arg": { "event": "status", "source": "gameConnection", "subId": 1 }
        }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true,
        "subscribe with source:gameConnection must succeed; got {r}");

    shutdown.notify_one();
    handle.stop().await;
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn unknown_source_returns_descriptive_error() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);
    let state = Arc::new(WebState::new().and_game_events(tx));

    let cfg      = super::common::test_config("subs-source");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, subscribe_msg(1, "banana", 1)).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], false,
        "subscribe with unknown source must fail; got {r}");

    let error = r["error"].as_str().unwrap_or("");
    assert!(error.contains("unknown source"),
        "error must name the unknown source; got {r}");

    shutdown.notify_one();
    handle.stop().await;
}
