// SPDX-License-Identifier: AGPL-3.0-only

pub mod athletes;
pub mod kv;
pub mod migrations;
pub mod open;

pub use athletes::{AthletesDb, AthleteRecord};
pub use kv::KvStore;
pub use migrations::{migrate, Migration};
pub use open::open;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("schema too new: db is at v{found}, max known v{max}")]
    SchemaTooNew { found: u32, max: u32 },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
