# Missing fixture: zwift-proto server_to_client_basic.bin

`crates/zwift-proto/tests/server_to_client.rs::fixture_basic_packet_decodes` fails
when run with `cargo test -- --include-ignored` because the fixture file
`crates/zwift-proto/tests/fixtures/server_to_client_basic.bin` does not exist in
the repository.

## Symptom

```
thread 'fixture_basic_packet_decodes' panicked:
missing fixture …/server_to_client_basic.bin: capture a real ServerToClient payload
from Zwift wire traffic and place it at this path.
```

The test is correctly marked `#[ignore = "requires tests/fixtures/server_to_client_basic.bin
(real Zwift wire capture)"]`, so it is skipped by `cargo test` and `cargo test --
--ignored` would also skip it.  Only `--include-ignored` triggers the failure.

## Root cause

The fixture requires a real captured relay frame from Zwift wire traffic.  It was
never committed because:
- The fixture must be captured from a live Zwift session using `ranchero start
  --capture <path>` and then extracted.
- Committing a live network capture may include personal account identifiers.

The test was introduced in commit `b97b406` and has never had a passing run because
the fixture was not captured at the time.

## Proposed fix

Once a suitable capture is available:

1. Capture a session: `cargo run -- start --capture /tmp/zwift.cap`
2. Extract a `ServerToClient` frame using `cargo run -- follow /tmp/zwift.cap` and
   identify a TCP inbound record.
3. Strip the 2-byte length prefix (outbound TCP records include it; inbound do not)
   and save the raw protobuf bytes to
   `crates/zwift-proto/tests/fixtures/server_to_client_basic.bin`.
4. Verify the frame decodes: `cargo test -p zwift-proto -- --ignored`.

Until then, `--include-ignored` will report one failure from this test.  The
criterion for a "green" full-suite run excludes this test alongside
`https_conditional` (see `docs/planning/flaky-https-conditional.md`).
