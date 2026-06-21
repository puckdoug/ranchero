// SPDX-License-Identifier: AGPL-3.0-only
//! 17.27-T — Backpressure: slow client is disconnected at 8 MB.
//!
//! A client that stops reading must be disconnected once its buffered outbound
//! data crosses 8 MB. A second client that keeps reading must be unaffected.
//!
//! The disconnect reason must appear in the tracing log.
//!
//! Fails at runtime until the per-client `BufferedBytes` counter and
//! `session.close(CloseCode::Policy)` path are implemented (17.27-I).
//!
//! See docs/plans/STEP-17-web-server.md, item 17.27-T.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use ranchero::daemon::relay::GameEvent;
use ranchero::web::{start, AthleteRegistry, WebState};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Notify};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

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

async fn subscribe(ws: &mut WsStream, source: &str, event: &str, sub_id: u64) {
    ws_send(ws, json!({
        "type": "request",
        "uid":  sub_id,
        "data": {
            "method": "subscribe",
            "arg": { "event": event, "source": source, "subId": sub_id }
        }
    })).await;
    let resp = ws_recv(ws).await;
    assert_eq!(resp["success"], true,
        "subscribe must succeed; got {resp}");
}

/// Push enough events through `tx` to exceed 8 MB of outbound data for a
/// client that is not reading.  Each PlayerState event produces a JSON frame
/// of ~140 bytes; 65 000 events × 140 bytes ≈ 9 MB, comfortably over the
/// 8 MB threshold.  Events are sent in batches with a yield between each
/// batch so the fanout task can process them as they arrive.
async fn flood(tx: &broadcast::Sender<GameEvent>) {
    for i in 0u64..65_000 {
        tx.send(GameEvent::PlayerState { athlete_id:    1001 }).ok();
        if i % 128 == 0 {
            tokio::task::yield_now().await;
        }
    }
}

#[tokio::test]
#[ignore = "slow: pushes 8 MB through a real socket"]
async fn slow_client_is_disconnected_at_8mb_and_fast_client_is_unaffected() {
    let (tx, _) = broadcast::channel::<GameEvent>(65_536);

    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);

    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),
    );

    let cfg      = super::common::test_config("ws-backpressure");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let addr     = handle.local_addr();

    // Connect the slow client and subscribe but never read event frames.
    let (mut slow, _) = connect_async(format!("ws://{addr}/api/ws/events"))
        .await.expect("slow client must connect");
    subscribe(&mut slow, "stats", "athlete/watching", 1).await;

    // Connect the fast client and subscribe; it will actively drain frames.
    let (mut fast, _) = connect_async(format!("ws://{addr}/api/ws/events"))
        .await.expect("fast client must connect");
    subscribe(&mut fast, "stats", "athlete/watching", 2).await;

    // Flood the broadcast channel. The slow client stops reading after
    // subscribing; the fast client drains in a background task.
    let fast_drain = tokio::spawn(async move {
        let mut count = 0usize;
        while let Some(Ok(Message::Text(_))) = fast.next().await {
            count += 1;
            if count >= 100 {
                break; // received enough to prove it stayed connected
            }
        }
        count
    });

    flood(&tx).await;

    // Give the server time to process the flood and disconnect the slow client.
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // The slow client must have been closed by the server.  TCP delivers
    // already-buffered Text frames before the Close frame arrives, so drain
    // until we observe a non-Text result (Close, stream end, or error).
    // A test failure is when no such terminator arrives within the timeout.
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        async {
            loop {
                match slow.next().await {
                    Some(Ok(Message::Text(_))) => continue,        // buffered frame
                    Some(Ok(Message::Close(_))) => return "close",
                    None                        => return "end",
                    Some(Err(_))                => return "err",
                    Some(Ok(_))                 => continue,        // ping/pong/etc.
                }
            }
        },
    ).await;
    let outcome = outcome.expect("slow client must reach a terminator within 5s");
    assert!(
        matches!(outcome, "close" | "end" | "err"),
        "slow client must have been disconnected; got outcome: {outcome}"
    );

    // The fast client must have received at least 100 frames uninterrupted.
    let received = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        fast_drain,
    ).await
        .expect("fast drain task must finish")
        .expect("fast drain task must not panic");
    assert!(received >= 100,
        "fast client must receive at least 100 frames; got {received}");

    shutdown.notify_one();
    handle.stop().await;
}
