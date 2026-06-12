// SPDX-License-Identifier: AGPL-3.0-only
//! 17.18-T — WebSocket subscription uid isolation.
//!
//! Two clients subscribe to the same event (athlete/watching) with different
//! subIds.  Each receives its own event stream.  Unsubscribing on client A
//! does not stop event delivery to client B.
//!
//! Fails to compile until `WebState::and_game_events` is defined.
//! Fails at runtime until the subscription engine enforces per-client
//! subId isolation.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.18-T.

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
        log_file:              PathBuf::from("/tmp/ranchero-ws-uid-test.log"),
        pidfile:               PathBuf::from("/tmp/ranchero-ws-uid-test.pid"),
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

fn player_state(athlete_id: i64, _power_w: i32) -> GameEvent {
    // Step 19: `GameEvent::PlayerState` carries only `athlete_id`. The
    // power-watts parameter survives so call sites need not change.
    GameEvent::PlayerState { athlete_id }
}

#[tokio::test]
#[ignore = "slow: real socket"]
async fn ws_unsubscribe_on_client_a_does_not_affect_client_b() {
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

    // Connect two independent clients.
    let mut ws_a = connect_async(&url).await.expect("client A connect").0;
    let mut ws_b = connect_async(&url).await.expect("client B connect").0;

    // Client A subscribes with subId 10.
    ws_send(&mut ws_a, json!({
        "type": "request", "uid": 1,
        "data": { "method": "subscribe",
                  "arg": { "event": "athlete/watching", "source": "stats", "subId": 10 } }
    })).await;
    let r = ws_recv(&mut ws_a).await;
    assert_eq!(r["success"], true, "client A subscribe must succeed; got {r}");

    // Client B subscribes with subId 20.
    ws_send(&mut ws_b, json!({
        "type": "request", "uid": 1,
        "data": { "method": "subscribe",
                  "arg": { "event": "athlete/watching", "source": "stats", "subId": 20 } }
    })).await;
    let r = ws_recv(&mut ws_b).await;
    assert_eq!(r["success"], true, "client B subscribe must succeed; got {r}");

    // Inject an event; both clients should receive it.
    tx.send(player_state(1001, 200)).expect("broadcast send");

    let ev_a = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ws_recv(&mut ws_a),
    ).await.expect("client A must receive first event");
    assert_eq!(ev_a["uid"], 10, "client A event uid must be subId 10; got {ev_a}");

    let ev_b = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ws_recv(&mut ws_b),
    ).await.expect("client B must receive first event");
    assert_eq!(ev_b["uid"], 20, "client B event uid must be subId 20; got {ev_b}");

    // Client A unsubscribes.
    ws_send(&mut ws_a, json!({
        "type": "request", "uid": 2,
        "data": { "method": "unsubscribe", "arg": { "subId": 10 } }
    })).await;
    let r = ws_recv(&mut ws_a).await;
    assert_eq!(r["success"], true, "client A unsubscribe must succeed; got {r}");

    // Inject another event.
    tx.send(player_state(1001, 250)).expect("broadcast send");

    // Client A must receive nothing.
    let nothing = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        ws_recv(&mut ws_a),
    ).await;
    assert!(nothing.is_err(), "client A must not receive event after unsubscribe");

    // Client B must still receive the event.
    let ev_b2 = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ws_recv(&mut ws_b),
    ).await.expect("client B must still receive events after client A unsubscribes");
    assert_eq!(ev_b2["uid"], 20, "client B event uid must still be subId 20; got {ev_b2}");

    shutdown.notify_one();
    handle.stop().await;
}
