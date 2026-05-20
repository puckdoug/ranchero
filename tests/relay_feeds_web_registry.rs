// SPDX-License-Identifier: AGPL-3.0-only
//! 17.38-T — The relay-to-web bridge populates the registry and triggers
//! WebSocket event delivery with correct ordering.
//!
//! `route_player_state` (17.28-I) updates the `AthleteRegistry`.  The stats
//! fanout task reads that registry when a `GameEvent::PlayerState` arrives on
//! `game_events_tx`.  If the event is emitted before the registry is updated,
//! the fanout finds no entry and sends nothing to subscribers — a
//! read-before-write race.
//!
//! The bridge function (17.38-I) enforces the correct order:
//!   1. Call `route_player_state` → registry updated.
//!   2. Send `GameEvent::PlayerState` → fanout reads the registry.
//!
//! This test verifies the end-to-end outcome with an empty registry at entry
//! (no pre-seeded athletes), so any event that the WS subscriber receives
//! can only have come from the bridge populating the registry first.
//!
//! **This test fails to compile** until 17.38-I adds
//! `ranchero::web::proto_to_stats::bridge_player_state_event`.
//! Once that function exists with correct ordering the test should pass.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.38-T.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use actix_web::{test as axtest, web, App};
use futures_util::{SinkExt, StreamExt};
use ranchero::config::{EditingMode, ResolvedConfig, ZwiftEndpoints};
use ranchero::daemon::relay::GameEvent;
use ranchero::web::{http::configure_api, proto_to_stats, start, WebState};
use serde_json::json;
use tokio::sync::{broadcast, Notify};
use tokio_tungstenite::{connect_async, tungstenite::Message};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
        log_file:              PathBuf::from("/tmp/ranchero-bridge-test.log"),
        pidfile:               PathBuf::from("/tmp/ranchero-bridge-test.pid"),
        config_path:           None,
        editing_mode:          EditingMode::Default,
        zwift_endpoints:       ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base:  "http://127.0.0.1:1".into(),
        },
        relay_enabled:         false,
        watched_athlete_id:    Some(54321),
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
        .expect("ws send");
}

async fn ws_recv_text(ws: &mut WsStream) -> serde_json::Value {
    loop {
        let msg = ws.next().await
            .expect("ws stream must not end")
            .expect("ws frame must be valid");
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text).expect("ws JSON");
        }
    }
}

/// Proto fixture: athlete 54321, course 7, sport 2, power 300 W, 90 rpm,
/// speed 10 m/s, heartrate 160 bpm.
fn player_state_proto() -> zwift_proto::PlayerState {
    zwift_proto::PlayerState {
        id:            Some(54321),
        world:         Some(7),
        sport:         Some(2),
        power:         Some(300),
        cadence_u_hz:  Some(1_500_000), // 1_500_000 µHz × 60 / 1_000_000 = 90 rpm
        speed:         Some(36_000_000), // 36_000_000 mm/h ÷ 3_600_000 = 10 m/s
        heartrate:     Some(160),
        world_time:    Some(5_000),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Bridging a `PlayerState` proto causes `GET /api/athlete/v1/54321` to return
/// the athlete's data and a WS `stats` subscription to receive an event frame
/// with the right values.
///
/// Fails to compile until 17.38-I adds `proto_to_stats::bridge_player_state_event`.
/// Once that function exists with correct ordering (registry update → event
/// emit), this test passes without modification.
#[tokio::test]
#[ignore = "slow: real socket"]
async fn bridge_populates_registry_then_triggers_ws_event() {
    let (tx, _rx) = broadcast::channel::<GameEvent>(16);

    // Start with an empty registry — no athletes pre-seeded.
    let state = Arc::new(
        WebState::with_registry(
            zwift_stats::AthleteRegistry::new(),
            Some(54321),
            Some(54321),
        )
        .and_game_events(tx.clone()),
    );

    let cfg      = test_config();
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, Arc::clone(&state), shutdown.clone())
        .await
        .expect("server must start");

    let url    = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws = connect_async(&url).await.expect("ws connect").0;

    // Subscribe to the athlete before the bridge fires.
    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": { "method": "subscribe",
                  "arg": { "event": "athlete/54321", "source": "stats", "subId": 9 } }
    })).await;
    let sub = ws_recv_text(&mut ws).await;
    assert_eq!(sub["success"], true, "subscribe must succeed; got {sub}");

    // --- The bridge: registry update first, then event emit. ----------------
    //
    // bridge_player_state_event does NOT yet exist; this line fails to compile
    // until 17.38-I adds it to src/web/proto_to_stats.rs.
    //
    // The function must:
    //   1. Call route_player_state (updates registry with course_id=7, sport=2, etc.)
    //   2. Send GameEvent::PlayerState on web_state.game_events_tx
    // If the order is reversed the stats fanout reads an empty registry and
    // the assertion below times out.
    let now          = 1.0_f64;
    let wall_clock_ms: u64 = 1_700_000_000_000;
    let proto = player_state_proto();
    proto_to_stats::bridge_player_state_event(&proto, &state, now, wall_clock_ms);

    // The fanout must have read an already-populated registry entry.
    let event = tokio::time::timeout(Duration::from_secs(3), ws_recv_text(&mut ws))
        .await
        .expect("WS event must arrive within 3 s after bridge_player_state_event");

    assert_eq!(event["type"],    "event", "got {event}");
    assert_eq!(event["uid"],     9,       "uid must match subId; got {event}");
    assert_eq!(event["success"], true,    "got {event}");

    let data = &event["data"];
    assert!(data.is_object(),                  "data must be an object; got {event}");
    assert_eq!(data["athleteId"], 54321,       "athleteId must be 54321; got {event}");
    assert_eq!(data["courseId"],  7,           "courseId must be 7 (from proto.world); got {event}");

    // Verify that the HTTP endpoint also reflects the proto (registry was populated).
    let app = axtest::init_service(
        App::new()
            .app_data(web::Data::new(Arc::clone(&state)))
            .configure(configure_api),
    ).await;
    let req   = axtest::TestRequest::get().uri("/api/athlete/v1/54321").to_request();
    let resp  = axtest::call_service(&app, req).await;
    assert_eq!(resp.status().as_u16(), 200,
        "GET /api/athlete/v1/54321 must return 200 after bridge populates the registry");
    let body: serde_json::Value = axtest::read_body_json(resp).await;
    assert_eq!(body["athleteId"], 54321, "body athleteId; got {body}");
    assert_eq!(body["courseId"],  7,     "courseId must match proto.world; got {body}");

    shutdown.notify_one();
    handle.stop().await;
}
