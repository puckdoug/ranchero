// SPDX-License-Identifier: AGPL-3.0-only
//! 17.19-T — WebSocket disconnect drops delegation reference counts.
//!
//! Closing a WebSocket while subscriptions are active releases the
//! delegation.  When the last subscriber departs, the upstream listener
//! is removed.  A new client subscribing to the same event after the
//! previous client disconnected must receive a fresh response (success:true)
//! and then receive events normally — evidence that the delegation was
//! cleaned up rather than left in a broken state.
//!
//! Fails to compile until `WebState::and_game_events` is defined.
//! Fails at runtime until disconnect cleanup is implemented.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.19-T.

use std::path::PathBuf;
use std::sync::Arc;

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
        log_file:              PathBuf::from("/tmp/ranchero-ws-disc-test.log"),
        pidfile:               PathBuf::from("/tmp/ranchero-ws-disc-test.pid"),
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

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_new_client_can_subscribe_after_previous_client_disconnects() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);

    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);

    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),  // fails to compile until and_game_events is added
    );

    let cfg      = test_config();
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());

    // Client A subscribes, then disconnects without unsubscribing.
    {
        let mut ws_a = connect_async(&url).await.expect("client A connect").0;
        ws_send(&mut ws_a, json!({
            "type": "request", "uid": 1,
            "data": { "method": "subscribe",
                      "arg": { "event": "athlete/watching", "source": "stats", "subId": 5 } }
        })).await;
        let r = ws_recv(&mut ws_a).await;
        assert_eq!(r["success"], true, "client A subscribe must succeed; got {r}");
        // ws_a drops here — connection closes with active subscription.
    }

    // Give the server a moment to process the disconnect.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Client B subscribes to the same event; the server must handle it cleanly.
    let mut ws_b = connect_async(&url).await.expect("client B connect").0;
    ws_send(&mut ws_b, json!({
        "type": "request", "uid": 1,
        "data": { "method": "subscribe",
                  "arg": { "event": "athlete/watching", "source": "stats", "subId": 9 } }
    })).await;
    let r = ws_recv(&mut ws_b).await;
    assert_eq!(r["success"], true,
        "client B subscribe must succeed after client A disconnected; got {r}");

    // Client B must receive events normally.
    tx.send(GameEvent::PlayerState {
        athlete_id:    1001,
        power_w:       200,
        cadence_u_hz:  5000,
        speed_mm_h:    36_000_000,
        world_time_ms: 1_000_000,
    }).expect("broadcast send");

    let ev = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ws_recv(&mut ws_b),
    ).await.expect("client B must receive event");
    assert_eq!(ev["uid"],     9,    "event uid must match subId 9; got {ev}");
    assert_eq!(ev["success"], true, "got {ev}");

    shutdown.notify_one();
    handle.stop().await;
}
