// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::Arc;

use actix_cors::Cors;
use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::{header, StatusCode};
use actix_web::middleware::{from_fn, Next};
use actix_web::{web, Error, HttpResponse};
use serde_json::json;
use zwift_stats::AthleteData;

use crate::web::state::WebState;

// ---------------------------------------------------------------------------
// Preflight middleware
// ---------------------------------------------------------------------------

// actix-cors 0.7 returns 200 for preflight; this outer layer converts it to
// 204 and ensures Access-Control-Allow-Headers: * is present even when the
// request omits Access-Control-Request-Headers.
async fn fix_preflight(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let is_preflight = req.method() == actix_web::http::Method::OPTIONS
        && req.headers().contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
    let mut res = next.call(req).await?;
    if is_preflight && res.status() == StatusCode::OK {
        *res.response_mut().status_mut() = StatusCode::NO_CONTENT;
        res.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            header::HeaderValue::from_static("*"),
        );
    }
    Ok(res)
}

// ---------------------------------------------------------------------------
// API directory
// ---------------------------------------------------------------------------

fn api_directory() -> serde_json::Value {
    json!([{
        "athlete/v1/<id>|self|watching": "[GET] Current data for an athlete in the game",
        "athlete/v2/<id>|self|watching[?resource=RES1][&resource=...RESN][&stats=true]":
            "[GET] Current data for an athlete in the game.\n   ?resource: stats|state|athlete|lap|lastLap|laps|segments|events|timeInPowerZones\n   ?stats: Include extended statistics for applicable resources",
        "athlete/laps/v1/<id>|self|watching": "[GET] Lap data for an athlete",
        "athlete/segments/v1/<id>|self|watching": "[GET] Segments data for an athlete",
        "athlete/events/v1/<id>|self|watching": "[GET] Events data for an athlete",
        "athlete/streams/v1/<id>|self|watching": "[GET] Stream data (power, cadence, etc..) for an athlete",
        "nearby/v1": "[GET] Information for all nearby athletes",
        "nearby/v2[?resource=RES1][&resource=...RESN][&stats=true]":
            "[GET] Information for all nearby athletes\n   ?resource: stats|state|athlete|lap|lastLap|laps|segments|events|timeInPowerZones\n   ?stats: Include extended statistics for applicable resources",
        "groups/v1": "[GET] Information for all nearby groups",
        "groups/v2[?resource=RES1][&resource=...RESN][&stats=true]":
            "[GET] Information for all nearby groups\n   ?resource: stats|state|athlete|lap|lastLap|laps|segments|events|timeInPowerZones\n   ?stats: Include extended statistics for applicable resources",
        "rpc/v1": "[GET] List available RPC resources",
        "rpc/v1/<name>": "[POST] Make an RPC to the backend.\n    Content body should be JSON array of arguments",
        "rpc/v1/<name>[/<arg1>][.../<argN>]": "[GET] Simple RPC to the backend.\n    CAUTION: Types are inferred based on value.  Values of null, undefined, true, false,\n    NaN, Infinity and -Infinity are converted to their native JavaScript counterpart.\n    Number-like values are converted to the native number type.  For advanced call patterns\n    use the POST method or the v2 endpoint.",
        "rpc/v2/<name>[/<base64url_arg1>][.../<base64url_argN>]": "[GET] Make an RPC to the backend.\n    URL components following the name should be Base64[URL] encoded JSON representing each\n    RPC argument.",
        "mods/v1": "[GET] List available mods (i.e. plugins)"
    }])
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn api_root_handler() -> HttpResponse {
    HttpResponse::Ok().json(api_directory())
}

fn format_athlete(
    athlete: &AthleteData,
    watching_id: Option<u32>,
    self_athlete_id: Option<u32>,
) -> serde_json::Value {
    let mut obj = json!({
        "athleteId": athlete.athlete_id,
        "courseId":  athlete.course_id,
        "lapCount":  athlete.lap_slices.len() as u32,
        "stats":     {},
        "lap":       {},
    });
    if watching_id == Some(athlete.athlete_id) {
        obj["watching"] = json!(true);
    }
    if self_athlete_id == Some(athlete.athlete_id) {
        obj["self"] = json!(true);
    }
    obj
}

async fn athlete_v1_handler(
    state: web::Data<Arc<WebState>>,
    path:  web::Path<String>,
) -> HttpResponse {
    let id_str = path.into_inner();
    let athlete_id = match id_str.as_str() {
        "watching" => match state.watching_id {
            Some(id) => id,
            None     => return HttpResponse::NotFound().finish(),
        },
        "self" => match state.self_athlete_id {
            Some(id) => id,
            None     => return HttpResponse::NotFound().finish(),
        },
        s => match s.parse::<u32>() {
            Ok(id)  => id,
            Err(_)  => return HttpResponse::NotFound().finish(),
        },
    };

    let registry = state.registry.read().unwrap();
    match registry.get(athlete_id) {
        Some(athlete) => HttpResponse::Ok()
            .json(format_athlete(athlete, state.watching_id, state.self_athlete_id)),
        None => HttpResponse::NotFound().finish(),
    }
}

async fn nearby_v1_handler(state: web::Data<Arc<WebState>>) -> HttpResponse {
    let registry = state.registry.read().unwrap();
    let body: Vec<serde_json::Value> = registry
        .iter()
        .map(|(_, a)| format_athlete(a, state.watching_id, state.self_athlete_id))
        .collect();
    HttpResponse::Ok().json(body)
}

async fn groups_v1_handler(state: web::Data<Arc<WebState>>) -> HttpResponse {
    let registry = state.registry.read().unwrap();

    let mut by_group: HashMap<u32, Vec<serde_json::Value>> = HashMap::new();
    for (_, athlete) in registry.iter() {
        if let Some(gid) = athlete.group_id {
            by_group
                .entry(gid)
                .or_default()
                .push(format_athlete(athlete, state.watching_id, state.self_athlete_id));
        }
    }

    let body: Vec<serde_json::Value> = by_group
        .into_values()
        .map(|athletes| json!({ "athletes": athletes }))
        .collect();
    HttpResponse::Ok().json(body)
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn configure_api(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .wrap(
                Cors::default()
                    .allow_any_origin()
                    .allow_any_method()
                    .allow_any_header()
                    .send_wildcard(),
            )
            .wrap(from_fn(fix_preflight))
            .route("/", web::get().to(api_root_handler))
            .route("/athlete/v1/{id}", web::get().to(athlete_v1_handler))
            .route("/nearby/v1", web::get().to(nearby_v1_handler))
            .route("/groups/v1", web::get().to(groups_v1_handler))
            .default_service(web::route().to(api_root_handler)),
    );
}
