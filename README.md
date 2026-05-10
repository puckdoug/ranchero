# Ranchero - a derivative of sauce4zwift written in Rust

## Running tests

The default test run is fast: tests that take more than ~1 s are marked
with `#[ignore]` and excluded by `cargo test`.

| Command                                | What it runs                                          |
| -------------------------------------- | ----------------------------------------------------- |
| `cargo test`                           | Fast tests only. Use this for the inner dev loop.     |
| `cargo test -- --ignored`              | Slow tests only. Use this to exercise the gated set.  |
| `cargo test -- --include-ignored`      | Every test. Use this before merging or for releases.  |

### Slow-test convention

A test is marked slow with a Rust attribute carrying a `slow:` reason:

```rust
#[ignore = "slow: 3 s real-time supervisor refresh; virtual time deadlocks under wiremock multi-thread"]
#[tokio::test]
async fn supervisor_refresh_fires_at_configured_fraction_of_expiration() {
    // ...
}
```

The reason string starts with `slow:` so a `grep` over the test tree
returns the gated set. Include enough context (root cause, references
to plans or comments) that a future reader can decide whether the
marker still applies.

A test should be marked slow when:

- It waits real wall-clock time (≥ ~1 s) that virtual time cannot
  replace — for example, wiremock plus a multi-thread runtime, where
  `tokio::time::pause` is known to deadlock.
- It spawns the daemon binary as a subprocess (cold-start cost is
  ~0.7 s in debug builds).
- The timing is intrinsic to the test invariant (e.g. a follower
  observing records as they are written at a fixed cadence).

Tests that are slow because of a fixable defect should still be marked,
with the reason citing the plan step that will remove the marker.
