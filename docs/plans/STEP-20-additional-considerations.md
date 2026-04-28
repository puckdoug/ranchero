# Step 20 — Additional considerations (parking lot)

## Purpose

A running list of items to consider later. These items surface during
earlier-step work but do not justify pausing the current step to
resolve. Each entry should be self-contained: where it came from, what
the trade-off looks like, and when to revisit.

Triage when starting any new step: any item here that the new step
naturally touches gets pulled into that step's elaboration. Items left
behind here are either accepted or revisited at the end of the
porting effort.

---

## Open items

### 20.1 — Virtual-time vs. real-time in async HTTP tests (from STEP 07)

**Where it came from.** The
`preemptive_refresh_fires_at_half_expires_in` test in
`crates/zwift-api/tests/auth.rs` originally used
`#[tokio::test(flavor = "current_thread", start_paused = true)]` plus
`tokio::time::advance(...)` so the half-life elapsed in virtual time
without a real-world wait. It deadlocked: after the scheduled
`tokio::time::sleep(expires_in / 2)` woke, the spawned refresh task
issued a `reqwest` round-trip to wiremock, which needs the IO driver
to make progress; however, on a `current_thread` runtime the reactor
only turns when the runtime parks, and the test task was busy
yielding, so the runtime never parked.

**Current resolution.** The test uses a 2 s `expires_in` (1 s
half-life) and a real `tokio::time::sleep(Duration::from_millis(2000))`.
This adds approximately 2 s of wall time to the suite and uses no
virtual-time machinery. A comment in the test explains the choice.

**Why this might come back.** Subsequent steps add more
time-driven background tasks against mock HTTP servers:

- STEP 09 — relay session refresh at ~90% of session lifetime.
- STEP 10 / 11 — UDP/TCP channel watchdogs (>30 s silent → reconnect),
  exponential backoff on reconnect.
- STEP 12 — `GameMonitor` supervision and reconnect cadence.

If several real-time waits accumulate to a noticeable suite slowdown
(for example, more than 5 s aggregate), revisiting is warranted.
Options:

1. **`flavor = "multi_thread"` + manual `tokio::time::pause()`** after
   the mock server is up. The IO driver runs on a worker thread, so
   reqwest can make progress while the test task yields. Cost: a
   `std::time::Instant` deadline loop in the test (since `tokio::time`
   is paused), which is awkward.
2. **Inject the clock and sleeper.** A `trait Clock` / `trait Sleeper`
   abstraction in `zwift-api` (and any other crate that schedules
   work) would let tests substitute a deterministic in-memory
   implementation, with no real sleeps and no interaction between
   virtual time and IO. Cost: an extra abstraction layer in
   production code, paid for by every consumer of the crate.
3. **Status quo.** Accept short real-time sleeps as the cost of
   testing time-driven behavior end-to-end through real `reqwest`
   and wiremock. Cost: the suite is a few seconds slower per such
   test.

**Decision rule.** Revisit when (a) total real-time test wait crosses
approximately 5 s, or (b) a flaky failure appears tied to scheduling
jitter on CI. Until then, the status quo is retained.

### 20.2 — Shared inbound-decode helper between UDP and TCP channels (from STEP 11)

**Where it came from.** STEP 11's plan recommended extracting
`process_inbound` (header decode → relay_id validation → IV state
mutation → AES-128-GCM-4 decrypt → `ServerToClient::decode`) into
a private module shared by `udp.rs` and `tcp.rs`. The two copies of
the function differ only in one constant: `ChannelType::UdpServer`
versus `ChannelType::TcpServer` in the IV construction.

**Current resolution.** The function was not extracted. Two
near-identical copies of `process_inbound` reside in
`crates/zwift-relay/src/udp.rs` and `crates/zwift-relay/src/tcp.rs`.
A shared helper parameterized on channel type would add one
indirection (passing the channel type as a parameter, or as a
generic) for one line of difference; this provides little value at
this step.

**Why this might come back.**

- A third channel type appears (the companion-app reverse channel is
  spec §6 out-of-scope today, but is listed there).
- The two copies begin to diverge; for example, one channel adds
  inbound envelope handling, error retry, metrics counters, or trace
  spans that the other does not need. At that point, either the
  divergence is real and the helper would have hidden it, or the
  divergence is a defect introduced by editing one copy and
  forgetting the other.
- A reviewer identifies the duplication as a code smell.

**Decision rule.** Extract when (a) the two copies have diverged
beyond the `ChannelType` constant in a way that would have been
caught by a shared helper, or (b) a third channel type is
implemented. Until then, the duplication is the lower-cost choice.

---

## How to use this file

When a step encounters a decision that is acceptable in this version
but worth revisiting later:

- Add a numbered subsection under **Open items** (`20.N — short
  title`).
- State where it came from, the current resolution, why it might come
  back, and a decision rule for when to revisit. Keep it concise:
  parking-lot entries should be readable within a minute.
- When an item is resolved or pulled into a step, move it to a
  **Resolved** section at the bottom, or delete it if the resolution
  was to retain the current approach.
