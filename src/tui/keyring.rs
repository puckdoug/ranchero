//! Backward-compatible re-exports. The credential storage trait and types
//! live in `crate::credentials`; the TUI driver was their original consumer
//! and still imports them through this path.
pub use crate::credentials::{InMemoryKeyringStore, KeyringEntry, KeyringError, KeyringStore};
