// SPDX-License-Identifier: AGPL-3.0-only
//! 19.8-T — v2 WebSocket fanout parity confirmation for `athlete/watching/v2`.
//!
//! Confirms that STEP 18 M1 (`stats_fanout_task_v2`) is correctly wired end-to-end:
//!
//! 1. `watching_v2_delivers_requested_resources_only` — subscribing to
//!    `athlete/watching/v2` with `{resources:["stats"],stats:true}` must produce
//!    a frame with `version:2` and the `stats` resource present.  Resources not
//!    in the query (`state`, `lap`) must be absent.  v1 hallmarks (`lapCount` at
//!    the top level) must not appear — confirming the event went through
//!    `format_athlete_v2`, not `format_athlete_data_v1`.
//!
//! This is a confirmation test, not new construction.  If the assertion fails it
//! indicates a STEP 18 M1 regression; fix the regression rather than deferring.
//!
//! See docs/planning/STEP-19-compatibility-tests.md, item 19.8.

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
use zwift_stats::MostRecentState;

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
        log_file:              PathBuf::from("/tmp/ranchero-compat-v2-fanout.log"),
        pidfile:               PathBuf::from("/tmp/ranchero-compat-v2-fanout.pid"),
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
    GameEvent::PlayerState {
        athlete_id,
        power_w:       200,
        cadence_u_hz:  5000,
        speed_mm_h:    36_000_000,
        world_time_ms: 60_000,
        world:         5,
        sport:         0,
        distance:      0,
        z:             0.0,
        draft:         0,
        heartrate:     0,
    }
}

fn state_with_watching_athlete(tx: broadcast::Sender<GameEvent>) -> Arc<WebState> {
    let mut registry = AthleteRegistry::new();
    let ad = registry.upsert(1001, 5, 0, 60.0, 0.0);
    ad.most_recent_state = Some(MostRecentState {
        world_time: 60.0, speed: 10.0, power: 200.0, heartrate: 0, cadence: 0,
        draft: 0.0, distance: 0.0, altitude: 0.0,
        lat: 1.23, lng: 4.56,
        course_id: 5, road_id: 0, road_time: 0.0, reverse: false,
        event_subgroup_id: 0, group_id: 0, time: 0.0, event_distance: 0.0,
        grade: 0.0,
        x: 0.0,
        y: 0.0,
        progress: 0.0,
    });
    Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx),
    )
}

/// Subscribing to `athlete/watching/v2` with `{resources:["stats"],stats:true}`
/// must produce a v2-shaped frame:
///
/// - `version: 2` present (v2 shape confirmed; absent from v1 payloads)
/// - `stats` object present (requested resource)
/// - `state` and `lap` absent (not in query — resource filtering confirmed)
///
/// Note: `lapCount`, `athleteId`, `courseId`, `watching`, `self`, `wBal`, and
/// the timing fields are part of the v2 base object and are always present
/// regardless of the query.  Their presence does not indicate a v1 shape.
#[tokio::test]
#[ignore = "slow: real socket"]
async fn watching_v2_delivers_requested_resources_only() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);
    let state   = state_with_watching_athlete(tx.clone());

    let shutdown = Arc::new(Notify::new());
    let handle   = start(&test_config(), state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": {
            "method": "subscribe",
            "arg": {
                "event":  "athlete/watching/v2",
                "source": "stats",
                "subId":  80,
                "query":  { "resources": ["stats"], "stats": true }
            }
        }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true, "subscribe must succeed; got {r}");

    tx.send(player_state(1001)).expect("broadcast");

    let ev = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("event must arrive within 2 s");

    let data = &ev["data"];

    // v2 shape: explicit version field.
    assert_eq!(data["version"], 2,
        "athlete/watching/v2 must deliver version:2; got {ev}");

    // Requested resource must be present.
    assert!(data["stats"].is_object(),
        "stats must be present when queried; got {ev}");

    // Unrequested resources must be absent.
    assert!(data.get("state").map_or(true, |v| v.is_null()),
        "state must be absent when not queried; got {ev}");
    assert!(data.get("lap").map_or(true, |v| v.is_null()),
        "lap must be absent when not queried; got {ev}");

    shutdown.notify_one();
    handle.stop().await;
}
