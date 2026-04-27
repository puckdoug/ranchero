# Step 20 — Additional considerations (parking lot)

## Purpose

A running list of "think about later" items that surface during
earlier-step work but don't justify pausing the current step to
resolve. Each entry should be self-contained: where it came from, what
the trade-off looks like, what to revisit when.

Triage when starting any new step: any item here that the new step
naturally touches gets pulled into that step's elaboration. Items left
behind here are either accepted or revisited at the very end of the
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
to make progress — but on a `current_thread` runtime the reactor only
turns when the runtime parks, and the test task was busy yielding (so
the runtime never parks).

**Current resolution.** The test uses a 2 s `expires_in` (1 s
half-life) and a real `tokio::time::sleep(Duration::from_millis(2000))`.
~2 s of wall time on the suite, no virtual-time machinery. Comment in
the test explains it.

**Why this might come back.** Subsequent steps add more
time-driven background tasks against mock HTTP servers:

- STEP 09 — relay session refresh at ~90% of session lifetime.
- STEP 10 / 11 — UDP/TCP channel watchdogs (>30 s silent → reconnect),
  exponential backoff on reconnect.
- STEP 12 — `GameMonitor` supervision and reconnect cadence.

If we end up with several real-time waits adding up to a noticeable
suite slowdown — say >5 s aggregate — it's worth revisiting. Options:

1. **`flavor = "multi_thread"` + manual `tokio::time::pause()`** after
   the mock server is up. The IO driver runs on a worker thread, so
   reqwest can make progress while the test task yields. Cost: a
   `std::time::Instant` deadline loop in the test (since `tokio::time`
   is paused), which is ugly.
2. **Inject the clock + sleeper.** A `trait Clock` / `trait Sleeper`
   abstraction in `zwift-api` (and any other crate that schedules
   work) lets tests substitute a deterministic fake — no real sleeps,
   no virtual-time + IO interaction. Cost: an extra abstraction layer
   in production code paid for by every consumer of the crate.
3. **Status quo.** Accept short real-time sleeps as the price of
   testing time-driven behavior end-to-end through real `reqwest` +
   wiremock. Cost: suite gets a few seconds slower per such test.

**Decision rule.** Revisit when (a) total real-time test wait crosses
~5 s, or (b) a flake appears tied to scheduling jitter on CI. Until
then, status quo.

### 20.2 — Shared inbound-decode helper between UDP and TCP channels (from STEP 11)

**Where it came from.** STEP 11's plan recommended extracting
`process_inbound` (header decode → relay_id validation → IV state
mutation → AES-128-GCM-4 decrypt → `ServerToClient::decode`) into
a private module shared by `udp.rs` and `tcp.rs`. The two copies of
the function differ only in one constant — `ChannelType::UdpServer`
vs `ChannelType::TcpServer` in the IV construction.

**Current resolution.** Did not extract. Two near-identical copies
of `process_inbound` live in `crates/zwift-relay/src/udp.rs` and
`crates/zwift-relay/src/tcp.rs`. A shared helper parameterized on
channel type would add one indirection (passing the channel type as
a parameter, or generic) for one line of difference; not worth it
yet.

**Why this might come back.**

- A third channel type appears (companion-app reverse channel is
  spec §6 out-of-scope today, but listed there).
- The two copies start to diverge — e.g. one channel adds inbound
  envelope handling, error retry, metrics counters, or trace spans
  the other doesn't need. At that point either the divergence is
  real and the helper would have hidden it, or the divergence is a
  bug introduced by editing one copy and forgetting the other.
- A reviewer flags the duplication as a smell.

**Decision rule.** Extract when (a) the two copies have diverged
beyond the `ChannelType` constant in a way that would have been
caught by a shared helper, or (b) a third channel type lands. Until
then, the duplication is the cheaper choice.

---

## How to use this file

When you hit a "this is fine for now but worth thinking about later"
moment in a step:

- Add a numbered subsection under **Open items** (`20.N — short
  title`).
- State where it came from, the current resolution, why it might come
  back, and a decision rule for when to revisit. Keep it short —
  parking-lot entries should be skimmable in a minute.
- When an item is resolved or pulled into a real step, move it to a
  **Resolved** section at the bottom (or delete it if the resolution
  was "we decided to keep doing what we're doing").
