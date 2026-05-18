# Step 16 — SQLite persistence

## Goal

Per spec §5.7 / §7.10, give the daemon three on-disk SQLite
stores so that settings, athlete profiles, and segment
leaderboards survive a restart:

- **`store.sqlite`** — opaque key-value table for daemon
  settings, window/profile state, mod-equivalent flags.
  Schema: `store(id TEXT PRIMARY KEY, data BLOB)`, WAL mode.
- **`athletes.sqlite`** — per-athlete profile cache
  (`id, fname, lname, ftp, weight, badges JSON, last_seen, …`).
  Source of truth for "what do we know about athlete X"
  between sessions; populated lazily from `zwift-api` profile
  fetches and from incoming `PlayerState` ingest.
- **`segments.sqlite`** — segment leaderboard cache with TTL
  per row, so a freshly opened daemon can render the segments
  panel without re-fetching every leaderboard from Zwift.

FIT export of a finished session (`exportFIT`, `stats.mjs:2057`
in sauce4zwift) is **deferred past v1** per the spec stub and
is not in scope here.

### One spec contradiction worth resolving up front

Spec §5.7 describes `store.sqlite` as
`store(id TEXT PK, data TEXT)` (JSON text, matching
sauce4zwift's `src/storage.mjs`); spec §7.10 says
`store(id TEXT PRIMARY KEY, data BLOB)` (binary). The two are
incompatible.

**Decision (pre-committed): ship `data BLOB`** as §7.10 says.
Justification:

1. §7.10 is the rusqlite-specific section and is closer to the
   implementation surface.
2. A `BLOB` column accepts both binary and UTF-8 text without
   loss; callers can encode JSON via `serde_json::to_vec` and
   still get parity with sauce4zwift's behaviour.
3. It avoids forcing a `TEXT` column on values you may want
   to persist as `bincode` / protobuf / compressed bytes
   later (window layouts, captured frame slices, etc.).

Sauce4zwift's `TEXT`-as-JSON pattern is the porting reference,
not the binding contract — see CLAUDE.md.

## Summary checklist

`-T` is a failing test; `-I` is the implementation that turns
it green. The whole list is plain TDD: write the test, watch
it fail, write the smallest code to pass.

### Foundations

- [x] **16.1-T** `tests/data_dir.rs` — `data_dir()` resolves
      to the platform-correct base under
      `~/Library/Application Support/net.heroic.ranchero` on
      darwin (mirrors the existing `default_config_path`
      shape in `src/config/paths.rs`).
- [x] **16.1-I** Add `pub fn data_dir() -> PathBuf` next to
      `default_config_path()`. Reuse the existing
      `directories::ProjectDirs` handle; create the directory
      with `create_dir_all` on first call.
- [x] **16.2-T** `tests/sqlite_open.rs` — opening a fresh DB
      file creates the file, runs the configured pragmas, and
      returns a usable `Connection`. Asserts:
      `PRAGMA journal_mode` returns `wal`,
      `PRAGMA foreign_keys` returns `1`,
      `PRAGMA busy_timeout` returns a positive integer.
- [x] **16.2-I** `zwift-store::open(path)` — open the
      connection, apply the standard pragma bundle, return
      the `Connection`. Single helper used by all three DBs.

### Migrations

- [x] **16.3-T** `tests/migrations.rs` — a fresh DB with
      `PRAGMA user_version = 0` runs every migration in order
      and ends at `user_version = N`. A DB at
      `user_version = k` runs only migrations `k+1..=N`.
      Migrations are idempotent across reruns.
- [x] **16.3-I** `zwift-store::migrate(conn, &[Migration])`
      where `Migration` is `{ version: u32, sql: &'static str }`
      executed in a transaction per migration. Reads/writes
      `user_version` via `PRAGMA`.
- [x] **16.4-T** Migration runner refuses to downgrade:
      opening a DB whose `user_version` exceeds the highest
      known migration returns `Error::SchemaTooNew`.

### Key-value store (`store.sqlite`)

- [x] **16.5-T** `tests/kv_round_trip.rs` — `put`/`get`/
      `delete`/`exists` over `BLOB` data. Insert, read back
      the identical bytes (binary safety), overwrite, delete,
      `get` returns `None`.
- [x] **16.5-I** `KvStore::open(path)` runs migrations and
      returns a handle. Methods: `put(&self, id: &str, data:
      &[u8])`, `get(&self, id: &str) -> Result<Option<Vec<u8>>>`,
      `delete(&self, id: &str) -> Result<bool>`,
      `exists(&self, id: &str) -> Result<bool>`.
- [x] **16.6-T** `tests/kv_concurrent.rs` — WAL allows a
      reader to complete while a writer holds an open
      transaction. Spawn two threads sharing the same DB file;
      the reader must observe the pre-transaction value while
      the writer is mid-write, and the post-commit value
      afterwards. Marked `#[ignore = "slow: threads +
      busy-wait"]`.
- [x] **16.7-T** `tests/kv_json_helper.rs` — `put_json` /
      `get_json` round-trip a `serde_json::Value` through the
      `BLOB` column without data corruption.
- [x] **16.7-I** Add `put_json<T: Serialize>` and
      `get_json<T: DeserializeOwned>` thin wrappers over
      `put`/`get`. Documented as the recommended path for
      settings.

### Athletes cache (`athletes.sqlite`)

- [x] **16.8-T** `tests/athletes_schema.rs` — fresh DB has
      the `athletes` table with the documented columns,
      primary key on `id`, and an index on `last_seen DESC`.
- [x] **16.8-I** Migration 1 of `athletes.sqlite` creates the
      table per the schema in *Schema definitions* below.
- [x] **16.9-T** `tests/athletes_upsert.rs` — `upsert(record)`
      inserts a new row and replaces an existing row by `id`,
      preserving all columns the caller filled and leaving
      unset optional columns as `NULL`.
- [x] **16.9-I** `AthletesDb::upsert(&self, &AthleteRecord)`
      using `INSERT … ON CONFLICT(id) DO UPDATE SET …`.
- [x] **16.10-T** `tests/athletes_touch.rs` — `touch(id, ts)`
      updates `last_seen` without rewriting any other column;
      missing row → `false`, present row → `true`.
- [x] **16.10-I** `AthletesDb::touch(&self, id, last_seen)`.
- [x] **16.11-T** `tests/athletes_get.rs` — `get(id)` returns
      a populated record for a present row, `None` for
      missing, and round-trips `badges` as JSON without
      corruption.
- [x] **16.11-I** `AthletesDb::get(&self, id) ->
      Result<Option<AthleteRecord>>`.

### Segments cache (`segments.sqlite`)

- [x] **16.12-T** `tests/segments_schema.rs` — fresh DB has
      the `leaderboards` table (segment_id, payload BLOB,
      inserted_at, expires_at) with primary key on
      `segment_id` and an index on `expires_at`.
- [x] **16.12-I** Migration 1 of `segments.sqlite` creates
      the table per *Schema definitions*.
- [x] **16.13-T** `tests/segments_put_get.rs` — `put` inserts
      or overwrites a `(segment_id, payload, ttl)` entry,
      `get(segment_id, now)` returns the payload if
      `now < expires_at` and `None` otherwise (does not
      auto-delete).
- [x] **16.13-I** `SegmentsDb::put(&self, segment_id, payload,
      ttl: Duration, now: SystemTime)`,
      `SegmentsDb::get(&self, segment_id, now) ->
      Result<Option<Vec<u8>>>`.
- [x] **16.14-T** `tests/segments_evict.rs` —
      `evict_expired(now)` deletes every row with
      `expires_at <= now` and returns the count; entries with
      `expires_at > now` remain.
- [x] **16.14-I** `SegmentsDb::evict_expired(&self, now) ->
      Result<usize>`.
- [x] **16.15-T** `tests/segments_evict_idempotent.rs` —
      calling `evict_expired` repeatedly on the same DB is
      idempotent (returns 0 after the first sweep until new
      entries expire).

### Wiring

- [ ] **16.16-T** `tests/daemon_wiring.rs` (in the root
      crate) — daemon boot opens all three DBs under
      `data_dir()`, creates the files if missing, and runs
      migrations. Smoke test only; uses a `tempfile::TempDir`
      override of the data directory.
- [ ] **16.16-I** Boot path in `src/daemon.rs` (or wherever
      `start` initialises subsystems) opens
      `KvStore`/`AthletesDb`/`SegmentsDb` and stashes them on
      the daemon handle. A test-only env var
      (`RANCHERO_DATA_DIR`) overrides `data_dir()` for the
      wiring test.
- [ ] **16.17-T** `tests/cli_status.rs` — `ranchero status`
      reports the on-disk size of each DB file when the
      daemon isn't running (read-only inspection that does
      not need a live connection).
- [ ] **16.17-I** Extend the existing `status` printer to add
      a "Persistence" block. Bytes-only; no schema
      introspection yet.

## Tests-first plan (detail)

### 16.1 `data_dir()` resolves to platform-correct base

Discussed in §"Foundations". The darwin assertion uses the
existing `directories::ProjectDirs::from("net", "heroic",
"ranchero")` handle. Linux gets the XDG path automatically;
Windows is best-effort and not exercised by CI.

Edge case: when `XDG_DATA_HOME` is set, the resolved path
should honour it on Linux. The macOS test does not exercise
that env var because `directories` does not consult it on
darwin (verified by the open-verification list at the bottom
of this plan).

### 16.2 `open(path)` enables WAL and the standard pragma bundle

Standard pragma bundle:

```
PRAGMA journal_mode  = WAL;
PRAGMA synchronous   = NORMAL;
PRAGMA foreign_keys  = ON;
PRAGMA busy_timeout  = 5000;
PRAGMA temp_store    = MEMORY;
```

Rationale: `journal_mode = WAL` for the multi-reader /
single-writer behaviour the daemon depends on.
`synchronous = NORMAL` is the standard WAL pairing — `FULL`
is too costly for a write-heavy KV. `busy_timeout = 5000`
gives the runtime a five-second window to retry on lock
contention before surfacing `SQLITE_BUSY`.

### 16.3 / 16.4 Migrations

Migrations are an ordered slice of `(version, sql)` tuples
embedded in the crate via `include_str!` from a
`migrations/` subdir. The runner:

1. Reads `PRAGMA user_version` (defaults to 0 on a fresh DB).
2. For each migration with `version > current`, runs the SQL
   inside a transaction and writes the new `user_version`.
3. Refuses to open when `current > max(migration.version)`.

No "down" migrations. No external migrator dep. The plan
keeps the migrator under fifty lines of code.

### 16.5 / 16.6 / 16.7 KV semantics

`KvStore` is `Send + Sync`. Internally it owns a single
`Mutex<Connection>` (good enough for the daemon's load —
every KV call is a single statement). Methods use prepared
statements cached on the connection.

`put_json` / `get_json` use `serde_json::to_vec` /
`serde_json::from_slice`. The BLOB column stores opaque
bytes, so callers may also use `bincode`, protobuf, or raw
bytes without coordination.

### 16.8 — 16.11 Athletes cache

The schema (see *Schema definitions*) is intentionally
narrow: just the fields the UI/HUD reads back without re-
fetching from Zwift. `badges` is JSON because the structure
is small but variable. `last_seen` is a unix-epoch integer
(seconds) for easy `ORDER BY` and TTL-based eviction in a
future step.

### 16.12 — 16.15 Segments cache

`expires_at` is unix-epoch seconds; `evict_expired(now)` is
explicit (the daemon calls it on a timer) rather than a
trigger, to keep eviction observable. `put` overwrites on
conflict so a re-fetched leaderboard naturally replaces the
stale one. TTL is per-row, supplied by the caller — segments
with cheap-to-recompute payloads can use a short TTL;
expensive ones can use a long TTL.

### 16.16 / 16.17 Wiring

The wiring test exercises the **path**, not the **content** —
it checks that boot opens the files, runs migrations, and
exposes the three handles. Functional behaviour of each DB
is covered by the per-DB tests.

## Resolved decisions

- **`rusqlite` with the `bundled` feature** — vendors SQLite,
  no system dependency, matches sauce4zwift's
  `better-sqlite3` ergonomics. Async via
  `tokio::task::spawn_blocking` at call sites that need it
  (the DB calls themselves are sync). No `sqlx`, no
  connection pool — single mutex-wrapped connection per DB.
- **One `Connection` per DB, behind a `Mutex`.** WAL
  guarantees multi-reader / single-writer; for the daemon's
  traffic profile (small writes, infrequent), a pool would
  be overkill.
- **`data BLOB` in `store.sqlite`** (see *Goal*).
- **`user_version`-based migrator, in-crate**, no external
  dep.
- **No FIT export.** Deferred past v1 (spec stub).
- **Crate name `zwift-store`**, sibling to `zwift-api`,
  `zwift-relay`, `zwift-stats`. Justification: persistence
  is cross-cutting and should not pull `zwift-stats` (which
  does not need rusqlite) or live in the root binary (which
  would make isolated testing awkward). The root binary
  depends on `zwift-store` and owns the wiring.
- **`AthleteRecord` is owned by `zwift-store`**, not
  `zwift-stats`. The in-memory `AthleteData` in
  `zwift-stats` is the live ingest type; `AthleteRecord` is
  the persistence shape. A small adapter on the root-crate
  side maps between them; the two are deliberately
  decoupled to keep `zwift-stats` rusqlite-free.

## Crate layout

```
crates/zwift-store/
    Cargo.toml
    src/
        lib.rs            — re-exports, Error type
        open.rs           — open(path), pragma bundle
        migrations.rs     — migration runner
        kv.rs             — KvStore
        athletes.rs       — AthletesDb, AthleteRecord
        segments.rs       — SegmentsDb
        paths.rs          — known DB filenames as consts
    migrations/
        store/0001_init.sql
        athletes/0001_init.sql
        segments/0001_init.sql
    tests/
        sqlite_open.rs
        migrations.rs
        kv_round_trip.rs
        kv_concurrent.rs                (ignored: slow)
        kv_json_helper.rs
        athletes_schema.rs
        athletes_upsert.rs
        athletes_touch.rs
        athletes_get.rs
        segments_schema.rs
        segments_put_get.rs
        segments_evict.rs
        segments_evict_idempotent.rs
```

Add `crates/zwift-store` to the workspace members in the
root `Cargo.toml`.

## Public API surface (proposed)

```rust
// lib.rs
pub use kv::KvStore;
pub use athletes::{AthletesDb, AthleteRecord};
pub use segments::SegmentsDb;
pub use open::{open, StandardPragmas};
pub use migrations::{migrate, Migration};
pub use paths::{STORE_FILENAME, ATHLETES_FILENAME, SEGMENTS_FILENAME};

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
```

```rust
// open.rs
pub struct StandardPragmas;
impl StandardPragmas {
    pub fn apply(conn: &rusqlite::Connection) -> Result<()>;
}
pub fn open(path: &Path) -> Result<rusqlite::Connection>;
```

```rust
// migrations.rs
pub struct Migration {
    pub version: u32,
    pub sql:     &'static str,
}
pub fn migrate(conn: &rusqlite::Connection, ms: &[Migration]) -> Result<()>;
```

```rust
// kv.rs
pub struct KvStore { /* Mutex<Connection> */ }
impl KvStore {
    pub fn open(path: &Path) -> Result<Self>;
    pub fn put   (&self, id: &str, data: &[u8])    -> Result<()>;
    pub fn get   (&self, id: &str)                 -> Result<Option<Vec<u8>>>;
    pub fn delete(&self, id: &str)                 -> Result<bool>;
    pub fn exists(&self, id: &str)                 -> Result<bool>;
    pub fn put_json<T: Serialize>      (&self, id: &str, v: &T) -> Result<()>;
    pub fn get_json<T: DeserializeOwned>(&self, id: &str)       -> Result<Option<T>>;
}
```

```rust
// athletes.rs
#[derive(Debug, Clone, PartialEq)]
pub struct AthleteRecord {
    pub id:        i64,
    pub fname:     Option<String>,
    pub lname:     Option<String>,
    pub ftp:       Option<i32>,
    pub weight:    Option<f64>,            // kg
    pub badges:    Option<serde_json::Value>,
    pub last_seen: i64,                    // unix seconds
}

pub struct AthletesDb { /* Mutex<Connection> */ }
impl AthletesDb {
    pub fn open(path: &Path)                            -> Result<Self>;
    pub fn upsert(&self, rec: &AthleteRecord)           -> Result<()>;
    pub fn touch(&self, id: i64, last_seen: i64)        -> Result<bool>;
    pub fn get(&self, id: i64)                          -> Result<Option<AthleteRecord>>;
}
```

```rust
// segments.rs
pub struct SegmentsDb { /* Mutex<Connection> */ }
impl SegmentsDb {
    pub fn open(path: &Path)                                          -> Result<Self>;
    pub fn put(&self, segment_id: i64, payload: &[u8],
               ttl: Duration, now: SystemTime)                        -> Result<()>;
    pub fn get(&self, segment_id: i64, now: SystemTime)               -> Result<Option<Vec<u8>>>;
    pub fn evict_expired(&self, now: SystemTime)                      -> Result<usize>;
}
```

## Schema definitions

### `store.sqlite`

```sql
-- migrations/store/0001_init.sql
CREATE TABLE store (
    id    TEXT PRIMARY KEY NOT NULL,
    data  BLOB             NOT NULL
);
```

### `athletes.sqlite`

```sql
-- migrations/athletes/0001_init.sql
CREATE TABLE athletes (
    id         INTEGER PRIMARY KEY NOT NULL,
    fname      TEXT,
    lname      TEXT,
    ftp        INTEGER,
    weight     REAL,
    badges     TEXT,                       -- JSON
    last_seen  INTEGER NOT NULL DEFAULT 0  -- unix seconds
);
CREATE INDEX athletes_last_seen ON athletes (last_seen DESC);
```

`badges` is stored as `TEXT` (JSON) for human-readable
inspection via the `sqlite3` CLI; the `data BLOB` argument
for the `store` table does not apply here because the field
has a known shape.

### `segments.sqlite`

```sql
-- migrations/segments/0001_init.sql
CREATE TABLE leaderboards (
    segment_id   INTEGER PRIMARY KEY NOT NULL,
    payload      BLOB    NOT NULL,
    inserted_at  INTEGER NOT NULL,         -- unix seconds
    expires_at   INTEGER NOT NULL          -- unix seconds
);
CREATE INDEX leaderboards_expires_at ON leaderboards (expires_at);
```

## Wiring into the workspace

- Root `Cargo.toml`: add `crates/zwift-store` to
  `[workspace.members]`.
- Root binary `Cargo.toml`: add
  `zwift-store = { path = "crates/zwift-store" }` under
  `[dependencies]`.
- `src/config/paths.rs`: add `pub fn data_dir() -> PathBuf`
  next to `default_config_path`.
- `src/cli.rs` / wherever the `start` command initialises
  subsystems: open the three DBs after credential
  resolution, before the relay handshake. Failure to open is
  fatal — log and exit non-zero.
- `src/cli.rs` `status` printer: print the on-disk size of
  each DB file under a new "Persistence" section.

## Acceptance criteria

- `cargo test -p zwift-store` is fully green on a clean
  checkout; the dev-loop set finishes in under a second.
- `cargo test -p zwift-store -- --include-ignored` runs the
  WAL concurrency test and passes.
- `ranchero start` on a clean machine creates
  `store.sqlite`, `athletes.sqlite`, `segments.sqlite` under
  `data_dir()` and the daemon logs each file's path at
  lifecycle level (not gated behind `-v`, per the
  lifecycle-logging convention in CLAUDE.md).
- `ranchero stop` followed by `ranchero start` shows that
  the DB files persist across the restart and remain
  readable (smoke test only — no end-to-end persistence of
  live state in this step).
- `ranchero status` (daemon down) reports the on-disk size
  of each DB file.
- No code outside the root binary opens a SQLite connection
  directly; every caller goes through `zwift-store`.

## Out of scope for STEP 16

- **FIT export of finished sessions.** Deferred past v1 per
  spec stub and CLAUDE.md.
- **Writing live `AthleteData` snapshots to
  `athletes.sqlite` on a tick or session boundary.** This
  step provides the store; the integration that actually
  persists ingest data belongs in a later step (probably
  STEP 18+).
- **A background eviction job for the segments cache.** The
  `evict_expired` method exists; wiring it into a periodic
  task is left to whoever wires segment fetches.
- **Encryption at rest.** Credentials stay in `keyring`; the
  SQLite files contain no secrets.
- **Schema introspection in `ranchero status`.** Bytes-only
  for now.
- **Backups, vacuum scheduling, integrity checks.** SQLite's
  defaults are sufficient for v1.

## Open verification points

Items to confirm with a quick read before starting
implementation; if any are wrong, the implementation note
above needs an amendment, not the test.

- The `directories` crate is already a dependency of the
  root binary (asserted by the research that produced this
  plan, but worth a `cargo tree` before promising
  `ProjectDirs::from(…)` in 16.1-I). If it is not, the
  implementation step needs to add it.
- `thiserror` is already in use somewhere in the workspace.
  If not, decide whether to add it for `zwift-store::Error`
  or hand-roll `Display`/`Error`.
- On darwin, `directories` does not honour `XDG_DATA_HOME`;
  the 16.1 test should set the env var only on Linux (or
  skip the assertion on darwin).
- The daemon module that owns `start` has a clear seam for
  opening subsystem handles (currently expected to be in
  `src/cli.rs` or `src/daemon.rs`). If the seam is messy,
  the wiring step (16.16-I) may need to grow a small
  `Subsystems` struct rather than threading three handles
  individually.
