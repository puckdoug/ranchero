# Step 16 — SQLite persistence (stub)

## Goal

Per spec §5.7 / §7.10:

- `store.sqlite` — `store(id TEXT PRIMARY KEY, data BLOB)` with WAL.
- `athletes.sqlite` — per-athlete profile cache.
- `segments.sqlite` — segment leaderboard cache with TTL.
- FIT export of a finished session — deferred past v1.

## Tests-first outline

- KV round-trip (insert, update, delete, concurrent read).
- Schema migrations via a tiny `user_version`-based migrator.
- WAL is enabled on open.

To be fully elaborated when we start work on this step.
