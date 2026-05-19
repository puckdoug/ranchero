// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use actix_ws::{AggregatedMessage, Session};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::daemon::relay::GameEvent;
use crate::web::http::format_athlete;
use crate::web::state::WebState;

// ---------------------------------------------------------------------------
// Frame codec — input
// ---------------------------------------------------------------------------

/// Nested `data.{method, arg}` form from the wire protocol.
#[derive(Deserialize, Default)]
struct NestedArg {
    method: Option<String>,
    arg:    Option<Value>,
}

/// Incoming WebSocket frame.  Accepts both the nested wire form
/// `{type, uid, data: {method, arg}}` and the flat spec form
/// `{type, method, uid, arg}`.  Extra fields are silently ignored.
#[derive(Deserialize, Default)]
struct InFrame {
    uid:    Option<i64>,
    // nested form
    data:   Option<NestedArg>,
    // flat form (takes precedence when both are present)
    method: Option<String>,
    arg:    Option<Value>,
}

impl InFrame {
    fn uid(&self) -> i64 {
        self.uid.unwrap_or(-1)
    }

    fn into_method_arg(self) -> (Option<String>, Option<Value>) {
        let method = self.method
            .or_else(|| self.data.as_ref().and_then(|d| d.method.clone()));
        let arg = self.arg
            .or_else(|| self.data.and_then(|d| d.arg));
        (method, arg)
    }
}

// ---------------------------------------------------------------------------
// Frame codec — output
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct WsResponse {
    #[serde(rename = "type")]
    type_:   &'static str,
    success: bool,
    uid:     i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    data:    Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error:   Option<String>,
}

#[derive(Serialize)]
struct WsEvent {
    #[serde(rename = "type")]
    type_:   &'static str,
    uid:     i64,
    success: bool,
    data:    Value,
}

fn ok_frame(uid: i64, data: Option<Value>) -> String {
    serde_json::to_string(&WsResponse {
        type_: "response", success: true, uid, data, error: None,
    }).unwrap()
}

fn err_frame(uid: i64, error: impl Into<String>) -> String {
    serde_json::to_string(&WsResponse {
        type_: "response", success: false, uid, data: None,
        error: Some(error.into()),
    }).unwrap()
}

fn event_text(sub_id: i64, data: Value) -> String {
    serde_json::to_string(&WsEvent {
        type_: "event", uid: sub_id, success: true, data,
    }).unwrap()
}

// ---------------------------------------------------------------------------
// WebSocket route handler
// ---------------------------------------------------------------------------

pub async fn ws_handler(
    state: web::Data<Arc<WebState>>,
    req:   HttpRequest,
    body:  web::Payload,
) -> Result<HttpResponse, actix_web::Error> {
    let (resp, session, stream) = actix_ws::handle(&req, body)?;
    actix_web::rt::spawn(client_task(state.get_ref().clone(), session, stream));
    Ok(resp)
}

// ---------------------------------------------------------------------------
// Client task — owns the read loop and per-client subscriptions
// ---------------------------------------------------------------------------

async fn client_task(
    state:   Arc<WebState>,
    mut session: Session,
    stream:  actix_ws::MessageStream,
) {
    let mut subs: HashMap<i64, JoinHandle<()>> = HashMap::new();
    let mut stream = stream.aggregate_continuations();

    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            AggregatedMessage::Text(text) => {
                let frame: InFrame = match serde_json::from_str(&text) {
                    Ok(f)  => f,
                    Err(_) => {
                        let _ = session.text(err_frame(-1, "malformed JSON")).await;
                        continue;
                    }
                };
                let uid           = frame.uid();
                let (method, arg) = frame.into_method_arg();

                let reply = match method.as_deref() {
                    Some("rpc")         => dispatch_rpc(&state, uid, arg).await,
                    Some("subscribe")   => {
                        handle_subscribe(&state, &mut session, &mut subs, uid, arg).await
                    }
                    Some("unsubscribe") => handle_unsubscribe(&mut subs, uid, arg),
                    _                   => err_frame(uid, "missing or unknown method"),
                };
                let _ = session.text(reply).await;
            }
            AggregatedMessage::Ping(bytes) => {
                let _ = session.pong(&bytes).await;
            }
            AggregatedMessage::Close(_) => break,
            _ => {}
        }
    }

    // Abort all subscription tasks on disconnect.
    for (_, handle) in subs {
        handle.abort();
    }
}

// ---------------------------------------------------------------------------
// RPC dispatch
// ---------------------------------------------------------------------------

async fn dispatch_rpc(state: &WebState, uid: i64, arg: Option<Value>) -> String {
    let arg = match arg {
        Some(v) => v,
        None    => return err_frame(uid, "rpc arg is required"),
    };
    let name = match arg.get("name").and_then(Value::as_str) {
        Some(n) => n.to_string(),
        None    => return err_frame(uid, "rpc arg.name is required"),
    };
    let args: Vec<Value> = arg
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let rpc = match state.rpc.as_ref() {
        Some(r) => r,
        None    => return err_frame(uid, format!("unknown rpc handler: {name}")),
    };

    match rpc.dispatch(&name, args).await {
        None         => err_frame(uid, format!("unknown rpc handler: {name}")),
        Some(Ok(d))  => ok_frame(uid, Some(d)),
        Some(Err(e)) => err_frame(uid, e),
    }
}

// ---------------------------------------------------------------------------
// Subscribe / unsubscribe
// ---------------------------------------------------------------------------

async fn handle_subscribe(
    state:   &Arc<WebState>,
    session: &mut Session,
    subs:    &mut HashMap<i64, JoinHandle<()>>,
    uid:     i64,
    arg:     Option<Value>,
) -> String {
    let arg = match arg {
        Some(v) => v,
        None    => return err_frame(uid, "subscribe arg is required"),
    };

    let event = match arg.get("event").and_then(Value::as_str) {
        Some(e) => e.to_string(),
        None    => return err_frame(uid, "subscribe arg.event is required"),
    };
    let source = match arg.get("source").and_then(Value::as_str) {
        Some(s) => s.to_string(),
        None    => return err_frame(uid, "subscribe arg.source is required"),
    };
    let sub_id = match arg.get("subId").and_then(Value::as_i64) {
        Some(id) => id,
        None     => return err_frame(uid, "subscribe arg.subId is required"),
    };

    if source != "stats" {
        return err_frame(uid, format!("unknown source: {source}"));
    }

    let game_tx = match state.game_events_tx.as_ref() {
        Some(tx) => tx.clone(),
        None     => return err_frame(uid, "stats source not available"),
    };

    let handle = actix_web::rt::spawn(subscription_task(
        game_tx.subscribe(),
        Arc::clone(state),
        event,
        sub_id,
        session.clone(),
    ));
    subs.insert(sub_id, handle);

    ok_frame(uid, None)
}

fn handle_unsubscribe(
    subs: &mut HashMap<i64, JoinHandle<()>>,
    uid:  i64,
    arg:  Option<Value>,
) -> String {
    let sub_id = arg.as_ref()
        .and_then(|v| v.get("subId"))
        .and_then(Value::as_i64);

    match sub_id {
        Some(id) => {
            if let Some(handle) = subs.remove(&id) {
                handle.abort();
            }
            ok_frame(uid, None)
        }
        None => err_frame(uid, "unsubscribe arg.subId is required"),
    }
}

// ---------------------------------------------------------------------------
// Subscription task — runs per subscription until client disconnects
// ---------------------------------------------------------------------------

async fn subscription_task(
    mut rx: broadcast::Receiver<GameEvent>,
    state:  Arc<WebState>,
    event:  String,
    sub_id: i64,
    mut session: Session,
) {
    loop {
        match rx.recv().await {
            Ok(GameEvent::PlayerState { athlete_id, .. }) => {
                if event == "athlete/watching" {
                    let watched = state.watching_id.map(|id| id as i64);
                    if watched != Some(athlete_id) {
                        continue;
                    }
                }

                let athlete_id_u32 = match u32::try_from(athlete_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                };

                let data = {
                    let registry = state.registry.read().unwrap();
                    registry.get(athlete_id_u32).map(|a| {
                        format_athlete(a, state.watching_id, state.self_athlete_id)
                    })
                };

                if let Some(data) = data {
                    if session.text(event_text(sub_id, data)).await.is_err() {
                        break;
                    }
                }
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Closed)     => break,
            Err(broadcast::error::RecvError::Lagged(_))  => continue,
        }
    }
}
