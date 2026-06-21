// SPDX-License-Identifier: AGPL-3.0-only
//! 17.21-T — Subscription delegation deduplication.
//!
//! Two clients subscribing to the same (source, event) pair must share
//! a single upstream broadcast receiver, not create two independent ones.
//! The observable is `broadcast::Sender::receiver_count()`:
//!
//! - After one subscription:   receiver_count == 1
//! - After two subscriptions:  receiver_count == 1  (deduped, not 2)
//! - Both clients receive every emitted event.
//! - After the first unsubscribes: receiver_count == 1  (upstream stays)
//! - After the second unsubscribes + brief settle: receiver_count == 0
//!
//! `two_subscriptions_share_one_upstream_receiver` fails at runtime until
//! delegation deduplication is implemented (17.21-I).
//!
//! See docs/plans/STEP-17-web-server.md, item 17.21-T.

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
async fn two_subscriptions_share_one_upstream_receiver() {
    // Fails until the subscription engine deduplicates upstream receivers (17.21-I).
    let (tx, _) = broadcast::channel::<GameEvent>(16);

    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);

    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),
    );

    let cfg      = super::common::test_config("subs-dedup");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());

    let mut ws_a = connect_async(&url).await.expect("client A connect").0;
    let mut ws_b = connect_async(&url).await.expect("client B connect").0;

    // Client A subscribes.  The server creates the upstream broadcast receiver
    // synchronously before returning the response, so the count is stable once
    // the response is received.
    ws_send(&mut ws_a, json!({
        "type": "request", "uid": 1,
        "data": { "method": "subscribe",
                  "arg": { "event": "athlete/watching", "source": "stats", "subId": 10 } }
    })).await;
    let r = ws_recv(&mut ws_a).await;
    assert_eq!(r["success"], true, "client A subscribe must succeed; got {r}");
    assert_eq!(tx.receiver_count(), 1,
        "one subscription must produce one upstream receiver");

    // Client B subscribes to the same event.  With deduplication the upstream
    // receiver count must remain 1; without deduplication it would rise to 2.
    ws_send(&mut ws_b, json!({
        "type": "request", "uid": 1,
        "data": { "method": "subscribe",
                  "arg": { "event": "athlete/watching", "source": "stats", "subId": 20 } }
    })).await;
    let r = ws_recv(&mut ws_b).await;
    assert_eq!(r["success"], true, "client B subscribe must succeed; got {r}");
    assert_eq!(tx.receiver_count(), 1,
        "two subscriptions to the same event must share one upstream receiver");

    // Both clients must receive each emitted event.
    tx.send(player_state(1001)).expect("broadcast send");

    let ev_a = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws_a))
        .await.expect("client A must receive event");
    assert_eq!(ev_a["uid"], 10, "client A event uid must be subId 10; got {ev_a}");

    let ev_b = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws_b))
        .await.expect("client B must receive event");
    assert_eq!(ev_b["uid"], 20, "client B event uid must be subId 20; got {ev_b}");

    // Client A unsubscribes; the upstream receiver must stay (client B still active).
    ws_send(&mut ws_a, json!({
        "type": "request", "uid": 2,
        "data": { "method": "unsubscribe", "arg": { "subId": 10 } }
    })).await;
    let r = ws_recv(&mut ws_a).await;
    assert_eq!(r["success"], true, "client A unsubscribe must succeed; got {r}");
    // Allow the aborted task to settle before inspecting the count.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(tx.receiver_count(), 1,
        "upstream receiver must remain after one of two clients unsubscribes");

    // Client B unsubscribes; the upstream receiver must now be released.
    ws_send(&mut ws_b, json!({
        "type": "request", "uid": 2,
        "data": { "method": "unsubscribe", "arg": { "subId": 20 } }
    })).await;
    let r = ws_recv(&mut ws_b).await;
    assert_eq!(r["success"], true, "client B unsubscribe must succeed; got {r}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(tx.receiver_count(), 0,
        "upstream receiver must be released after last subscriber departs");

    shutdown.notify_one();
    handle.stop().await;
}
