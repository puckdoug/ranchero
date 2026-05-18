// SPDX-License-Identifier: AGPL-3.0-only

mod server;
pub mod state;
pub(crate) mod http;
pub(crate) mod ws;
pub(crate) mod subs;

pub use server::{start, WebError, WebServerHandle};
pub use state::WebState;
