// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use actix_cors::Cors;
use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::{header, StatusCode};
use actix_web::middleware::{DefaultHeaders, from_fn, Next};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use actix_files::NamedFile;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{json, Value};
use zwift_stats::AthleteData;

use zwift_relay::ZWIFT_EPOCH_MS;
use crate::web::format::{
    format_athlete_data_v1, format_athlete_v2,
    format_data_slice, format_segment_slice, format_event_slice, filter_slices,
    format_athlete_streams,
};
use crate::web::state::WebState;
use crate::web::ws;

fn local_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

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
// Query helpers
// ---------------------------------------------------------------------------

fn parse_resources(query: &str) -> Vec<String> {
    serde_urlencoded::from_str::<Vec<(String, String)>>(query)
        .unwrap_or_default()
        .into_iter()
        .filter(|(k, _)| k == "resource")
        .map(|(_, v)| v)
        .collect()
}

/// Parse the `stats` query parameter, mirroring `parseAthleteDataV2Query`
/// (`webserver.mjs:280`): numeric values are truthy when non-zero; string
/// values are truthy only when they equal `"true"` (case-insensitive).
fn parse_stats(query: &str) -> bool {
    serde_urlencoded::from_str::<Vec<(String, String)>>(query)
        .unwrap_or_default()
        .into_iter()
        .find(|(k, _)| k == "stats")
        .map(|(_, v)| {
            if let Ok(n) = v.parse::<f64>() { n != 0.0 } else { v.to_lowercase() == "true" }
        })
        .unwrap_or(false)
}

fn resolve_athlete_id(id_str: &str, state: &WebState) -> Option<u32> {
    match id_str {
        "watching" => state.watching_id,
        "self"     => state.self_athlete_id,
        s          => s.parse::<u32>().ok(),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn api_root_handler() -> HttpResponse {
    HttpResponse::Ok().json(api_directory())
}

async fn athlete_v1_handler(
    state: web::Data<Arc<WebState>>,
    path:  web::Path<String>,
) -> HttpResponse {
    let athlete_id = match resolve_athlete_id(&path.into_inner(), &state) {
        Some(id) => id,
        None     => return HttpResponse::NotFound().finish(),
    };
    let registry = state.registry.read().unwrap();
    match registry.get(athlete_id) {
        Some(athlete) => {
            let now   = local_now();
            let ts_ms = athlete.wt_offset * 1000.0 + ZWIFT_EPOCH_MS as f64;
            HttpResponse::Ok()
                .json(format_athlete_data_v1(athlete, state.watching_id, state.self_athlete_id,
                                             None, now, ts_ms))
        }
        None => HttpResponse::NotFound().finish(),
    }
}

async fn athlete_v2_handler(
    state: web::Data<Arc<WebState>>,
    path:  web::Path<String>,
    req:   HttpRequest,
) -> HttpResponse {
    let athlete_id = match resolve_athlete_id(&path.into_inner(), &state) {
        Some(id) => id,
        None     => return HttpResponse::NotFound().finish(),
    };
    let resources = parse_resources(req.query_string());
    let stats     = parse_stats(req.query_string());
    let registry  = state.registry.read().unwrap();
    match registry.get(athlete_id) {
        Some(athlete) => HttpResponse::Ok()
            .json(format_athlete_v2(athlete, &resources, state.watching_id, state.self_athlete_id, stats)),
        None => HttpResponse::NotFound().finish(),
    }
}

async fn laps_v1_handler(
    state: web::Data<Arc<WebState>>,
    path:  web::Path<String>,
) -> HttpResponse {
    let athlete_id = match resolve_athlete_id(&path.into_inner(), &state) {
        Some(id) => id,
        None     => return HttpResponse::NotFound().finish(),
    };
    let registry = state.registry.read().unwrap();
    match registry.get(athlete_id) {
        Some(athlete) => {
            let slices: Vec<Value> = filter_slices(&athlete.lap_slices, None, None, true)
                .into_iter()
                .map(|s| format_data_slice(s, athlete))
                .collect();
            HttpResponse::Ok().json(slices)
        }
        None => HttpResponse::NotFound().finish(),
    }
}

async fn segments_v1_handler(
    state: web::Data<Arc<WebState>>,
    path:  web::Path<String>,
) -> HttpResponse {
    let athlete_id = match resolve_athlete_id(&path.into_inner(), &state) {
        Some(id) => id,
        None     => return HttpResponse::NotFound().finish(),
    };
    let registry = state.registry.read().unwrap();
    match registry.get(athlete_id) {
        Some(athlete) => {
            let slices: Vec<Value> = filter_slices(&athlete.segment_slices, None, None, true)
                .into_iter()
                .map(|s| format_segment_slice(s, athlete))
                .collect();
            HttpResponse::Ok().json(slices)
        }
        None => HttpResponse::NotFound().finish(),
    }
}

async fn events_v1_handler(
    state: web::Data<Arc<WebState>>,
    path:  web::Path<String>,
) -> HttpResponse {
    let athlete_id = match resolve_athlete_id(&path.into_inner(), &state) {
        Some(id) => id,
        None     => return HttpResponse::NotFound().finish(),
    };
    let registry = state.registry.read().unwrap();
    match registry.get(athlete_id) {
        Some(athlete) => {
            let slices: Vec<Value> = filter_slices(&athlete.event_slices, None, None, true)
                .into_iter()
                .map(|s| format_event_slice(s, athlete))
                .collect();
            HttpResponse::Ok().json(slices)
        }
        None => HttpResponse::NotFound().finish(),
    }
}

async fn streams_v1_handler(
    state: web::Data<Arc<WebState>>,
    path:  web::Path<String>,
) -> HttpResponse {
    let athlete_id = match resolve_athlete_id(&path.into_inner(), &state) {
        Some(id) => id,
        None     => return HttpResponse::NotFound().finish(),
    };
    let registry = state.registry.read().unwrap();
    match registry.get(athlete_id) {
        Some(athlete) => HttpResponse::Ok().json(format_athlete_streams(athlete)),
        None          => HttpResponse::NotFound().finish(),
    }
}

async fn nearby_v1_handler(state: web::Data<Arc<WebState>>) -> HttpResponse {
    let now = local_now();
    let registry = state.registry.read().unwrap();
    let body: Vec<serde_json::Value> = registry
        .iter()
        .map(|(_, a)| {
            let ts_ms = a.wt_offset * 1000.0 + ZWIFT_EPOCH_MS as f64;
            format_athlete_data_v1(a, state.watching_id, state.self_athlete_id, None, now, ts_ms)
        })
        .collect();
    HttpResponse::Ok().json(body)
}

async fn nearby_v2_handler(
    state: web::Data<Arc<WebState>>,
    req:   HttpRequest,
) -> HttpResponse {
    let resources = parse_resources(req.query_string());
    let stats     = parse_stats(req.query_string());
    let registry  = state.registry.read().unwrap();
    let body: Vec<serde_json::Value> = registry
        .iter()
        .map(|(_, a)| format_athlete_v2(a, &resources, state.watching_id, state.self_athlete_id, stats))
        .collect();
    HttpResponse::Ok().json(body)
}

async fn groups_v1_handler(state: web::Data<Arc<WebState>>) -> HttpResponse {
    let now = local_now();
    let registry = state.registry.read().unwrap();
    let body = group_athletes(&registry, |a| {
        let ts_ms = a.wt_offset * 1000.0 + ZWIFT_EPOCH_MS as f64;
        format_athlete_data_v1(a, state.watching_id, state.self_athlete_id, None, now, ts_ms)
    });
    HttpResponse::Ok().json(body)
}

async fn groups_v2_handler(
    state: web::Data<Arc<WebState>>,
    req:   HttpRequest,
) -> HttpResponse {
    let resources = parse_resources(req.query_string());
    let stats     = parse_stats(req.query_string());
    let registry  = state.registry.read().unwrap();
    let body = group_athletes(&registry, |a| {
        format_athlete_v2(a, &resources, state.watching_id, state.self_athlete_id, stats)
    });
    HttpResponse::Ok().json(body)
}

fn group_athletes<F>(registry: &zwift_stats::AthleteRegistry, fmt: F) -> Vec<serde_json::Value>
where
    F: Fn(&AthleteData) -> serde_json::Value,
{
    let mut by_group: HashMap<u32, Vec<serde_json::Value>> = HashMap::new();
    for (_, athlete) in registry.iter() {
        if let Some(gid) = athlete.group_id {
            by_group.entry(gid).or_default().push(fmt(athlete));
        }
    }
    by_group
        .into_values()
        .map(|athletes| json!({ "athletes": athletes }))
        .collect()
}

// ---------------------------------------------------------------------------
// RPC helpers
// ---------------------------------------------------------------------------

fn coerce_arg(s: &str) -> Value {
    match s {
        "null" | "undefined" => Value::Null,
        "true"               => Value::Bool(true),
        "false"              => Value::Bool(false),
        // JSON has no NaN or ±Infinity; map them to null.
        "NaN" | "Infinity" | "+Infinity" | "-Infinity" => Value::Null,
        _ => {
            if let Ok(n) = s.parse::<i64>() {
                return json!(n);
            }
            if let Ok(n) = s.parse::<f64>() {
                if let Some(num) = serde_json::Number::from_f64(n) {
                    return Value::Number(num);
                }
            }
            Value::String(s.to_string())
        }
    }
}

fn rpc_ok(data: Value) -> HttpResponse {
    HttpResponse::Ok().json(json!({"success": true, "data": data}))
}

fn rpc_err(error: &str) -> HttpResponse {
    HttpResponse::Ok().json(json!({"success": false, "error": error}))
}

// ---------------------------------------------------------------------------
// RPC handlers
// ---------------------------------------------------------------------------

async fn rpc_discovery_handler(state: web::Data<Arc<WebState>>) -> HttpResponse {
    let mut names: Vec<&str> = match &state.rpc {
        Some(rpc) => rpc.names(),
        None      => vec![],
    };
    names.sort_unstable();
    HttpResponse::Ok().json(json!(names))
}

async fn rpc_v1_get_handler(
    state: web::Data<Arc<WebState>>,
    name:  web::Path<String>,
    req:   HttpRequest,
) -> HttpResponse {
    let rpc = match state.rpc.as_ref() {
        Some(r) => r,
        None    => return HttpResponse::NotFound().finish(),
    };
    let args: Vec<Value> = serde_urlencoded::from_str::<Vec<(String, String)>>(req.query_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|(k, _)| k == "arg")
        .map(|(_, v)| coerce_arg(&v))
        .collect();
    match rpc.dispatch(&name.into_inner(), args).await {
        None         => HttpResponse::NotFound().finish(),
        Some(Ok(d))  => rpc_ok(d),
        Some(Err(e)) => rpc_err(&e),
    }
}

async fn rpc_v1_post_handler(
    state: web::Data<Arc<WebState>>,
    name:  web::Path<String>,
    body:  web::Json<Vec<Value>>,
) -> HttpResponse {
    let rpc = match state.rpc.as_ref() {
        Some(r) => r,
        None    => return HttpResponse::NotFound().finish(),
    };
    match rpc.dispatch(&name.into_inner(), body.into_inner()).await {
        None         => HttpResponse::NotFound().finish(),
        Some(Ok(d))  => rpc_ok(d),
        Some(Err(e)) => rpc_err(&e),
    }
}

async fn rpc_v2_handler(
    state: web::Data<Arc<WebState>>,
    tail:  web::Path<String>,
) -> HttpResponse {
    let rpc = match state.rpc.as_ref() {
        Some(r) => r,
        None    => return HttpResponse::NotFound().finish(),
    };
    let tail = tail.into_inner();
    let parts: Vec<&str> = tail.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return HttpResponse::NotFound().finish();
    }
    let name = parts[0];
    let mut args: Vec<Value> = Vec::new();
    for seg in &parts[1..] {
        let decoded = match URL_SAFE_NO_PAD.decode(seg) {
            Ok(b)  => b,
            Err(_) => return HttpResponse::BadRequest().finish(),
        };
        match serde_json::from_slice::<Value>(&decoded) {
            Ok(v)  => args.push(v),
            Err(_) => return HttpResponse::BadRequest().finish(),
        }
    }
    match rpc.dispatch(name, args).await {
        None         => HttpResponse::NotFound().finish(),
        Some(Ok(d))  => rpc_ok(d),
        Some(Err(e)) => rpc_err(&e),
    }
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
            .route("/athlete/v1/{id}",          web::get().to(athlete_v1_handler))
            .route("/athlete/v2/{id}",          web::get().to(athlete_v2_handler))
            .route("/athlete/laps/v1/{id}",     web::get().to(laps_v1_handler))
            .route("/athlete/segments/v1/{id}", web::get().to(segments_v1_handler))
            .route("/athlete/events/v1/{id}",   web::get().to(events_v1_handler))
            .route("/athlete/streams/v1/{id}", web::get().to(streams_v1_handler))
            .route("/nearby/v1",       web::get().to(nearby_v1_handler))
            .route("/nearby/v2",       web::get().to(nearby_v2_handler))
            .route("/groups/v1",       web::get().to(groups_v1_handler))
            .route("/groups/v2",       web::get().to(groups_v2_handler))
            .route("/rpc/v1",          web::get().to(rpc_discovery_handler))
            .route("/rpc/v1/{name}",   web::get().to(rpc_v1_get_handler))
            .route("/rpc/v1/{name}",   web::post().to(rpc_v1_post_handler))
            .route("/rpc/v2",          web::get().to(rpc_discovery_handler))
            .route("/rpc/v2/{tail:.*}", web::get().to(rpc_v2_handler))
            .route("/ws/events",        web::get().to(ws::ws_handler))
            .default_service(web::route().to(api_root_handler)),
    );
}

// ---------------------------------------------------------------------------
// Static file router
// ---------------------------------------------------------------------------

/// Returns a configuration closure that mounts the widget pages tree.
///
/// - `/`         → `pages_root/index.html` (explicit route; Files::new does
///                 not serve the bare root path)
/// - `/pages/*`  → files under `pages_root`, with CORS
/// - `/shared/*` → files under `pages_root/../shared`, with CORS (only
///                 mounted when that directory exists)
///
/// `.mjs` files are served as `text/javascript` regardless of what
/// `mime_guess` returns for the extension.
pub fn configure_static(pages_root: PathBuf) -> impl Fn(&mut web::ServiceConfig) {
    move |cfg| {
        // Bare-root handler — serves pages/index.html.
        let index_path = pages_root.join("index.html");
        cfg.route("/", web::get().to(move || {
            let p = index_path.clone();
            async move {
                NamedFile::open(p).map_err(actix_web::error::ErrorNotFound)
            }
        }));

        // /pages/* — CORS added unconditionally via DefaultHeaders so that
        // cross-origin font and asset loads work without requiring an Origin
        // header on the request.
        cfg.service(
            web::scope("/pages")
                .wrap(DefaultHeaders::new().add(("Access-Control-Allow-Origin", "*")))
                .wrap(from_fn(fix_preflight))
                .wrap(from_fn(fix_mjs_content_type))
                .service(
                    actix_files::Files::new("", &pages_root)
                        .use_last_modified(true)
                )
        );

        // /shared/* — same setup (optional; only mounted when the directory is present).
        if let Some(parent) = pages_root.parent() {
            let shared_root = parent.join("shared");
            if shared_root.exists() {
                cfg.service(
                    web::scope("/shared")
                        .wrap(DefaultHeaders::new().add(("Access-Control-Allow-Origin", "*")))
                        .wrap(from_fn(fix_preflight))
                        .wrap(from_fn(fix_mjs_content_type))
                        .service(
                            actix_files::Files::new("", &shared_root)
                                .use_last_modified(true)
                        )
                );
            }
        }
    }
}

// Rewrites Content-Type to `text/javascript` for `.mjs` responses.
// mime_guess 2.0.x maps .mjs to application/javascript; the IANA-registered
// value (and the one sauce4zwift uses) is text/javascript.
async fn fix_mjs_content_type(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let is_mjs = req.path().ends_with(".mjs");
    let mut res = next.call(req).await?;
    if is_mjs && res.status().is_success() {
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/javascript"),
        );
    }
    Ok(res)
}
