// SPDX-License-Identifier: AGPL-3.0-only
//! 19.6 — WebSocket widget parity and headless render snapshots.
//!
//! Two test layers:
//!
//! **Payload contract (fast, `#[ignore = "slow: real socket"]`).**
//! Start the web server, subscribe to each event the widget pages use, push a
//! `PlayerState`, and assert the payload shape.  Includes a position assertion
//! (`state.lat` / `state.lng` as separate scalars) that exposes the deviation
//! from sauce4zwift's `state.latlng: [lat, lng]` array — to be resolved by
//! 19.7.
//!
//! **Rendered snapshot (slow, `#[ignore = "slow: headless browser render"]`).**
//! Drive each generated widget page headless against the running server, push a
//! `PlayerState`, and compare key DOM element text to golden snapshots committed
//! in `compat/expected/golden_render/`.  If a golden does not exist, the test
//! writes it (snapshot-update mode).
//!
//! See docs/planning/STEP-19-compatibility-tests.md, item 19.6.

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

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn test_config(log_name: &str) -> ResolvedConfig {
    ResolvedConfig {
        main_email:            None,
        main_password:         None,
        monitor_email:         None,
        monitor_password:      None,
        server_bind:           "127.0.0.1".into(),
        server_port:           0,
        server_https:          false,
        log_level:             None,
        log_file:              PathBuf::from(format!("/tmp/ranchero-{log_name}.log")),
        pidfile:               PathBuf::from(format!("/tmp/ranchero-{log_name}.pid")),
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

/// A `PlayerState` broadcast event with known telemetry values.
///
/// Power=200W, HR=120bpm, speed=36_000_000mm/h (36km/h).
fn known_player_state(athlete_id: i64) -> GameEvent {
    GameEvent::PlayerState {
        athlete_id,
        power_w:       200,
        cadence_u_hz:  80_000,
        speed_mm_h:    36_000_000,
        world_time_ms: 60_000,
        world:         5,
        sport:         0,
        distance:      10_000,
        z:             0.0,
        draft:         0,
        heartrate:     120,
    }
}

/// A `MostRecentState` that matches the `known_player_state` values.
fn known_most_recent_state() -> MostRecentState {
    MostRecentState {
        world_time:       60.0,
        speed:            10.0,     // m/s = 36 km/h
        power:            200.0,
        heartrate:        120,
        cadence:          80,
        draft:            0.0,
        distance:         10_000.0,
        altitude:         100.0,
        lat:              1.23,
        lng:              4.56,
        course_id:        5,
        road_id:          0,
        road_time:        0.0,
        reverse:          false,
        event_subgroup_id: 0,
        group_id:         0,
        time:             0.0,
        event_distance:   0.0,
        grade:            0.0,
    }
}

/// Build a `WebState` with athlete 1001 pre-populated (stats + most_recent_state).
fn state_with_watching_athlete(tx: broadcast::Sender<GameEvent>) -> Arc<WebState> {
    let mut registry = AthleteRegistry::new();
    let ad = registry.upsert(1001, 5, 0, 60.0, 0.0);
    ad.most_recent_state = Some(known_most_recent_state());
    Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx),
    )
}

// ---------------------------------------------------------------------------
// Payload contract tests
// ---------------------------------------------------------------------------

/// Subscribing to `athlete/watching/v2` with `{resources:["stats","state"]}` must
/// produce a v2 payload containing both `stats` and `state`.
#[tokio::test]
#[ignore = "slow: real socket"]
async fn watching_v2_payload_has_stats_and_state() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);
    let state   = state_with_watching_athlete(tx.clone());

    let cfg      = test_config("compat-watching");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": {
            "method": "subscribe",
            "arg": {
                "event":  "athlete/watching/v2",
                "source": "stats",
                "subId":  10,
                "query":  { "resources": ["stats", "state"], "stats": true }
            }
        }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true, "subscribe must succeed; got {r}");

    tx.send(known_player_state(1001)).expect("broadcast");

    let ev = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("event must arrive within 2 s");

    assert_eq!(ev["type"],    "event", "got {ev}");
    assert_eq!(ev["uid"],     10,      "got {ev}");
    assert_eq!(ev["success"], true,    "got {ev}");

    let data = &ev["data"];
    assert_eq!(data["version"], 2, "must be v2 payload; got {ev}");
    assert!(data["stats"].is_object(), "stats resource must be present; got {ev}");
    assert!(data["state"].is_object(), "state resource must be present; got {ev}");

    shutdown.notify_one();
    handle.stop().await;
}

/// Subscribing to `nearby/v2` must produce a v2 payload with `athleteId` and
/// `state` for each athlete whose `PlayerState` passes through the broadcast.
#[tokio::test]
#[ignore = "slow: real socket"]
async fn nearby_v2_payload_has_athlete_id_and_state() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);
    let state   = state_with_watching_athlete(tx.clone());

    let cfg      = test_config("compat-nearby");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": {
            "method": "subscribe",
            "arg": {
                "event":  "nearby/v2",
                "source": "stats",
                "subId":  20,
                "query":  { "resources": ["state"] }
            }
        }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true, "subscribe must succeed; got {r}");

    tx.send(known_player_state(1001)).expect("broadcast");

    let ev = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("event must arrive within 2 s");

    let data = &ev["data"];
    assert_eq!(data["version"],   2,    "must be v2; got {ev}");
    assert_eq!(data["athleteId"], 1001, "athleteId must match; got {ev}");
    assert!(data["state"].is_object(),  "state resource must be present; got {ev}");

    shutdown.notify_one();
    handle.stop().await;
}

/// Subscribing to `groups/v2` must produce a v2 payload that includes the
/// athlete's `state.groupId` field.
#[tokio::test]
#[ignore = "slow: real socket"]
async fn groups_v2_payload_has_state_with_group_id() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);
    let state   = state_with_watching_athlete(tx.clone());

    let cfg      = test_config("compat-groups");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": {
            "method": "subscribe",
            "arg": {
                "event":  "groups/v2",
                "source": "stats",
                "subId":  30,
                "query":  { "resources": ["state"] }
            }
        }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true, "subscribe must succeed; got {r}");

    tx.send(known_player_state(1001)).expect("broadcast");

    let ev = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("event must arrive within 2 s");

    let data = &ev["data"];
    assert_eq!(data["version"],   2,    "must be v2; got {ev}");
    assert!(data["state"].is_object(),  "state resource must be present; got {ev}");
    assert!(data["state"]["groupId"].is_number(), "groupId must be a number; got {ev}");

    shutdown.notify_one();
    handle.stop().await;
}

/// `state.latlng` is a `[lat, lng]` two-element array matching sauce4zwift's
/// `_formatState` output.  Separate scalar `lat`/`lng` fields must not appear.
/// This was the deviation documented by 19.6; 19.7 resolves it.
#[tokio::test]
#[ignore = "slow: real socket"]
async fn state_position_is_latlng_array() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);
    let state   = state_with_watching_athlete(tx.clone());

    let cfg      = test_config("compat-latlng");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let url      = format!("ws://{}/api/ws/events", handle.local_addr());
    let mut ws   = connect_async(&url).await.expect("ws connect").0;

    ws_send(&mut ws, json!({
        "type": "request", "uid": 1,
        "data": {
            "method": "subscribe",
            "arg": {
                "event":  "athlete/watching/v2",
                "source": "stats",
                "subId":  40,
                "query":  { "resources": ["state"] }
            }
        }
    })).await;
    let r = ws_recv(&mut ws).await;
    assert_eq!(r["success"], true, "subscribe must succeed; got {r}");

    tx.send(known_player_state(1001)).expect("broadcast");

    let ev = tokio::time::timeout(Duration::from_secs(2), ws_recv(&mut ws))
        .await.expect("event must arrive within 2 s");

    let state_obj = &ev["data"]["state"];
    assert!(state_obj.is_object(), "state must be an object; got {ev}");

    // 19.7: latlng is now a [lat, lng] array matching sauce4zwift.
    let latlng = &state_obj["latlng"];
    assert!(latlng.is_array(), "state.latlng must be a [lat, lng] array; got {state_obj}");
    let arr = latlng.as_array().unwrap();
    assert_eq!(arr.len(), 2, "state.latlng must have 2 elements; got {latlng}");
    assert!(arr[0].is_number(), "latlng[0] (lat) must be a number; got {latlng}");
    assert!(arr[1].is_number(), "latlng[1] (lng) must be a number; got {latlng}");

    // Separate scalar fields must no longer appear.
    assert!(state_obj.get("lat").is_none(), "state.lat must be absent after 19.7; got {state_obj}");
    assert!(state_obj.get("lng").is_none(), "state.lng must be absent after 19.7; got {state_obj}");

    shutdown.notify_one();
    handle.stop().await;
}

// ---------------------------------------------------------------------------
// Headless render tests
// ---------------------------------------------------------------------------

const CHROME_BIN: &str =
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const GOLDEN_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/compat/expected/golden_render",
);

/// Load or create a golden file.  If the file exists, returns its contents.
/// If not, writes `current` and returns it (snapshot-update mode).
fn golden(name: &str, current: &serde_json::Value) -> serde_json::Value {
    let path = format!("{GOLDEN_DIR}/{name}");
    if std::path::Path::new(&path).exists() {
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("read golden {path}: {e}"));
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| panic!("parse golden {path}: {e}"))
    } else {
        std::fs::create_dir_all(GOLDEN_DIR).expect("create golden_render dir");
        let json = serde_json::to_string_pretty(current).expect("serialize");
        std::fs::write(&path, json.as_bytes())
            .unwrap_or_else(|e| panic!("write golden {path}: {e}"));
        println!("  wrote new golden: {path}");
        current.clone()
    }
}

/// Launch Chrome headless in an isolated profile and return the browser +
/// handler task handle.  Each call gets its own temp `user-data-dir` so
/// parallel tests do not collide on Chrome's singleton lock.
///
/// The returned `TempDir` must be kept alive until after `browser.close()`.
async fn launch_chrome() -> (chromiumoxide::Browser, tokio::task::JoinHandle<()>, tempfile::TempDir) {
    use chromiumoxide::BrowserConfig;
    use futures_util::StreamExt as _;

    let profile = tempfile::tempdir().expect("create chrome profile dir");
    let cfg = BrowserConfig::builder()
        .chrome_executable(CHROME_BIN)
        .no_sandbox()
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .arg("--disable-extensions")
        .arg("--disable-background-networking")
        .build()
        .expect("build Chrome config");

    let (browser, mut handler) = chromiumoxide::Browser::launch(cfg)
        .await
        .expect("launch Chrome — is Google Chrome installed at the expected path?");

    let task = tokio::spawn(async move {
        while handler.next().await.is_some() {}
    });

    (browser, task, profile)
}

/// Navigate to `url`, wait for the WebSocket to connect and subscribe, send
/// a `GameEvent`, wait for the page to update, then extract the `textContent`
/// of each CSS selector in `selectors`.  Returns a JSON object keyed by selector.
async fn capture_render(
    page:      &chromiumoxide::Page,
    tx:        &broadcast::Sender<GameEvent>,
    athlete_id: i64,
    selectors:  &[&str],
) -> serde_json::Value {
    // Allow the page's JS module to load and the WebSocket to connect.
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Push a known PlayerState so the page receives data.
    tx.send(known_player_state(athlete_id)).expect("broadcast");

    // Allow time for the WebSocket event to propagate and the DOM to update.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut map = serde_json::Map::new();
    for &sel in selectors {
        let js = format!(
            "(() => {{ \
                const el = document.querySelector({:?}); \
                return el ? el.textContent.trim() : '(not found)'; \
            }})()",
            sel
        );
        let text: String = page
            .evaluate(js)
            .await
            .expect("evaluate JS")
            .into_value()
            .unwrap_or_else(|_| "(eval error)".to_string());
        map.insert(sel.to_string(), serde_json::Value::String(text));
    }
    serde_json::Value::Object(map)
}

/// Headless render of all four widget pages in a single Chrome instance.
///
/// Chrome's process-singleton lock prevents parallel instances; running all
/// page checks sequentially inside one test avoids the conflict.  Each page
/// is verified against a golden snapshot in `compat/expected/golden_render/`;
/// if the golden does not yet exist the test writes it (snapshot-update mode).
///
/// Pages verified:
/// - **watching.html** — power / HR / speed elements updated by `athlete/watching/v2`
/// - **nearby.html**  — first table row's power cell updated by `nearby/v2`
/// - **groups.html**  — first group section heading updated by `groups/v2`
/// - **map.html**     — canvas element present (position data deferred to 19.7)
#[tokio::test]
#[ignore = "slow: headless browser render"]
async fn all_widget_pages_render_correctly() {
    let (tx, _) = broadcast::channel::<GameEvent>(16);
    let state   = state_with_watching_athlete(tx.clone());

    let cfg      = test_config("compat-render-all");
    let shutdown = Arc::new(Notify::new());
    let handle   = start(&cfg, state, shutdown.clone()).await.expect("server must start");
    let addr     = handle.local_addr();

    let (mut browser, handler_task, _profile) = launch_chrome().await;

    // --- watching.html ---
    {
        let page = browser.new_page(format!("http://{addr}/pages/watching.html"))
            .await.expect("open watching page");
        let actual = capture_render(
            &page, &tx, 1001,
            &["#power .val", "#hr .val", "#speed .val"],
        ).await;
        let expected = golden("watching.json", &actual);
        assert_eq!(actual, expected, "watching page render must match golden");
        drop(page);
    }

    // --- nearby.html ---
    {
        let page = browser.new_page(format!("http://{addr}/pages/nearby.html"))
            .await.expect("open nearby page");
        let actual = capture_render(
            &page, &tx, 1001,
            &["#nearby tbody tr:first-child td:nth-child(2)"],
        ).await;
        let expected = golden("nearby.json", &actual);
        assert_eq!(actual, expected, "nearby page render must match golden");
        drop(page);
    }

    // --- groups.html ---
    {
        let page = browser.new_page(format!("http://{addr}/pages/groups.html"))
            .await.expect("open groups page");
        let actual = capture_render(
            &page, &tx, 1001,
            &["#groups section:first-child h2"],
        ).await;
        let expected = golden("groups.json", &actual);
        assert_eq!(actual, expected, "groups page render must match golden");
        drop(page);
    }

    // --- map.html — canvas presence (position rendering deferred to 19.7) ---
    {
        let page = browser.new_page(format!("http://{addr}/pages/map.html"))
            .await.expect("open map page");
        tokio::time::sleep(Duration::from_millis(500)).await;
        let canvas_count: i64 = page
            .evaluate("document.querySelectorAll('canvas').length")
            .await.expect("evaluate canvas count")
            .into_value()
            .expect("canvas count i64");
        assert!(canvas_count >= 1, "map page must contain at least one canvas element");
        let actual = serde_json::json!({"canvas_count": canvas_count});
        golden("map.json", &actual);  // write golden, no comparison needed beyond count
        drop(page);
    }

    browser.close().await.expect("close Chrome");
    handler_task.await.expect("handler task");
    shutdown.notify_one();
    handle.stop().await;
}
