// SPDX-License-Identifier: AGPL-3.0-only

mod server;
pub mod state;
pub mod http;
pub mod format;
pub mod rpc;
pub mod proto_to_stats;
pub mod proto_view;
pub(crate) mod ws;
pub(crate) mod subs;

pub use server::{start, WebError, WebServerHandle};
pub use state::{WebState, gc_tick_loop};
pub use rpc::RpcRegistry;
pub use zwift_stats::{AthleteRegistry, GC_TICK_INTERVAL_SECS};
