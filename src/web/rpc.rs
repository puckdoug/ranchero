// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

type BoxFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'static>>;
type RpcHandler = Box<dyn Fn(Vec<Value>) -> BoxFuture + Send + Sync + 'static>;

pub struct RpcRegistry {
    handlers: HashMap<String, RpcHandler>,
}

impl RpcRegistry {
    pub fn new() -> Self {
        let mut r = Self { handlers: HashMap::new() };
        r.register("getVersion", |_args: Vec<Value>| async {
            Ok::<Value, String>(Value::String(env!("CARGO_PKG_VERSION").to_string()))
        });
        r
    }

    pub fn register<F, Fut>(&mut self, name: &str, f: F)
    where
        F: Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, String>> + Send + 'static,
    {
        self.handlers.insert(
            name.to_string(),
            Box::new(move |args| Box::pin(f(args))),
        );
    }

    pub fn names(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
    }

    pub async fn dispatch(&self, name: &str, args: Vec<Value>) -> Option<Result<Value, String>> {
        let fut = {
            let f = self.handlers.get(name)?;
            f(args)
        };
        Some(fut.await)
    }
}
