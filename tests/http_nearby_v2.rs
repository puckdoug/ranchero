// SPDX-License-Identifier: AGPL-3.0-only
//! 17.11-T (nearby) — GET /api/nearby/v2 returns a JSON array of nearby
//! athletes. With no query, each entry carries version: 2 and the v1
//! base fields. With ?resource=stats, each entry contains only the stats
//! resource field.
//!
//! Fails at runtime until `configure_api` registers /api/nearby/v2.
//! No real socket; no slow marker.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.11-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_api, AthleteRegistry, WebState};

fn two_athlete_state() -> web::Data<Arc<WebState>> {
    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);
    registry.upsert(1002, 5, 0, 0.0, 0.0);
    web::Data::new(Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
    ))
}

#[actix_web::test]
async fn nearby_v2_no_query_returns_entries_with_version_field() {
    let state = two_athlete_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get().uri("/api/nearby/v2").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let arr = body.as_array().expect("nearby/v2 must return a JSON array");
    assert_eq!(arr.len(), 2, "both registered athletes must appear");

    for entry in arr {
        assert_eq!(entry["version"], 2,
            "each nearby/v2 entry must carry version: 2; got {entry}");
        assert!(entry["athleteId"].is_number(),
            "each nearby/v2 entry must carry athleteId; got {entry}");
    }
}

#[actix_web::test]
async fn nearby_v2_resource_filter_applied_per_entry() {
    let state = two_athlete_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/nearby/v2?resource=stats")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let arr = body.as_array().expect("nearby/v2 must return a JSON array");
    assert_eq!(arr.len(), 2);

    for entry in arr {
        let obj = entry.as_object()
            .expect("each nearby/v2 entry must be a JSON object");
        assert!(obj.contains_key("stats"),
            "requested resource 'stats' must be present; got {entry}");
        assert!(!obj.contains_key("athleteId"),
            "athleteId must be absent when resource filter is active; got {entry}");
    }
}
