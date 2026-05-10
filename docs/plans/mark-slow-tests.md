# Mark slow tests

Certain tests take significant time to run (>100 ms per test). This plan
inventories those tests, distinguishes the ones that can be made faster
from the ones that cannot, and proposes a default-skip mechanism for the
inherently slow ones so `cargo test` returns quickly.

## Execution checklist

Tick items as they land. Detail for each step is in the "Implementation
plan" section below.

### Step 1 — Mark Category B tests as slow (~42 s)

- [x] `crates/zwift-relay/tests/session.rs::supervisor_refresh_fires_at_configured_fraction_of_expiration`
- [x] `crates/zwift-relay/tests/session.rs::supervisor_refresh_failure_triggers_relogin`
- [x] `crates/zwift-relay/tests/session.rs::supervisor_relogin_failure_emits_login_failed_with_attempt_count`
- [x] `crates/zwift-relay/tests/session.rs::supervisor_shutdown_cancels_pending_refresh`
- [x] `crates/zwift-relay/tests/session.rs::supervisor_refresh_fire_emits_scheduled_delay_event`
- [x] `crates/zwift-relay/tests/session.rs::supervisor_refresh_failure_path_emits_refresh_failed_and_relogin_attempt`
- [x] `crates/zwift-relay/tests/session.rs::supervisor_relogin_success_emits_relogin_ok`
- [x] `crates/zwift-relay/tests/session.rs::supervisor_persistent_login_failure_emits_login_failed_warn`
- [x] `tests/relay_runtime.rs::start_with_writer_subscribes_to_real_supervisor_events`
- [x] `tests/relay_runtime.rs::start_with_writer_records_fresh_manifest_on_supervisor_relogin`
- [x] `tests/relay_runtime.rs::login_http_exchange_appears_in_capture`
- [x] `tests/relay_runtime.rs::start_all_inner_writes_udp_outbound_to_capture_file`
- [x] `crates/zwift-api/tests/auth.rs::preemptive_refresh_fires_at_half_expires_in`
- [x] `crates/zwift-relay/tests/capture.rs::follower_reads_records_as_they_are_written`
- [x] `crates/zwift-relay/tests/capture.rs::follower_with_poll_interval_respects_setting`

### Step 2 — Mark Category C tests as slow (~25 s)

- [x] `tests/logging.rs::default_silences_info_on_stderr`
- [x] `tests/logging.rs::verbose_flag_emits_startup_info_to_stderr`
- [x] `tests/logging.rs::debug_flag_emits_control_debug_to_stderr`
- [x] `tests/logging.rs::rust_log_env_overrides_default_filter`
- [x] `tests/daemon_lifecycle.rs::stop_clears_pid_file_and_status_reports_shutdown`

### Step 3 — Mark daemon-lifecycle outliers as slow (~14 s)

- [x] `tests/daemon_lifecycle.rs::start_canonicalizes_relative_capture_path`
- [x] `tests/daemon_lifecycle.rs::capture_file_handle_survives_fork`
- [x] `tests/daemon_lifecycle.rs::start_when_already_running_refuses`
- [x] `tests/daemon_lifecycle.rs::backgrounded_start_returns_quickly_and_keeps_running`
- [x] `tests/daemon_lifecycle.rs::stale_pid_file_is_cleaned_up_on_start`
- [x] `tests/daemon_lifecycle.rs::debug_flag_keeps_process_in_foreground`
- [x] `tests/daemon_lifecycle.rs::start_exits_nonzero_when_log_directory_missing`
- [x] `tests/daemon_lifecycle.rs::start_removes_socket_when_relay_start_fails`
- [x] `tests/daemon_lifecycle.rs::start_exits_nonzero_when_pidfile_directory_missing`
- [x] `tests/daemon_lifecycle.rs::start_exits_nonzero_and_prints_error_when_password_missing`
- [x] `tests/daemon_lifecycle.rs::start_does_not_write_socket_when_validation_fails`
- [x] `tests/daemon_lifecycle.rs::start_does_not_write_pidfile_when_validation_fails`
- [x] `tests/daemon_lifecycle.rs::start_exits_nonzero_and_prints_error_when_email_missing`
- [x] `tests/daemon_lifecycle.rs::start_removes_pidfile_when_relay_start_fails`
- [x] `tests/daemon_lifecycle.rs::start_exits_nonzero_when_relay_start_fails`
- [x] `tests/daemon_lifecycle.rs::start_exits_nonzero_when_capture_path_not_openable`
- [x] `tests/cli_args.rs::dispatch_start_passes_capture_path_to_daemon`

### Step 4 — Document the convention

- [x] Add "Running tests" section (README or `docs/development.md`) covering the three commands and the `#[ignore = "slow: …"]` convention
- [x] Confirm CI runs `cargo test -- --include-ignored` *(no CI configured in this repo as of 2026-05-10; convention will apply once CI is added)*

### Step 5 — Verify the quick-win baseline

- [x] `cargo test` finishes in ≲20 s wall clock *(measured 8.4 s on 2026-05-10)*
- [x] `cargo test -- --include-ignored` matches today's 678 passing tests *(683 total today; 682 pass, 1 pre-existing failure `fixture_basic_packet_decodes` from missing `tests/fixtures/server_to_client_basic.bin` — unrelated to this plan)*
- [x] `cargo test -- --ignored` runs only the marked tests and passes *(42 ignored tests run: all 37 slow-marked tests pass; 2 pre-existing failures — `os_main_and_monitor_are_independent` keychain test and `fixture_basic_packet_decodes` — are unrelated to this plan)*

### Step 6 — Optimize `print_follow_to` for sub-second timeouts (~13 s)

- [x] Change `print_follow_to(idle_timeout_secs: Option<u64>)` to `idle_timeout: Option<Duration>` in `src/cli.rs`
- [x] Update CLI dispatch site to convert `--idle-timeout` (seconds) into `Duration::from_secs(n)`
- [x] Update `tests/follow.rs::run_follow` signature and switch every `Some(1)` call site to `Some(Duration::from_millis(100))`
- [x] Adjust elapsed-time assertion in `follow_returns_within_idle_timeout_when_no_records_arrive` (800 ms / 2.5 s → ~100 ms / ~500 ms)
- [x] Decide on `tests/full_scope.rs::workflow_start_capture_follow_reads_header` *(API now supports sub-second timeouts; this test could be optimized in a follow-up, but is not a blocker for the baseline)*

### Step 7 — Investigate Category C root cause

- [ ] Add `Instant::now()` probes in one Category C test to confirm `child.wait_with_output()` is the slow call
- [ ] Bisect daemon exit path in `src/daemon/runtime.rs::start()` to identify which step holds the stderr pipe open
- [ ] Try moving `_log_guard` out of `start()` (or explicit `drop` before `tracing::info!("ranchero stopped")`) and re-measure
- [ ] If the guard is not the culprit, audit spawned tasks in `run_daemon` for ones that don't abort on shutdown
- [ ] Confirm Category C tests run in <500 ms each
- [ ] Remove `#[ignore]` markers added in Step 2 from the affected tests

## Methodology

Timings were captured with:

```
cargo +nightly test -- -Z unstable-options --report-time
```

The unstable `--report-time` flag prints a `<elapsed>` token next to each
test name. The numbers below are from a single sequential run; absolute
values are reproducible to ~10 ms.

## Headline numbers

- **678** tests pass in **~80 s** wall-clock today.
- **84** tests at ≥ 100 ms account for **104.5 s** of cumulative test
  time (96.5 % of the 108.2 s sequential-sum budget).
- The five slowest tests each consume ~5 s. The next ten consume ~3 s
  apiece. Together those top 15 tests dominate the wall clock through
  per-binary serialization.

So a small number of tests dominate. Trimming or gating them is high
leverage.

## Slow-test inventory (≥ 100 ms, sorted by elapsed)

Each row is `elapsed | file | test fn`.

```
5.110  tests/logging.rs                              debug_flag_emits_control_debug_to_stderr
5.104  tests/logging.rs                              default_silences_info_on_stderr
5.101  tests/daemon_lifecycle.rs                     stop_clears_pid_file_and_status_reports_shutdown
5.098  tests/logging.rs                              verbose_flag_emits_startup_info_to_stderr
5.085  tests/logging.rs                              rust_log_env_overrides_default_filter
5.005  crates/zwift-relay/tests/session.rs           supervisor_shutdown_cancels_pending_refresh
4.511  tests/relay_runtime.rs                        start_with_writer_records_fresh_manifest_on_supervisor_relogin
4.506  tests/relay_runtime.rs                        start_with_writer_subscribes_to_real_supervisor_events
3.509  crates/zwift-relay/tests/capture.rs           follower_reads_records_as_they_are_written
3.116  crates/zwift-relay/tests/session.rs           supervisor_persistent_login_failure_emits_login_failed_warn
3.108  crates/zwift-relay/tests/session.rs           supervisor_relogin_failure_emits_login_failed_with_attempt_count
3.013  crates/zwift-relay/tests/session.rs           supervisor_refresh_failure_path_emits_refresh_failed_and_relogin_attempt
3.008  crates/zwift-relay/tests/session.rs           supervisor_relogin_success_emits_relogin_ok
3.006  crates/zwift-relay/tests/session.rs           supervisor_refresh_fires_at_configured_fraction_of_expiration
3.006  crates/zwift-relay/tests/session.rs           supervisor_refresh_fire_emits_scheduled_delay_event
3.006  crates/zwift-relay/tests/session.rs           supervisor_refresh_failure_triggers_relogin
2.006  crates/zwift-api/tests/auth.rs                preemptive_refresh_fires_at_half_expires_in
1.517  tests/relay_runtime.rs                        login_http_exchange_appears_in_capture
1.217  tests/relay_runtime.rs                        start_all_inner_writes_udp_outbound_to_capture_file
1.126  crates/zwift-relay/tests/capture.rs           follower_with_poll_interval_respects_setting
1.119  tests/full_scope.rs                           workflow_start_capture_follow_reads_header
1.117  tests/follow.rs                               follow_prints_one_summary_line_per_frame_record
1.112  tests/follow.rs                               follow_output_contains_no_some_wrappers
1.108  tests/follow.rs                               follow_http_protobuf_payload_is_decoded
1.101  tests/follow.rs                               follow_decrypts_outbound_tcp_frame
1.100  tests/follow.rs                               follow_output_contains_no_none_fields
1.097  tests/follow.rs                               follow_decrypts_inbound_tcp_frame
1.086  tests/follow.rs                               follow_http_json_payload_is_pretty_printed
1.081  tests/follow.rs                               follow_prints_format_version_header
1.079  tests/follow.rs                               follow_output_includes_manifest_summary
1.074  tests/follow.rs                               follow_http_empty_payload_prints_empty_marker
1.071  tests/follow.rs                               follow_returns_within_idle_timeout_when_no_records_arrive
1.068  tests/follow.rs                               follow_http_urlencoded_payload_is_displayed
0.907  tests/daemon_lifecycle.rs                     start_canonicalizes_relative_capture_path
0.904  tests/daemon_lifecycle.rs                     capture_file_handle_survives_fork
0.900  tests/daemon_lifecycle.rs                     start_when_already_running_refuses
0.899  tests/daemon_lifecycle.rs                     backgrounded_start_returns_quickly_and_keeps_running
0.868  tests/daemon_lifecycle.rs                     stale_pid_file_is_cleaned_up_on_start
0.863  tests/daemon_lifecycle.rs                     debug_flag_keeps_process_in_foreground
0.854  tests/daemon_lifecycle.rs                     start_exits_nonzero_when_log_directory_missing
0.853  tests/daemon_lifecycle.rs                     start_removes_socket_when_relay_start_fails
0.853  tests/daemon_lifecycle.rs                     start_exits_nonzero_when_pidfile_directory_missing
0.853  tests/daemon_lifecycle.rs                     start_exits_nonzero_and_prints_error_when_password_missing
0.853  tests/daemon_lifecycle.rs                     start_does_not_write_socket_when_validation_fails
0.853  tests/daemon_lifecycle.rs                     start_does_not_write_pidfile_when_validation_fails
0.848  tests/daemon_lifecycle.rs                     start_exits_nonzero_and_prints_error_when_email_missing
0.847  tests/daemon_lifecycle.rs                     start_removes_pidfile_when_relay_start_fails
0.847  tests/daemon_lifecycle.rs                     start_exits_nonzero_when_relay_start_fails
0.836  tests/daemon_lifecycle.rs                     start_exits_nonzero_when_capture_path_not_openable
0.606  tests/cli_args.rs                             dispatch_start_passes_capture_path_to_daemon
0.503  tests/relay_runtime.rs                        start_all_inner_waits_for_udp_config_before_udp_connect
0.361  crates/zwift-relay/tests/tcp.rs               watchdog_fires_after_silence
0.234  tests/logging.rs                              logfile_is_appended_across_two_runs
0.202  crates/zwift-relay/tests/udp.rs               watchdog_fires_after_silence
0.189  crates/zwift-relay/tests/capture.rs           follower_resumes_after_truncated_record_at_eof
0.177  crates/zwift-relay/tests/capture.rs           follower_no_idle_timeout_blocks_indefinitely
0.176  crates/zwift-relay/tests/udp.rs               udp_channel_with_capture_records_outbound_player_state
0.173  crates/zwift-relay/tests/udp.rs               udp_channel_with_capture_records_inbound_packets
0.172  crates/zwift-relay/tests/udp.rs               udp_steady_state_recv_records_raw_datagram_pre_decrypt
0.171  crates/zwift-relay/tests/udp.rs               udp_steady_state_send_records_encrypted_datagram
0.161  crates/zwift-relay/tests/udp.rs               udp_steady_state_recv_emits_relay_udp_message_recv_with_fields
0.160  crates/zwift-relay/tests/udp.rs               udp_steady_state_send_emits_relay_udp_playerstate_sent
0.160  crates/zwift-relay/tests/udp.rs               udp_hello_ack_matcher_reads_ackseqno_at_proto_tag_5_not_tag_4
0.160  crates/zwift-relay/tests/udp.rs               recv_loop_emits_inbound_event_per_decoded_packet
0.160  crates/zwift-relay/tests/udp.rs               establish_converges_after_six_replies_and_emits_established
0.160  crates/zwift-relay/tests/tcp.rs               shutdown_stops_recv_loop_and_emits_shutdown_event
0.159  crates/zwift-relay/tests/udp.rs               udp_recv_trace_player_count_uses_states_tag_8_not_player_states_tag_28
0.159  crates/zwift-relay/tests/udp.rs               udp_hello_recv_emits_ack_per_response_and_one_converged_event
0.159  crates/zwift-relay/tests/udp.rs               send_player_state_emits_packet_with_seqno_flag_only
0.159  crates/zwift-relay/tests/udp.rs               recv_loop_decryption_failure_emits_recv_error
0.156  tests/relay_runtime.rs                        heartbeat_send_failure_emits_warn
0.154  tests/relay_runtime.rs                        heartbeat_tick_emits_debug_event_per_interval
0.149  tests/full_scope.rs                           cli_capture_flag_governs_capture_file_creation
0.133  tests/logging.rs                              backgrounded_daemon_writes_lifecycle_to_logfile_without_flags
0.118  tests/full_scope.rs                           foreground_start_emits_relay_lifecycle_to_stderr
0.116  tests/relay_runtime.rs                        recv_loop_handles_tcp_inbound_and_emits_relay_tcp_message_recv
0.113  tests/relay_runtime.rs                        supervisor_refresh_writes_fresh_manifest_when_key_rotates
0.109  tests/relay_runtime.rs                        udp_channel_swap_runs_grace_shutdown_on_old_channel
0.108  tests/full_scope.rs                           workflow_stop_leaves_capture_file_readable
0.108  tests/full_scope.rs                           daemon_drives_capture_open_close_lifecycle
0.107  tests/full_scope.rs                           daemon_logs_relay_capture_opened_in_background
0.106  tests/relay_runtime.rs                        supervisor_relogin_recreates_channels_with_new_key
0.105  tests/relay_runtime.rs                        udp_config_v2_and_flat_fallback_paths_are_inert
0.105  tests/relay_runtime.rs                        tcp_server_pinned_across_reconnects
```

### Distribution by file

| file                                       | count |
| ------------------------------------------ | ----- |
| `tests/daemon_lifecycle.rs`                | 17    |
| `tests/relay_runtime.rs`                   | 13    |
| `crates/zwift-relay/tests/udp.rs`          | 13    |
| `tests/follow.rs`                          | 12    |
| `crates/zwift-relay/tests/session.rs`      | 8     |
| `tests/logging.rs`                         | 6     |
| `tests/full_scope.rs`                      | 6     |
| `crates/zwift-relay/tests/capture.rs`      | 4     |
| `crates/zwift-relay/tests/tcp.rs`          | 2     |
| `tests/cli_args.rs`                        | 1     |
| `crates/zwift-api/tests/auth.rs`           | 1     |

## Categorization

Tests fall into three buckets based on the source of their wait:

### Category A — Optimizable (can be made fast)

These tests pay real wall-clock time for a delay that the test author
chose for convenience and that a small refactor could replace with a
shorter wait or virtual time.

| Tests                                            | Why slow                                                               | How to fix                                                                                                                                          |
| ------------------------------------------------ | ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `follow_*` (12 tests in `tests/follow.rs`, ~13 s) | Each calls `print_follow_to(..., Some(1))`, where the timeout is in **seconds**. The follower already accepts a `Duration` internally. | Change `print_follow_to`'s `idle_timeout_secs: Option<u64>` parameter to `idle_timeout: Option<Duration>` (or add a `_ms` variant) so tests can use 100 ms. CLI dispatch keeps building a `Duration::from_secs` from the `--idle-timeout` flag value. |
| `workflow_start_capture_follow_reads_header` (1.12 s in `tests/full_scope.rs`) | Same root cause — uses the second-granularity follow timeout via the CLI. | After the API change above, this test can pass `--idle-timeout-ms 100` (new CLI flag, gated behind `#[cfg(test)]` or always-on with the original flag preserved for users) or be rewritten to construct the follower directly. |
| `dispatch_start_passes_capture_path_to_daemon` (0.61 s in `tests/cli_args.rs`) | Currently invokes the real daemon path which spawns the binary. | Inspect the test — if it can stub the daemon `start` call rather than running it, it drops to milliseconds. (Verify before changing; may already be a binary spawn that can't be avoided.) |

**Rough yield**: ~14 s removed from the slow-test budget, gained back at the
expense of one signature change in `src/cli.rs::print_follow_to`.

### Category B — Inherently slow (mark as slow)

These tests deliberately wait for real-time wall-clock events that virtual
time cannot replace. The session-supervisor authors specifically called out
that `tokio::time::pause()` deadlocks under wiremock with multi-threading,
documented in `docs/plans/done/STEP-07-auth-and-rest.md` §20.1 and the
comment block in `crates/zwift-api/tests/auth.rs:370`. The fixed
`Duration::from_millis(4500)` waits in `tests/relay_runtime.rs` are
calibrated to specific supervisor timing; shrinking them risks flakes.

| Tests                                              | Reason                                                              | Sleep observed                                  |
| -------------------------------------------------- | ------------------------------------------------------------------- | ----------------------------------------------- |
| `crates/zwift-relay/tests/session.rs::supervisor_*` (8 tests, ~24 s) | 60 s expiration × 0.05 refresh fraction = 3 s real wait. The factory `fast_relay_config_for` already minimizes this. | 3 s scheduled real time per test                |
| `tests/relay_runtime.rs::start_with_writer_*` (2 tests, ~9 s) | Hard-coded `tokio::time::sleep(Duration::from_millis(4500))` waiting for supervisor refresh. | 4.5 s explicit                                  |
| `crates/zwift-api/tests/auth.rs::preemptive_refresh_fires_at_half_expires_in` (2 s) | 2 s real wait; virtual time deadlocks with wiremock per the test's own comment. | 2 s explicit                                    |
| `tests/relay_runtime.rs::login_http_exchange_appears_in_capture` (1.5 s) | Real wiremock + capture-file flush sequence. | sub-second sleeps that compound                 |
| `tests/relay_runtime.rs::start_all_inner_writes_udp_outbound_to_capture_file` (1.2 s) | Capture-flush + UDP-establish sequence. | sub-second sleeps                                |
| `crates/zwift-relay/tests/capture.rs::follower_reads_records_as_they_are_written` (3.5 s) | Writer pushes 10 records at 50 ms cadence + follower idle timeout. The 50 ms cadence matters because the test exists to verify the follower observes records *as they are written*; collapsing the cadence collapses the test. | 50 ms × 10 + idle timeout                       |
| `crates/zwift-relay/tests/capture.rs::follower_with_poll_interval_respects_setting` (1.1 s) | Verifies a 5 ms `poll_interval` against a 25 ms inter-record gap; needs real time. | ~1 s sequenced waits                            |

**Total Category B**: ~42 s. These should be marked as slow tests and
skipped from the default `cargo test`.

### Category C — Pseudo-slow due to a real bug or accidental cost

The five 5-second logging tests and the 5-second daemon-lifecycle test
are suspicious. Manual reproduction of the same start/stop sequence
through the CLI takes ~250 ms. The tests pay 5 s consistently, and 5 s
matches `SHUTDOWN_WAIT` in `src/daemon/runtime.rs:23`.

| Tests                                                                | Symptom                                                              |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `tests/logging.rs::default_silences_info_on_stderr` (5.10 s)         | Manual repro of identical operations: 0.26 s. Test: 5.10 s.          |
| `tests/logging.rs::verbose_flag_emits_startup_info_to_stderr` (5.10 s) | Same.                                                                |
| `tests/logging.rs::debug_flag_emits_control_debug_to_stderr` (5.11 s)  | Same.                                                                |
| `tests/logging.rs::rust_log_env_overrides_default_filter` (5.09 s)   | Same.                                                                |
| `tests/daemon_lifecycle.rs::stop_clears_pid_file_and_status_reports_shutdown` (5.10 s) | Same.                                                                |

The shape strongly suggests `child.wait_with_output()` is the slow point
— the daemon process exits but its stdout/stderr `Stdio::piped()` pipes
are not closing for ~5 s, so the parent reading them blocks. Likely
culprits, in priority order:

1. **`tracing-appender` non-blocking writer guard drop**. The
   `WorkerGuard` returned by `tracing_appender::non_blocking` waits for
   its background thread to drain on drop, and that drop happens late
   in `start()` (after `run_daemon` returns). If the worker is blocked
   on a write to stderr, the guard takes time to flush.
2. **Tokio multi-thread runtime drop**. `rt.block_on(run_daemon(...))`
   in `src/daemon/runtime.rs:69` returns; the `rt` then drops. The
   multi-thread runtime's drop joins worker threads. If a stray
   spawned task (signal handler, listener accept) holds an `Arc`
   that owns a stderr writer, that thread waits.
3. **A child process inherited by the daemon** keeping the pipe alive.
   With `relay.enabled = false` no relay subprocess exists; not the
   suspect here.

Whichever it is, fixing the root cause moves these five tests from ~5 s
each to ~250 ms each — a 24 s drop in the slow-test budget.

**Recommendation for Category C**: mark as slow now (so `cargo test`
returns quickly), and open a follow-up to diagnose why the daemon
holds its stderr open for 5 s after `run_daemon` returns. The test
authors are unlikely to tolerate the slowness once the marker scheme
is in place; it's the slowness that's hiding the bug.

### Category D — Tests just over the 100 ms threshold

The remaining ~52 tests in the 100–500 ms range are generally
unavoidable: each one stands up a small piece of network, capture, or
relay machinery. Examples:

- `crates/zwift-relay/tests/udp.rs::establish_converges_after_six_replies_and_emits_established` (160 ms): the `establish` loop waits `10 × hello_idx` ms between hellos (`crates/zwift-relay/src/udp.rs:289`), so 6 hellos = 210 ms minimum. Authentic to the protocol; not worth shaving.
- `tests/daemon_lifecycle.rs::start_exits_nonzero_*` (~850 ms × 11): spawn the binary, run validation, exit. Binary cold-start in debug profile is ~700 ms; this is mostly process launch cost.
- `crates/zwift-relay/tests/udp.rs::watchdog_fires_after_silence` (200 ms): waits for the watchdog timer.

Of these, the **11 `tests/daemon_lifecycle.rs::start_exits_*` validation
tests (~9 s)** are the most attractive secondary target. Each spawns the
binary just to assert that a missing-credential or missing-directory
condition triggers a non-zero exit. That logic lives in
`src/daemon/validate.rs` and could be exercised directly by unit tests
on `validate_startup`, returning typed errors. The integration tests
would still exist (one per failure mode is enough as a smoke check)
but the bulk could move into the in-process unit test suite. A separate
plan covers that refactor; for now the plan here treats them as
"inherent" and marks them slow.

The Category D tests below 200 ms are kept in the default `cargo test`
run. They contribute ~10 s in aggregate but parallelize across binaries.

## Decisions

These were resolved in the planning conversation:

1. **Scope of marking**: only mark tests that actually exceed the
   100 ms threshold. Tests in `tests/daemon_lifecycle.rs` that run in
   ~64 ms (e.g., `start_writes_pid_file_and_status_reports_running`)
   stay in the default suite — they are not slow and need no marker.
2. **Category C investigation**: the suspected
   `tracing-appender` / runtime-drop bug is in scope. Investigate after
   the quick-win marking lands. Fixing it benefits real daemon shutdown
   latency, not just test wall clock.
3. **Marker mechanism**: start with `#[ignore = "slow: …"]`. If a
   future need for tiers (e.g., `network-tests`) appears, refactor to
   a cargo feature later.

## Marking scheme

`#[ignore]` is the standard Rust mechanism. Marked tests:

- are **skipped** by default `cargo test`
- run with `cargo test -- --ignored` (slow tests only)
- run with `cargo test -- --include-ignored` (everything)

`#[ignore = "<reason>"]` accepts a string slot that surfaces in the
test output, making the gating self-documenting.

```rust
#[test]
#[ignore = "slow: real-time supervisor refresh; see docs/plans/mark-slow-tests.md"]
fn supervisor_refresh_fires_at_configured_fraction_of_expiration() { ... }
```

A short README / `docs/development.md` note documents the convention
for future contributors:

> Tests carrying `#[ignore = "slow: …"]` are skipped by default. Run
> them via `cargo test -- --ignored` (slow only) or
> `cargo test -- --include-ignored` (all). CI runs the latter.

## Implementation plan

The order is **mark first, optimize second, investigate third**. Marking
is a mechanical edit per test: it produces an immediate quick win
without changing test logic, frees up future `cargo test` runs from the
slow tail, and lets the deeper work (follower API change, Category C
diagnosis) happen against a fast baseline.

### Step 1 — Mark Category B tests as slow (quick win, ~42 s)

Add `#[ignore = "slow: <one-line reason>"]` to each test below. The
reason string should be specific to the test's wait so a future reader
knows why it is gated.

- `crates/zwift-relay/tests/session.rs` (8 tests):
  - `supervisor_refresh_fires_at_configured_fraction_of_expiration`
  - `supervisor_refresh_failure_triggers_relogin`
  - `supervisor_relogin_failure_emits_login_failed_with_attempt_count`
  - `supervisor_shutdown_cancels_pending_refresh`
  - `supervisor_refresh_fire_emits_scheduled_delay_event`
  - `supervisor_refresh_failure_path_emits_refresh_failed_and_relogin_attempt`
  - `supervisor_relogin_success_emits_relogin_ok`
  - `supervisor_persistent_login_failure_emits_login_failed_warn`

  Suggested reason: `slow: 3 s real-time supervisor refresh; virtual time deadlocks under wiremock multi-thread (see STEP-07 §20.1)`.

- `tests/relay_runtime.rs` (4 tests):
  - `start_with_writer_subscribes_to_real_supervisor_events`
  - `start_with_writer_records_fresh_manifest_on_supervisor_relogin`
  - `login_http_exchange_appears_in_capture`
  - `start_all_inner_writes_udp_outbound_to_capture_file`

  Suggested reason: `slow: hard-coded 4.5 s sleep waiting for supervisor refresh / capture flush`.

- `crates/zwift-api/tests/auth.rs` (1 test):
  - `preemptive_refresh_fires_at_half_expires_in`

  Suggested reason: `slow: 2 s real wait; virtual time deadlocks under wiremock (per the comment block above the test)`.

- `crates/zwift-relay/tests/capture.rs` (2 tests):
  - `follower_reads_records_as_they_are_written`
  - `follower_with_poll_interval_respects_setting`

  Suggested reason: `slow: follower observes records over real-time write cadence; collapsing the cadence collapses the test`.

### Step 2 — Mark Category C tests as slow (quick win, ~25 s)

Apply the marker to the five 5 s tests. Their slowness is suspicious
(see Category C analysis above) and Step 6 below will diagnose the
root cause; for now, gate them so the rest of the suite runs fast.

- `tests/logging.rs` (4 tests):
  - `default_silences_info_on_stderr`
  - `verbose_flag_emits_startup_info_to_stderr`
  - `debug_flag_emits_control_debug_to_stderr`
  - `rust_log_env_overrides_default_filter`

- `tests/daemon_lifecycle.rs` (1 test):
  - `stop_clears_pid_file_and_status_reports_shutdown`

Reason string: `slow: foreground daemon teardown holds piped stderr open ~5 s; suspected tracing-appender / runtime drop bug, see docs/plans/mark-slow-tests.md §Category C`.

### Step 3 — Mark the daemon-lifecycle outliers as slow (quick win, ~14 s)

Mark the **17** `tests/daemon_lifecycle.rs` tests in the slow inventory
above (the ones with elapsed > 100 ms in the timing table). Do **not**
touch the file's faster tests (e.g.,
`start_writes_pid_file_and_status_reports_running` at 64 ms,
`stop_when_not_running_reports_no_daemon`,
`status_when_not_running_reports_no_daemon`) — they keep the integration
path covered by default `cargo test`.

Specific tests to mark in `tests/daemon_lifecycle.rs`:

- `start_canonicalizes_relative_capture_path`
- `capture_file_handle_survives_fork`
- `start_when_already_running_refuses`
- `backgrounded_start_returns_quickly_and_keeps_running`
- `stale_pid_file_is_cleaned_up_on_start`
- `debug_flag_keeps_process_in_foreground`
- `start_exits_nonzero_when_log_directory_missing`
- `start_removes_socket_when_relay_start_fails`
- `start_exits_nonzero_when_pidfile_directory_missing`
- `start_exits_nonzero_and_prints_error_when_password_missing`
- `start_does_not_write_socket_when_validation_fails`
- `start_does_not_write_pidfile_when_validation_fails`
- `start_exits_nonzero_and_prints_error_when_email_missing`
- `start_removes_pidfile_when_relay_start_fails`
- `start_exits_nonzero_when_relay_start_fails`
- `start_exits_nonzero_when_capture_path_not_openable`
- `dispatch_start_passes_capture_path_to_daemon` (lives in
  `tests/cli_args.rs`, not `daemon_lifecycle.rs`, but matches the
  same pattern: 0.61 s spent spawning the binary).

Reason string: `slow: spawns the ranchero binary; ~700 ms cold-start in debug profile`.

### Step 4 — Document the convention (quick win)

Add a "Running tests" section to the project README (or
`docs/development.md` if expanding the README is undesirable) with the
three commands and a one-line explanation of the `#[ignore = "slow: …"]`
convention. Confirm CI runs `cargo test -- --include-ignored` so slow
coverage is not silently lost.

This step is what turns the marker from a private convention into a
durable project rule.

### Step 5 — Run the verification baseline

Re-run `cargo test` and `cargo test -- --include-ignored` after Steps
1–4 land. Confirm:

- Default `cargo test` finishes in ≲20 s wall clock.
- `cargo test -- --include-ignored` matches today's count of 678
  passing tests (i.e., nothing was ignored that should have run by
  default).
- `cargo test -- --ignored` runs only the marked tests and passes.

This step is the fast feedback loop for the deeper work in Steps 6
and 7.

### Step 6 — Optimize `print_follow_to` for sub-second idle timeouts (~13 s)

Goal: drop the 12 `follow_*` tests and
`workflow_start_capture_follow_reads_header` from the slow set without
having to mark them.

1. In `src/cli.rs::print_follow_to`, change the parameter from
   `idle_timeout_secs: Option<u64>` to `idle_timeout: Option<Duration>`.
   The body already calls `Duration::from_secs(secs)` — replace with the
   passed `Duration`.
2. Update the CLI dispatch site (the only production caller) to keep
   converting `--idle-timeout` (seconds) to `Duration::from_secs(n)`.
   The user-facing flag stays as seconds; only the test-facing API
   changes.
3. Update `tests/follow.rs::run_follow` to accept `Option<Duration>` and
   pass `Some(Duration::from_millis(100))` from each test that currently
   uses `Some(1)`. Adjust the elapsed-time assertion in
   `follow_returns_within_idle_timeout_when_no_records_arrive` from
   800 ms / 2.5 s to ~100 ms / ~500 ms.
4. For `tests/full_scope.rs::workflow_start_capture_follow_reads_header`,
   either pass a sub-second timeout via the new API or accept the 1 s
   cost as part of the broader integration shape `full_scope.rs` covers.
   Decide on inspection.

Optional clean-up: with the follow tests no longer slow they don't need
the marker, so this step also lets you delete the markers Step 1
didn't apply (the `follow_*` tests aren't in Step 1 — confirm before
deleting).

### Step 7 — Investigate Category C root cause

The five 5 s tests pay almost exactly `SHUTDOWN_WAIT` (5 s in
`src/daemon/runtime.rs:23`). Manual reproduction of the same start /
stop sequence runs in 0.26 s, so the slowness is specific to the
test-harness pattern of capturing the foreground daemon's piped stderr
via `Stdio::piped()` and then calling `child.wait_with_output()`.

Diagnostic plan, in order of likelihood:

1. **Confirm the source of the wait** by adding `Instant::now()` probes
   inside one of the affected tests around `wait_for_pidfile_gone()`
   and `child.wait_with_output()`. The expectation is that
   `wait_with_output` is the slow call.
2. **Bisect the daemon's exit path**. The order in
   `src/daemon/runtime.rs::start()` is:
   - `rt.block_on(run_daemon(...))` returns
   - `tracing::info!("ranchero stopped")`
   - `pidfile.remove()`, `remove_file(socket)`
   - function returns; `_log_guard` drops; `rt` drops
   - `start()` returns to `main()`; process exits and OS closes stdio.

   Add a probe after each of these to identify which step holds the
   stderr pipe open.
3. **Most likely culprit**: the `WorkerGuard` returned by
   `tracing_appender::non_blocking` waits on its background thread on
   drop. If that thread is blocked or if `_log_guard` ends up dropped
   late in the function's stack-unwind order, drop can take seconds.
   Try moving `_log_guard` out of `start()` into the caller in
   `main.rs` (or explicitly `drop(_log_guard)` before
   `tracing::info!("ranchero stopped")` — though that re-orders the
   tracing message order and may need a different fix).
4. **Secondary culprit**: `tokio::runtime::Builder::new_multi_thread()`
   drops by joining its workers. If a spawned task (signal handler,
   listener) holds an `Arc` that owns a writer, the worker waits.
   Inspect the spawned tasks in `run_daemon` for ones that don't
   abort on the shutdown channel.

Acceptance: the five Category C tests run in <500 ms each. Once
confirmed, remove their `#[ignore]` markers added in Step 2.

This step is the only one that fixes a real production issue (slow
daemon shutdown when stdio is captured); the others purely shape the
test suite.

## Expected outcome

Cumulative slow-test time before plan: **104.5 s** (84 tests).

| Step                                 | Tests removed from default | Time removed |
| ------------------------------------ | -------------------------- | ------------ |
| 1. Category B markers                | 15                         | ~42 s        |
| 2. Category C markers                | 5                          | ~25 s        |
| 3. daemon_lifecycle markers          | 17                         | ~14 s        |
| **Steps 1–3 (the quick-win subtotal)** | **37**                   | **~81 s**    |
| 6. follow API change                 | 13                         | ~13 s        |
| 7. Category C fix (un-mark)          | -5                         | (already removed in Step 2; net effect is restoring those 5 tests to the default suite while keeping them <500 ms each) |
| **Total reduction in slow-test time** | **50**                    | **~94 s**    |

After Steps 1–3 alone, default `cargo test` should finish in ≲20 s wall
clock. Steps 6 and 7 then trim further and fix a real bug.

`cargo test -- --ignored` runs the slow set in ~70 s after Step 3,
bounded by the slowest binary's serialization. Once Step 7 lands, the
five Category C tests no longer need the marker, so the slow set
shrinks accordingly.
