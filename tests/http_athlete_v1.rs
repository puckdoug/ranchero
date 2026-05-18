// SPDX-License-Identifier: AGPL-3.0-only
//! 17.6-T — GET /api/athlete/v1/:id returns the athlete record for a
//! present athlete; returns 404 for a missing one; accepts `self`,
//! `watching`, or a numeric id; emits JSON whose top-level field names
//! match `_formatAthleteData` in sauce4zwift `stats.mjs:2050+`.
//!
//! Fails to compile until:
//!   - `ranchero::web::AthleteRegistry` is re-exported (requires
//!     `zwift-stats` to become a dependency of the root crate)
//!   - `WebState::with_registry` exists
//!   - `ranchero::web::http::configure_api` exists
//!
//! No real socket; no slow marker.
//!
//! See `docs/plans/STEP-17-web-server.md`, item 17.6-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_api, AthleteRegistry, WebState};

/// Seed athlete 1001 as both the watched and self athlete.
fn seeded_state() -> web::Data<Arc<WebState>> {
    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);
    web::Data::new(Arc::new(WebState::with_registry(
        registry,
        Some(1001), // watching
        Some(1001), // self (logged-in user's athlete id)
    )))
}

#[actix_web::test]
async fn athlete_v1_returns_200_with_required_fields() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v1/1001")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["athleteId"], 1001);
    assert_eq!(body["courseId"], 5);
    assert!(body["lapCount"].is_number());
    assert!(body["stats"].is_object());
    assert!(body["lap"].is_object());
}

#[actix_web::test]
async fn athlete_v1_returns_404_for_unknown_id() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v1/9999")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn athlete_v1_watching_alias_resolves_to_watched_athlete() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v1/watching")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["athleteId"], 1001);
    // The watching flag must be true because athlete 1001 is the
    // watched athlete.
    assert_eq!(body["watching"], true);
}

#[actix_web::test]
async fn athlete_v1_self_alias_resolves_to_logged_in_athlete() {
    let state = seeded_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/athlete/v1/self")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    // The self flag must be true because the logged-in user's athlete
    // id matches athlete 1001.
    assert_eq!(body["self"], true);
}
