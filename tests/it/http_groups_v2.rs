// SPDX-License-Identifier: AGPL-3.0-only
//! 17.11-T (groups) — GET /api/groups/v2 returns a JSON array of groups.
//! With no query, each group's athlete entries carry version: 2 and the
//! v1 base fields. With ?resource=stats, each athlete entry contains only
//! the stats resource field.
//!
//! Fails at runtime until `configure_api` registers /api/groups/v2.
//! No real socket; no slow marker.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.11-T.

use std::sync::Arc;
use actix_web::{http::StatusCode, test, web, App};
use ranchero::web::{http::configure_api, AthleteRegistry, WebState};

fn grouped_state() -> web::Data<Arc<WebState>> {
    let mut registry = AthleteRegistry::new();
    {
        let ad = registry.upsert(1001, 5, 0, 0.0, 0.0);
        ad.group_id = Some(1);
    }
    {
        let ad = registry.upsert(1002, 5, 0, 0.0, 0.0);
        ad.group_id = Some(1);
    }
    registry.touch_group(1, 0.0);
    web::Data::new(Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
    ))
}

#[actix_web::test]
async fn groups_v2_athletes_have_version_field() {
    let state = grouped_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get().uri("/api/groups/v2").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let groups = body.as_array().expect("groups/v2 must return a JSON array");
    assert!(!groups.is_empty(), "at least one group must appear");

    for group in groups {
        let athletes = group["athletes"].as_array()
            .expect("each group must have an 'athletes' array; got {group}");
        for entry in athletes {
            assert_eq!(entry["version"], 2,
                "each group athlete entry must carry version: 2; got {entry}");
            assert!(entry["athleteId"].is_number(),
                "each group athlete entry must carry athleteId; got {entry}");
        }
    }
}

#[actix_web::test]
async fn groups_v2_resource_filter_applies_to_athletes() {
    let state = grouped_state();
    let app = test::init_service(
        App::new().app_data(state).configure(configure_api)
    ).await;

    let req = test::TestRequest::get()
        .uri("/api/groups/v2?resource=stats")
        .to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = test::read_body_json(resp).await;
    let groups = body.as_array().expect("groups/v2 must return a JSON array");
    assert!(!groups.is_empty());

    for group in groups {
        let athletes = group["athletes"].as_array()
            .expect("each group must have an 'athletes' array");
        for entry in athletes {
            let obj = entry.as_object()
                .expect("each athlete entry must be a JSON object");
            assert!(obj.contains_key("stats"),
                "requested resource 'stats' must be present; got {entry}");
            // athleteId is part of the v2 base object and is always present,
            // even when resources are requested (18.9-I corrected v2 behavior).
            assert!(obj.contains_key("athleteId"),
                "athleteId must be present as part of the v2 base object; got {entry}");
            assert_eq!(entry["version"], 2,
                "version: 2 must be present in the v2 base object; got {entry}");
        }
    }
}
