// SPDX-License-Identifier: AGPL-3.0-only
//! 17.10-T — GET /api/athlete/v2/:id?resource=stats&resource=lap returns
//! only the requested resource fields; omitting the query returns the
//! v1 shape with a version: 2 field added; unknown id returns 404.
//!
//! Fails at runtime (not compile time) until `configure_api` registers
//! /api/athlete/v2/:id. No real socket; no slow marker.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.10-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_api, AthleteRegistry, WebState};

fn seeded_state() -> web::Data<Arc<WebState>> {
    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);
    web::Data::new(Arc::new(WebState::with_registry(
        registry,
        Some(1001),
        Some(1001),
    )))
}

#[actix_web::test]
async fn athlete_v2_no_query_returns_base_with_version() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;

    assert_eq!(body["version"], 2, "v2 response must carry version: 2");
    assert_eq!(body["athleteId"], 1001);
    assert_eq!(body["courseId"], 5);
    assert!(body["lapCount"].is_number());
    assert!(body["createdServerTime"].is_number());
}

#[actix_web::test]
async fn athlete_v2_returns_404_for_unknown_id() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/9999")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn athlete_v2_resource_filter_returns_requested_keys_plus_base() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=stats&resource=lap")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let obj = body.as_object()
        .expect("filtered v2 response must be a JSON object");

    assert!(obj.contains_key("stats"),     "requested resource 'stats' must be present");
    assert!(obj.contains_key("lap"),       "requested resource 'lap' must be present");
    assert!(obj.contains_key("athleteId"), "base field athleteId must always be present");
    assert_eq!(body["version"], 2);
}

#[actix_web::test]
async fn athlete_v2_resource_filter_excludes_unrequested_keys() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    // Request stats but not lap.
    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=stats")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let obj = body.as_object()
        .expect("filtered v2 response must be a JSON object");

    assert!(obj.contains_key("stats"), "requested resource 'stats' must be present");
    assert!(!obj.contains_key("lap"),
        "resource 'lap' was not requested and must be absent");
}

// ---------------------------------------------------------------------------
// 18.9-T — v2 athlete formatter: base object + resource fields
// ---------------------------------------------------------------------------

/// No-query v2 request returns the base object WITHOUT stats or lap.
/// stats and lap are resources; they must be explicitly requested.
///
/// Fails until 18.9-I replaces the stub: the current stub falls back to the
/// v1 shape (which always includes stats and lap) when no resources are given.
#[actix_web::test]
async fn v2_no_query_returns_base_object_not_v1_shape() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let obj = body.as_object().expect("response must be a JSON object");

    assert_eq!(body["version"], 2);
    assert_eq!(body["athleteId"], 1001);
    assert!(body["createdServerTime"].is_number());
    assert!(body["lapCount"].is_number());

    assert!(!obj.contains_key("stats"),
        "stats is a resource; must be absent from no-query v2 response");
    assert!(!obj.contains_key("lap"),
        "lap is a resource; must be absent from no-query v2 response");
}

/// When resources are specified, the response includes the base object PLUS
/// the requested resource fields.
///
/// Fails until 18.9-I: the current stub returns ONLY the resource keys,
/// omitting all base fields (athleteId, version, lapCount, etc.).
#[actix_web::test]
async fn v2_resource_response_includes_base_object_fields() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=stats")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;

    assert_eq!(body["version"], 2);
    assert_eq!(body["athleteId"], 1001);
    assert!(body["lapCount"].is_number());
    assert!(body["stats"].is_object(), "requested resource stats must be present");
}

/// When ?resource=stats is requested, power.peaks is an array (v2 shape),
/// not a period-keyed object (v1 shape).
///
/// Fails until 18.8-I and 18.9-I are implemented: the current stub returns
/// stats: {} for any resource request.
#[actix_web::test]
async fn v2_stats_resource_peaks_are_arrays() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=stats")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;

    let stats = &body["stats"];
    assert!(stats.is_object());
    assert!(stats["power"]["peaks"].is_array(),
        "v2 stats power.peaks must be an array, not a period-keyed object");
    assert!(stats["power"]["smooth"].is_array(),
        "v2 stats power.smooth must be an array of {{period, avg}} objects");
}

/// Unknown resource names are ignored (not rejected), and the response still
/// contains all base object fields.
///
/// Fails until 18.9-I: the current stub returns {} for an unknown resource
/// name, so base fields like athleteId are absent.
#[actix_web::test]
async fn v2_unknown_resource_is_ignored_base_still_present() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=no_such_field")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;

    assert_eq!(body["version"], 2);
    assert_eq!(body["athleteId"], 1001);
    assert!(body.get("no_such_field").is_none(),
        "unknown resource names must not produce a key in the response");
}

// ---------------------------------------------------------------------------
// 18.9b-T — lastLap resource writes to lastLap key, not lap (decision D1)
// ---------------------------------------------------------------------------

/// ?resource=lastLap must produce a `lastLap` key, not a `lap` key.
///
/// The JS bug (`stats.mjs:4376`) writes the lastLap value into `data.lap`.
/// The Rust port deviates intentionally: lastLap resource writes to
/// `data.lastLap` (decision D1).
///
/// Fails until 18.9-I: the current stub returns ONLY the resource key
/// without base fields, so assertions on athleteId and version fail.
#[actix_web::test]
async fn v2_last_lap_resource_populates_last_lap_key() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=lastLap")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let obj = body.as_object().expect("response must be a JSON object");

    assert!(obj.contains_key("lastLap"),
        "lastLap resource must produce a lastLap key");
    assert!(!obj.contains_key("lap"),
        "lastLap resource must not produce a lap key (D1: JS bug fix)");

    assert_eq!(body["version"], 2);
    assert_eq!(body["athleteId"], 1001);
}

/// Requesting both lap and lastLap yields two independent keys in the response.
///
/// Fails until 18.9-I: the current stub returns ONLY resource keys without
/// base fields, so assertions on version and athleteId fail.
#[actix_web::test]
async fn v2_lap_and_last_lap_resources_produce_independent_keys() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=lap&resource=lastLap")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let obj = body.as_object().expect("response must be a JSON object");

    assert!(obj.contains_key("lap"),     "lap resource must produce a lap key");
    assert!(obj.contains_key("lastLap"), "lastLap resource must produce a lastLap key");

    assert_eq!(body["version"], 2);
    assert_eq!(body["athleteId"], 1001);
}

// ---------------------------------------------------------------------------
// 18.14-T — Phase 7: laps/segments/events resource parity
//
// The reference `_formatAthleteDataV2` (stats.mjs:4378-4384) maps
// `laps`/`segments`/`events` resources to per-slice-formatter arrays, not
// the empty stubs ranchero currently emits.  The `stats=true` query parameter
// (parsed by `parseAthleteDataV2Query`, webserver.mjs:278) controls whether
// each slice's `stats` field is a v2 stats object (true) or null (absent/false).
//
// The seeded athlete has one initial open lap slice and no segments or events.
//
// All three tests below fail at runtime until 18.14-I wires the slice
// formatters into `format_athlete_v2` and parses the `stats` query param.
// ---------------------------------------------------------------------------

/// `?resource=laps` must return an array of formatted slice objects, one entry
/// per lap slice.  The seeded athlete has one initial open slice; the current
/// stub returns `laps: []` so the length assertion fails.
#[actix_web::test]
async fn v2_laps_resource_returns_slice_array() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=laps")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let laps = body["laps"].as_array()
        .expect("laps resource must produce a JSON array");

    assert_eq!(laps.len(), 1,
        "seeded athlete has one initial lap slice; stub returns [] so this fails");
    assert_eq!(laps[0]["active"], true,   "initial lap slice is open");
    assert!(laps[0]["courseId"].is_number(), "slice must carry courseId");
}

/// Without `?stats=true`, each slice in the `laps` array must carry `stats: null`
/// (matching `_formatDataSlice` with `version:2, stats:false`, stats.mjs:1750).
/// Fails now because the stub returns `laps: []` — no elements to inspect.
#[actix_web::test]
async fn v2_laps_resource_slice_stats_null_without_stats_param() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=laps")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let laps = body["laps"].as_array()
        .expect("laps resource must produce a JSON array");

    assert_eq!(laps.len(), 1, "seeded athlete has one lap slice");
    assert!(laps[0]["stats"].is_null(),
        "slice stats must be null when ?stats=true is not set (v2 slice shape)");
}

/// With `?stats=true`, each slice in the `laps` array must carry a v2-shaped
/// stats object (peaks are arrays).  Fails now because the stub returns `laps: []`
/// and the `stats` query parameter is not parsed.
#[actix_web::test]
async fn v2_laps_resource_slice_has_v2_stats_with_stats_param() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v2/1001?resource=laps&stats=true")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let laps = body["laps"].as_array()
        .expect("laps resource must produce a JSON array");

    assert_eq!(laps.len(), 1, "seeded athlete has one lap slice");
    assert!(laps[0]["stats"].is_object(),
        "slice stats must be a v2 stats object when ?stats=true is set");
    assert!(laps[0]["stats"]["power"]["peaks"].is_array(),
        "v2 slice stats power.peaks must be an array");
}
