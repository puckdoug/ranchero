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
async fn athlete_v2_no_query_returns_v1_shape_with_version() {
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
    assert!(body["stats"].is_object(),
        "no-query v2 response must include stats");
    assert!(body["lap"].is_object(),
        "no-query v2 response must include lap");
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
async fn athlete_v2_resource_filter_returns_only_requested_keys() {
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

    assert!(obj.contains_key("stats"), "requested resource 'stats' must be present");
    assert!(obj.contains_key("lap"),   "requested resource 'lap' must be present");
    assert!(!obj.contains_key("athleteId"),
        "athleteId is not a resource key and must be absent when using resource filter");
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
