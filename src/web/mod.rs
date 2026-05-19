// SPDX-License-Identifier: AGPL-3.0-only

mod server;
pub mod state;
pub mod http;
pub mod rpc;
pub(crate) mod ws;
pub(crate) mod subs;

pub use server::{start, WebError, WebServerHandle};
pub use state::WebState;
pub use rpc::RpcRegistry;
pub use zwift_stats::AthleteRegistry;
