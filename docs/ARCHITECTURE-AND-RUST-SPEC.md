# Sauce for Zwift — Architecture Review & Rust Reimplementation Specification

> Scope: this document describes the **live-data core** of Sauce for Zwift — the
> parts that log into Zwift, pull the live game stream, compute metrics, and
> publish them to widgets and browser clients. It then gives a concrete Rust
> specification for reimplementing that core.
>
> Repository reviewed: `/Users/doug/Development/Zwift/sauce4zwift`, version
> `2.3.0-alpha.0` (branch `main`). File/line references are against that tree.

---

## 1. Executive summary

Sauce for Zwift is an Electron desktop app that connects to Zwift's relay
infrastructure as an independent client, receives the same live telemetry
stream that Zwift's own game client sees, and re-publishes that stream
(plus heavy derived analytics) to:

- Local overlay "widget" windows (Electron `BrowserWindow`s).
- External browser clients over a local HTTP + WebSocket server (default
  port 1080).
- A user-installable mod/plugin system.

Two Zwift accounts are used:

1. **Main account** — the account the user rides under (read/write, emits
   ride-ons, chat, etc.).
2. **Monitor account** — a second account used purely to receive the live
   relay stream without requiring the app to impersonate the rider. Telemetry
   is scoped by the *watched* athlete, not by the logged-in account.

The live protocol is **not** a passive sniff of the game client's traffic.
Sauce for Zwift joins the relay mesh itself, over **TCP/3025** and
**UDP/3024** (secure variants), exchanging **AES-128-GCM** encrypted
protobuf messages whose key is chosen by the client at login.

There is no pcap, no MITM, no port-forwarding.

---

## 2. High-level architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                        Electron main process                         │
│                                                                      │
│   ┌───────────────┐   ┌──────────────────┐   ┌────────────────────┐  │
│   │  ZwiftAPI ×2  │──▶│   GameMonitor    │──▶│   StatsProcessor   │  │
│   │ (main+monitor)│   │  (TCP+UDP relay) │   │  (4.6k LOC engine) │  │
│   └───────────────┘   └──────────────────┘   └─────────┬──────────┘  │
│          │                                             │             │
│          ▼                                             ▼             │
│   ┌───────────────┐   ┌──────────────────┐   ┌────────────────────┐  │
│   │   Keytar      │   │  RPC / Event bus │◀──│  EventEmitter map  │  │
│   │  (secrets)    │   │                  │   │  stats|windows|... │  │
│   └───────────────┘   └────┬─────────────┘   └────────────────────┘  │
│                            │                                         │
│              ┌─────────────┼──────────────┐                          │
│              ▼             ▼              ▼                          │
│       ┌────────────┐ ┌───────────┐ ┌────────────┐                    │
│       │ IPC +      │ │ Web server│ │  SQLite    │                    │
│       │ MessagePort│ │ Express+WS│ │ (storage,  │                    │
│       │ to widgets │ │  port 1080│ │ athletes,  │                    │
│       └─────┬──────┘ └─────┬─────┘ │ segments)  │                    │
│             │              │       └────────────┘                    │
└─────────────┼──────────────┼───────────────────────────────────────  ┘
              ▼              ▼
    ┌──────────────┐   ┌─────────────────┐
    │  Renderer    │   │ External browser│
    │  BrowserWin  │   │ (any client)    │
    │  widgets     │   │                 │
    └──────────────┘   └─────────────────┘
```

### 2.1 Top-level source layout

| Path | LOC | Role |
|---|---:|---|
| `src/loader.js` | 267 | Electron bootstrap (single-instance, Sentry, GPU/macOS checks, headless spawn). |
| `src/main.mjs` | 814 | Main-process entry: wall-clock sync, auth, window/startup orchestration. |
| `src/app.mjs` | 369 | `SauceApp` base class — wires `GameMonitor` → `StatsProcessor` → web server → RPC. |
| `src/zwift.mjs` | 3001 | REST `ZwiftAPI` + `GameMonitor` (TCP/UDP relay client) + `GameConnectionServer` (companion). |
| `src/zwift.proto` | 2079 | Protobuf schema for the full Zwift relay protocol. |
| `src/stats.mjs` | 4606 | `StatsProcessor` — rolling windows, NP/TSS/W', groups, segments, laps. |
| `src/windows.mjs` | 2105 | Widget-window profile system, overlay windows, manifest loader. |
| `src/webserver.mjs` | 573 | Express + WebSocket surface for external clients. |
| `src/rpc.mjs` | 80 | `handlers` registry, `subscribe`/`unsubscribe` abstraction over event emitters. |
| `src/mods.mjs` + `mods-core.mjs` | ~660 | Third-party mod discovery (Documents/SauceMods, ZIPs), validation, injection. |
| `src/storage.mjs`, `src/db.mjs` | ~120 | Key-value SQLite store, WAL mode. |
| `src/secrets.mjs` | 28 | Keytar wrapper (OS keychain). |
| `src/headless.mjs` | 147 | Node.js-only entry point (no UI). |
| `src/preload/*.js` | ~400 | Renderer preload bridge (IPC, MessagePort, CSS/JS injection). |
| `shared/sauce/*.mjs` | ~6 files | Sport-science primitives (`RollingAverage`, NP, TSS, zones, haversine). |
| `shared/routes.mjs`, `shared/curves.mjs` | — | Zwift world/route geometry tables and path interpolation. |

### 2.2 Process model

- **Main process** (Electron). Owns *everything* that talks to Zwift and
  everything that holds state. Single Node.js event loop.
- **Renderer processes** — one Chromium `BrowserWindow` per widget. They
  subscribe over IPC + `MessagePort` and render DOM/canvas. They **cannot**
  read the Zwift stream directly.
- **Web server** — runs inside the main process (not a separate process),
  `express` for REST and static files, `ws` for a single streaming
  WebSocket endpoint. Default bind is `localhost:1080`.
- **Headless mode** — `electron . --headless` re-spawns Node with
  `ELECTRON_RUN_AS_NODE=1` (loader.js:229-251) and runs `headless.mjs`. The
  stats processor and web server run; windowing stubs out. This is the
  mode a Rust reimplementation most directly replaces.

### 2.3 Initialization order (GUI mode)

1. `loader.js` takes the single-instance lock, installs Sentry, chooses
   log destinations, and calls `app.whenReady()`.
2. `main.mjs` constructs a `RobustRealTimeClock` (main.mjs:42-108) that
   runs a multi-sample network time sync so scheduled work and
   `worldTime` remain sane even if the OS clock drifts.
3. Two `ZwiftAPI` instances are constructed — main and monitor — sharing
   the clock.
4. `ElectronSauceApp` is created; credentials are pulled from keytar or a
   login dialog is shown. Both accounts authenticate.
5. `SauceApp.start()` (app.mjs:234-346) constructs `GameMonitor`,
   optionally `GameConnectionServer`, then `StatsProcessor`, registers
   ~50 RPC handlers, and starts the web server.
6. Windows open from the user's saved widget profile. Hotkeys bind.
7. Each renderer calls back via IPC to `subscribe` to event streams it
   cares about; the main process attaches a listener to the named
   `EventEmitter` in `rpcEventEmitters` and forwards emits through a
   dedicated `MessagePort`.

Headless mode skips steps 6-7 and reads credentials from CLI args.

---

## 3. Authentication and login

All of the following is in `src/zwift.mjs` (`ZwiftAPI` class) and
`src/secrets.mjs`.

### 3.1 OAuth2 password grant

Zwift's auth is Keycloak at
`https://secure.zwift.com/auth/realms/zwift/protocol/openid-connect/token`.

**Initial login** (`ZwiftAPI.authenticate`, zwift.mjs:340-363):

```
POST https://secure.zwift.com/auth/realms/zwift/protocol/openid-connect/token
Content-Type: application/x-www-form-urlencoded

client_id=Zwift Game Client
grant_type=password
username=<email>
password=<password>
```

Note the `client_id` literally contains a space. It is not URL-encoded in
the Keycloak form — it is the literal identifier Zwift's game client uses,
and is sent through `URLSearchParams` which encodes it as `Zwift+Game+Client`.

**Successful response** is standard Keycloak JSON:

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "expires_in": 3600,
  "refresh_expires_in": 86400,
  "token_type": "Bearer"
}
```

**Token refresh** (`ZwiftAPI._refreshToken`, zwift.mjs:381-398):

```
POST .../openid-connect/token
Content-Type: application/x-www-form-urlencoded

client_id=Zwift Game Client
grant_type=refresh_token
refresh_token=<refresh_token>
```

Refresh is pre-emptively scheduled at **50%** of `expires_in`
(zwift.mjs:361, 397) so a session never expires passively. A 401 on any
API call also triggers an inline refresh (`ZwiftAPI.fetch`,
zwift.mjs:424-437; 401 retry at ~483-487).

There is no captcha or MFA support. Accounts with MFA enabled cannot
currently log in to Sauce.

### 3.2 Credential storage

`src/secrets.mjs` (28 lines) wraps `keytar` with service name
`"Zwift Credentials - Sauce for Zwift"` and stores JSON under two account
keys — one for the main account, one for the monitor account. Values are
whatever OS-native secure store keytar maps to: macOS Keychain, Windows
Credential Manager, or libsecret on Linux.

Tokens themselves are kept in memory on `ZwiftAPI` instances; only
`{username, password}` is persisted. Re-auth happens on every app start.

### 3.3 REST endpoints used post-auth

All authenticated requests go to the Zwift game API host
`us-or-rly101.zwift.com` (zwift.mjs:468) over HTTPS with
`Authorization: Bearer <access_token>`. Headers also include
`Source: Sauce for Zwift` and `User-Agent: CNL/4.2.0 (...)` consistent with
a Zwift game client.

Observed endpoints (non-exhaustive):

| Endpoint | Purpose |
|---|---|
| `/api/profiles/me` | Fetch own athlete profile. |
| `/api/profiles/{id}` | Fetch another athlete's profile (protobuf). |
| `/api/profiles/{id}/activities` | List activities. |
| `/api/events/search` | Event discovery. |
| `/api/segment-results` | Segment leaderboards. |
| `/api/notifications` | Notifications. |
| `/relay/profiles/me/phone` | Register companion app session. |
| `/relay/worlds/{worldId}/players/{id}` | Poll a single player state. |
| `/relay/worlds/{worldId}/attributes` | Push world updates. |
| `/relay/session/refresh` | Extend relay session lifetime. |
| `/relay/dropin` | Public drop-in world list. |
| `/relay/tcp-config` | TCP relay server list (sometimes cached from login). |
| `/api/users/login` | **Relay** login (protobuf, see §4.1). |

REST and relay endpoints share the same auth token.

---

## 4. Relay session and live data stream

This is the heart of the app and the part a Rust port needs to match
byte-for-byte with the Zwift server. All of it lives in `zwift.mjs`
(classes `GameMonitor`, `NetChannel`, `TCPChannel`, `UDPChannel`,
`RelayIV`).

### 4.1 Bootstrap: protobuf login over HTTPS

`GameMonitor.login()` (zwift.mjs:1633-1658):

1. Client generates a random **16-byte AES key**:
   `Crypto.randomBytes(16)`.
2. `POST https://us-or-rly101.zwift.com/api/users/login` with body
   `protos.LoginRequest.encode({ aesKey }).finish()` and
   `Content-Type: application/x-protobuf-lite`. (Auth header is the
   normal Bearer token.)
3. Response body is a `protos.LoginResponse` protobuf:

   ```
   message LoginResponse {
       string sessionState  = 1;
       RelaySession session = 2;   // contains tcpConfig.servers[] and server time
       int32  relaySessionId = 3;
       int32  expiration     = 4;  // minutes
       ...
   }
   ```

   The client keeps **`aesKey`** (self-generated) and **`relaySessionId`**.

### 4.2 Session state object

From step 2 onward, every channel shares one "session" object:

```
{
  aesKey:          Buffer(16),    // client-chosen, never transmitted after login
  relayId:         u32,           // = relaySessionId from LoginResponse
  expiration:      minutes until relay session needs refresh,
  servers:         [{ ip, port, securePort, realm, courseId, xBound, yBound, ... }],
}
```

`POST /relay/session/refresh` is called at ~90% of the session lifetime
(zwift.mjs:1926). Failure forces a full re-login.

### 4.3 World time

```
worldTime = Date.now() + offset - 1414016074400
```

— i.e. milliseconds since the Zwift epoch `1414016074400` (≈ 2014-10-22
19:34:34 UTC), adjusted by an offset learned from UDP handshake samples
(see §4.6). The epoch is hard-coded at `zwift.mjs:92`. All protobuf
`worldTime` fields use this scale.

### 4.4 Channel framing (TCP and UDP)

Both TCP and UDP payloads are encrypted with **AES-128-GCM** with a
**4-byte** auth tag (not the default 16). The only differences between
TCP and UDP on the wire are:

- TCP is length-prefixed with a **2-byte big-endian** frame size.
- TCP has an extra 2-byte protocol-version/flags prefix inside the
  ciphertext (`[version=2, hello?0:1]`, zwift.mjs:1295-1297).
- UDP seqno is always sent; TCP only sends it on hello/force.

The **header** (plaintext, also AES-GCM AAD) is variable-length, built by
`NetChannel.encodeHeader` (zwift.mjs:1112-1135). Structure:

```
Byte 0 : flags bitmap
         0x4  relayId present  (4 bytes BE)
         0x2  connId  present  (2 bytes BE)
         0x1  seqno   present  (4 bytes BE)
Bytes 1..n : selected fields in the order relayId, connId, seqno
```

Rules:

- On the very first ("hello") packet of a channel, sender includes
  relayId + connId + (seqno if nonzero).
- Subsequent packets only re-send a field when it changes; a channel can
  therefore send a 1-byte header (just `flags=0`) for steady-state
  packets, because the peer keeps the last-known values.

### 4.5 AES-GCM IV derivation (RelayIV)

`RelayIV.toBuffer()` (zwift.mjs:1019-1026) constructs the 12-byte GCM IV:

```
offset 0-1 : 0x00 0x00                  (must be zero — see note)
offset 2-3 : deviceType  BE u16         1=relay, 2=companion
offset 4-5 : channelType BE u16         1=udpClient, 2=udpServer,
                                        3=tcpClient, 4=tcpServer
offset 6-7 : connId      BE u16
offset 8-11: seqno       BE u32
```

A Rust port **must zero the first two bytes explicitly**. (The JS code
uses `Buffer.allocUnsafe(12)` and only writes bytes 2-11, relying on the
Node buffer pool to have zeros there. This works in practice but is a
latent bug; do not replicate it. The server's expected IV has those
bytes as zero.)

- The *send* IV uses `channelType = {udp,tcp}Client`, the *recv* IV uses
  `{udp,tcp}Server`.
- `connId` is a per-channel counter that the client picks at channel
  construction (`NetChannel.getConnInc()`, zwift.mjs:1036-1038). TCP and
  UDP have **independent** counters, both `u16` wrapping.
- `seqno` starts at `0` and increments **after** each encrypt/decrypt
  (zwift.mjs:1096, 1108). Sender and receiver maintain their own
  counters; receiver updates its seqno from header fields when present
  (zwift.mjs:1087-1090).

### 4.6 UDP establishment and SNTP-style time sync

`UDPChannel.establish()` (zwift.mjs:1332-1405):

1. Create a connected UDPv4 socket to the chosen server's `securePort`
   (3024, or 3022 for plaintext).
2. Send up to 25 "hello" packets with increasing delays (10 ms, 20 ms,
   …), measuring round-trip on each reply.
3. For each reply compute
   `latency = (localWorldTime - serverWorldTime) / 2` and
   `offset   = localWorldTime - (packet.worldTime + latency)`.
4. When ≥5 samples agree within a reasonable stddev, take the median (by
   lowest latency) and call `worldTimer.adjustOffset(-meanOffset)`.
5. Emit `'latency'`.

The watchdog interval for both TCP and UDP is `timeout/2` = 15 s
(zwift.mjs:1168); peer silence for >30 s triggers reconnect.

### 4.7 TCP establishment

`TCPChannel.establish()` (zwift.mjs:1209-1229):

1. `net.createConnection({ host: <ip>, port: 3025, timeout: 31000 })`.
   Do **not** enable `setKeepAlive` (there's a Node bug —
   nodejs/node#40764).
2. After `'connect'`, send a hello `ClientToServer` with full header
   (relayId+connId+seqno flags set).
3. On `'close'`/`'timeout'`/`'error'`, shut the channel and reconnect
   with exponential backoff (`1000 * 1.2^n`, zwift.mjs:1880).

When reconnecting, the monitor tries to reuse the previous server IP
first (zwift.mjs:1818-1826) to preserve server-side per-connection
state.

### 4.8 Server pool selection

The server list arrives two ways:

- **Initial TCP servers** — `LoginResponse.session.tcpConfig.servers[]`.
  Filtered to `realm == 0 && courseId == 0` (the generic pool).
- **UDP servers (per-course)** — every `ServerToClient` may carry
  `udpConfigVOD` (see proto), a list of `UDPServerVODPool` records each
  scoped to a `(realm, courseId)` and containing per-server geographic
  bounds `(xBound, yBound, xBoundMin, yBoundMin)`.

When the watched athlete is in a given course, the monitor picks the
UDP server via `findBestUDPServer(pool, x, y)` (zwift.mjs:2295-2317):

- If `pool.useFirstInBounds`, the first server whose bounding box covers
  `(x, y)`.
- Otherwise the server with minimum Euclidean distance from `(x, y)` to
  the server's bound center.

### 4.9 ClientToServer payload

Every outbound packet wraps a `ClientToServer` (proto §1803):

```proto
message ClientToServer {
    int32     realm                         = 1;
    int32     athleteId                     = 2;
    int64     worldTime                     = 3;
    int32     seqno                         = 4;   // app-level, distinct from IV seqno
    int32     lastMessageReceived           = 5;
    int64     lastMessageReceivedAt         = 6;
    PlayerState state                       = 7;   // our own position
    bool      requestAuxiliaryControllerIpAddress = 11;
    int64     largestWorldAttributeTimestamp      = 13;
    repeated int64 subscribeToSegments            = 15;
    repeated int64 unsubscribeFromSegments        = 16;
}
```

The monitor sends one every **1000 ms** (`broadcastPlayerState`,
zwift.mjs:1761). If the monitor account isn't actually riding, the
`state` fields are zero/default but still must pass basic sanity.

### 4.10 ServerToClient payload

Every inbound packet is a `ServerToClient` (proto §222):

```proto
message ServerToClient {
    int32   realm                 = 1;
    int32   athleteId             = 2;   // our id
    uint64  worldTime             = 3;
    int32   seqno                 = 4;
    int32   ackSeqno              = 5;   // last seqno server received from us
    repeated PlayerState   playerStates   = 8;   // LIVE TELEMETRY for everyone
    repeated WorldUpdate   worldUpdates   = 9;   // events / ride-ons / chat / ...
    repeated int64 deletedWorldUpdates   = 10;
    int32   athleteCount          = 14;
    int64   latency               = 17;
    UDPConfig    udpConfig        = 24;
    UDPConfigVOD udpConfigVOD     = 25;
    TCPConfig    tcpConfig        = 29;
    repeated int64 ackSubscribedSegments = 30;
}
```

`playerStates[]` is the live stream. Cadence is 10+ Hz aggregate (many
small packets); per-athlete cadence is roughly 1-2 Hz, but the server
batches nearby riders into each packet.

### 4.11 PlayerState fields (live metrics)

`PlayerState` (proto §4) packs quantities densely. Key fields:

| # | Name | Type | Scale / meaning |
|---:|---|---|---|
| 1 | `athleteId` | int32 | Rider ID. |
| 6 | `_speed` | int32 | Millimeters/hour. Divide by 1e6 for m/s; ×3.6 for km/h. |
| 9 | `_cadence` | int32 | Microrevolutions/sec × 1e6. ×6e-5 → RPM. Cap at `240 × 1e6 / 60`; values above that are treated as 1 (cadenceMax, zwift.mjs:57). |
| 11 | `heartrate` | int32 | BPM. |
| 12 | `power` | int32 | Watts. |
| 13 | `_heading` | int32 | Microradians, range `[-π, 3π)`. |
| 19 | `_flags1` | uint32 | Packed: `powerMeter`, `companionApp`, `reverse`, `uTurn`, `rideons` counter. |
| 20 | `_flags2` | uint32 | Packed: `activePowerUp` (4 bits), `turning` (2 bits), `roadId` etc. |
| 21 | `_progress` | uint32 | Workout progress percent + zone (packed). |
| 25,26,27 | `x, z, y` | float | 3-D world position (engine coords; Zwift's `z` is vertical). |
| 35 | `courseId` | int32 | World/map id. |
| 39 | `routeId` | int32 | Route within the world. |
| 43 | `portal` | bool | In a portal/event instance. |

Decoded in `GameMonitor.processPlayerStateMessage` (zwift.mjs:306-324):
flags unpack, cadence/speed convert, heading normalized to degrees, and
packet latency = `now() − worldTime`.

### 4.12 WorldUpdate payloads

`WorldUpdate` is a tagged envelope:

```proto
message WorldUpdate {
    int32 realm = 1;
    int64 ts    = 2;
    int32 forAthleteId = 3;
    float radiusFilter = 4;
    WorldUpdatePayloadType payloadType = 5;
    bytes _payload = 6;   // nested protobuf or custom binary
    ...
}
```

`WorldUpdatePayloadType` (enum) partitions payloads:

- **< 100**: nested protobuf, decoded by name — `PlayerJoinedWorld`,
  `PlayerLeftWorld`, `RideOn`, `SocialAction`, `PlayerRegisteredForEvent`,
  `NotableMoment`, etc.
- **≥ 100**: custom binary formats decoded from a map
  (`binaryWorldUpdateDecoders`, zwift.mjs:1500-1508). `SegmentResult` is
  `105`.

### 4.13 Adaptive cadence and idle suspension

`GameMonitor._refreshStates` (zwift.mjs:1998-2028):

- Minimum refresh interval: **3000 ms**.
- On stale responses, multiply by 1.02; on errors by 1.15 (with a cap).
- If the watched athlete appears completely idle
  (`_speed == 0 && _cadence == 0 && power == 0`), suspend UDP channels
  to save relay load (zwift.mjs:1977-1982); resume on activity
  (zwift.mjs:2237).

### 4.14 Companion app (optional)

`ServerToClient` can embed `companionIP`, `companionPort`, and a
`companionAesKey`. If the user has the Zwift Companion phone app on the
same network, Sauce opens a reverse TCP connection
(`GameConnectionServer`, zwift.mjs:2384+) using the same AES-128-GCM
framing but with `deviceType = companion (2)` in the IV. This allows
sending commands (ride-on, bell, U-turn, camera change) back to the
game.

This is orthogonal to live data; a Rust port can defer it.

### 4.15 What this app is **not** doing

- Not sniffing the game client's packets.
- Not running a local relay proxy.
- Not impersonating the user's game connection (the monitor logs in
  with its own credentials).
- Not breaking TLS or server encryption — the AES key is one the *client*
  chose; Zwift's server accepts that key and uses it symmetrically.

---

## 5. Stats processing pipeline

Core: `src/stats.mjs` (`StatsProcessor extends EventEmitter`,
stats.mjs:844). Supporting: `shared/sauce/{data,power,pace,geo}.mjs`.

### 5.1 Input

`StatsProcessor.processState(state, now)` (stats.mjs:2941) is invoked
for every `PlayerState` coming off `GameMonitor`. For each rider it
ensures an `AthleteData` record exists
(`_createAthleteData`, stats.mjs:2817), preprocesses the state
(world/sport change detection, grade smoothing, route distance), then
fans out into rolling buckets.

### 5.2 Per-athlete record (`AthleteData`)

```text
AthleteData {
  athleteId, courseId, sport,
  created, updated, wtOffset, distanceOffset,
  wBal: WBalAccumulator,                  // anaerobic work-capacity balance
  timeInPowerZones: ZonesAccumulator,     // Z1..Z7 seconds
  smoothGrade: expWeightedAvg(8),
  streams: { distance[], altitude[], latlng[], wbal[] },
  roadHistory: { aRoad, bRoad, cRoad, a[], b, c },
                                           // 3-tier sliding window of road segments
  bucket: DataBucket,                      // session-wide rolling stats
  lapSlices: [DataSlice],                  // closed laps
  eventSlices: [DataSlice],
  segmentSlices: [DataSlice],
  activeSegments: Map<segId, DataSlice>,
  mostRecentState, gap, gapDistance, isGapEst,
  groupId, eventSubgroup, eventPrivacy, disabledByEvent,
  internalUpdated, internalAccessed,       // for GC
}
```

`_athleteData: Map<athleteId, AthleteData>` is garbage-collected every
tick: riders unseen for 1 h are dropped, groups for 90 s
(`gcAthleteData`, stats.mjs:4075).

### 5.3 Rolling windows

Heart of the math is `shared/sauce/data.mjs`:

- **`RollingAverage`** — time-indexed ring with gap-fill semantics
  (`idealGap`, `maxGap`, then `softPad`/`Break` sentinels). Maintains
  `_times[]`, `_values[]`, cumulative active time and value sums for
  O(1) average queries.
- **`RollingPower`** (power.mjs:161) extends `RollingAverage` to inline
  **Normalized Power** (30 s rolling window, 4th-power mean, ^(1/4)) and
  optional **XP**.
- **`DataCollector`** wraps a primary `RollingAverage` plus a clone per
  "peak period" (5 s, 15 s, 60 s, 300 s, 1200 s, 3600 s). Each period
  tracks its own max and NP peak.

`DataBucket` (stats.mjs:2697-2714) holds one `DataCollector` per signal
(power, hr, speed, cadence, draft). A `DataSlice` is a snapshot of a
bucket's state for closing a lap/event/segment.

Update cadence:

- Raw telemetry is bucketed into **1 s** bins before being pushed into
  rolling windows (`DataCollector.add` buffers `_bufferedStart/Sum/Len`,
  flushes on 1 s boundaries).

### 5.4 Published metrics

| Category | Metrics | Source |
|---|---|---|
| Power | Avg, Max, Peak (5 s–1 h), NP, IF, TSS, kJ, W/kg | `bucket.power` + athlete FTP/weight |
| HR | Avg, Max, time-in-zones, drift | `bucket.hr`, `timeInPowerZones` |
| Speed | Avg, Max, instant | `bucket.speed`, `state._speed` |
| Cadence | Avg, instant | `bucket.cadence` |
| Draft | Avg follow / work kJ | `bucket.draft` + power |
| Elevation | Instant altitude, smooth grade, climb total | `streams.altitude`, smoothGrade |
| Position | Route distance, route %, lap distance, segment time | Routes lookups + `roadTime` |
| Nearby | gap time, gap distance, `isGapEst` | `compareRoadPositions`, stats.mjs:4047 |
| Groups | composition, avg power/HR/draft/speed, length | `_computeGroups`, stats.mjs:4458 |
| W' Bal | Remaining anaerobic capacity | `WBalAccumulator` (CP + W' model) |

Group detection is a greedy-Jaccard clustering over a sorted-by-gap
list of riders, with a gap threshold of 2 s (0.8 s without draft,
stats.mjs:4471-4472).

### 5.5 Events, laps, segments

- **Events** — detected by `state.eventSubgroupId`; metadata fetched
  via `ZwiftAPI.getEventSubgroup()`. `triggerEventStart/End` create or
  close an event slice and apply privacy flags
  (`hidewbal`, `hideFTP`, `hidethehud`).
- **Laps** — manual (`startAthleteLap`) or automatic based on distance/
  time (`_autoLapCheck`). Route-specific lap detection uses
  `shared/routes.mjs` lap-weld tables.
- **Segments** — `_activeSegmentCheck` queries
  `Env.getRoadSegments(courseId, roadId, reverse)` and walks the rider's
  road history to detect start/stop events.

### 5.6 Emitted events

Consumers subscribe to named events on the `stats` emitter. The main
channel names are:

- `athlete/watching`, `athlete/self`, `athlete/{id}` — formatted
  snapshot of one rider (v1).
- `athlete/watching/v2`, `athlete/self/v2`, `athlete/{id}/v2` —
  query-reduced v2 (only the fields the subscriber asked for).
- `streams/watching`, `streams/self`, `streams/{id}` — raw stream
  slices (power/speed/hr/latlng/altitude/wbal/distance).
- `nearby` — array of formatted athletes sorted by gap.
- `groups` — aggregated group data.
- `game-state`, `rideon`, `chat`, `watching-athlete-change`.

V2 uses `ADV2QueryReductionEmitter` (stats.mjs:750): each subscription
carries a query specifying which fields/resources it needs, and the
emitter memoizes the formatted payload so N subscribers with identical
queries only cost one serialization.

### 5.7 Persistence

- Athletes and per-athlete history → `athletes.sqlite`.
- Segment leaderboards, event metadata, session history → dedicated
  SQLite DBs opened via `new SqliteDatabase(name, {tables: {...}})`.
- Settings, window profiles, mod state → key-value `store.sqlite`
  (`src/storage.mjs`, table `store(id TEXT PK, data TEXT)`).
- FIT file export of a finished session via `jsfit`
  (`exportFIT`, stats.mjs:2057-2166).

---

## 6. IPC, web server, plugins

### 6.1 RPC bus

`src/rpc.mjs` is thin:

- `handlers: Map<name, fn>` — registered by `app.mjs` (getters/setters
  for settings, stats processor methods, profile ops, etc.).
- `subscribe(emitterName, event, cb, opts)` / `unsubscribe(id)` over a
  `Map<emitterName, EventEmitter>` held by `SauceApp.rpcEventEmitters`.

Named emitters (main.mjs):

| Name | Emitter | Purpose |
|---|---|---|
| `stats` | `StatsProcessor` | All live and derived metrics. |
| `windows` | `Windows.eventEmitter` | Window lifecycle. |
| `logs` | logging subsystem | Log streaming. |
| `mods` | mods subsystem | Mod enable/disable. |
| `updater` | `electron-updater` | Update progress. |
| `app` | `SauceApp` itself | Settings changes. |
| `gameConnection` | `GameConnectionServer` | Companion status. |

### 6.2 Electron ↔ renderer IPC

`src/preload/common.js` bridges renderers. The only channels:

| Channel | Direction | Purpose |
|---|---|---|
| `rpc` | R→M, returns | One-shot handler invocation. |
| `subscribe` | R→M, returns `subId` | Open an event subscription. |
| `unsubscribe` | R→M | Close a subscription. |
| `subscribe-port` | M→R | Deliver a `MessageChannelMain` port for the subscription. |
| `getWindowMetaSync` | sync | Fetch window spec during preload. |

Once a subscription is live, events flow over the dedicated
`MessagePort` — *not* over the main IPC bus — avoiding serialization
contention and giving native structured-clone throughput. The main
process suspends non-persistent subscriptions when a window is hidden
or minimized (main.mjs:198-256).

### 6.3 Web server

`src/webserver.mjs` (573 lines): Express + `ws`. Default
`http://localhost:1080`. HTTPS is auto-enabled if `/https/key.pem` and
`/https/cert.pem` exist.

- Serves built-in widget pages under `/pages/...` and mod web roots.
- Single WebSocket endpoint `/api/socket`. The protocol is JSON:

  ```
  → { type: "request", method: "subscribe" | "unsubscribe" | "rpc",
      uid: <int>, arg: {...} }
  ← { type: "response", uid, success, data }
  ← { type: "event",    uid: <subId>, success: true, data }
  ```

  `subscribe` args: `{ event, source, subId, options }` — maps 1:1 to the
  RPC `subscribe`. Event frames are written as three buffers
  (header + JSON body + tail) to avoid re-encoding; compression turns on
  at 64 KB but is disabled for loopback. Backpressure drops clients that
  exceed 8 MB of buffered output (webserver.mjs:148-150).

### 6.4 Mod system

`src/mods.mjs` + `mods-core.mjs`. Mods are either directories under
`~/Documents/SauceMods/*/manifest.json` or zipped archives under
`{appPath}/mods/*.zip`. A `manifest.json` declares:

```
manifest_version: 1
id, name, version, author
web_root:       "./web"          // served under /mods/<id>/...
content_js:     ["inject.js"]    // injected into every renderer
content_css:    ["inject.css"]
windows:        [{ id, file, name, overlay, frame,
                   default_bounds, query }]
```

Mod windows merge into the main widget manifest table; the user can
enable/disable mods in settings. Disabled mods neither inject scripts
nor expose their widgets.

### 6.5 Headless mode

`src/headless.mjs` — re-spawn of Node with `ELECTRON_RUN_AS_NODE=1`. It
constructs `NodeSauceApp` which stubs out all window APIs but still
runs:

- Both `ZwiftAPI` instances
- `GameMonitor`
- `StatsProcessor`
- The Express + WS web server

So browser clients get the full data stream; the Electron UI is simply
absent. This is the natural target for a Rust port (replace Electron
plus the stats engine with a native binary, keep web clients as-is).

---

## 7. Rust reimplementation specification

The goal here is not to clone the Electron UI, but to reproduce the
**live-data core**: log into Zwift, maintain the relay session, decode
the stream, run the stats engine, and serve the same WebSocket protocol
on port 1080 so existing browser widgets continue to work unchanged.

### 7.1 Scope

| In scope | Out of scope (for v1) |
|---|---|
| OAuth2 password login against Keycloak, token refresh. | GUI, Electron widgets, hotkeys, macOS window control. |
| `/api/users/login` relay handshake, session refresh. | FIT export (jsfit replacement can come later). |
| TCP/3025 + UDP/3024 channels with AES-128-GCM. | Mod system (can be reintroduced as a data-only plugin API later). |
| Server-pool selection; reconnection with backoff. | Companion-app reverse channel. |
| Full `ServerToClient` / `ClientToServer` codec. | Sentry/error telemetry. |
| `StatsProcessor`-equivalent: rolling windows, NP, TSS, zones, W', groups, segments, laps. | Keytar UI (use OS secret store via `keyring` crate). |
| SQLite key-value store + athletes DB. | |
| HTTP + WebSocket server compatible with `webserver.mjs`'s JSON wire format. | |
| Headless-equivalent CLI. | |

### 7.2 Crate layout

```
zwift-relay/        # protocol: login, channels, codec, session
zwift-stats/        # rolling windows, power.rs, pace.rs, geo.rs
zwift-api/          # REST client (profiles, events, segments)
zwift-routes/       # static Zwift world/route tables (port of shared/routes.mjs, curves.mjs)
zwift-daemon/       # binary: wires them together, runs HTTP+WS server
zwift-proto/        # prost-generated types from vendored zwift-offline proto tree (proto/*.proto, proto2)
```

### 7.3 Dependencies (Rust)

| Need | Crate |
|---|---|
| Async runtime | `tokio` (rt-multi-thread, net, sync, time, io-util). |
| HTTP client | `reqwest` with `rustls-tls`. |
| HTTP server | `axum` (or `hyper` + `tower`). |
| WebSocket | `tokio-tungstenite` / `axum`'s ws. |
| Protobuf | `prost` + `prost-build` against `crates/zwift-proto/proto/*.proto`, vendored from upstream [`zoffline/zwift-offline`](https://github.com/zoffline/zwift-offline) (AGPL-3.0, proto2, multi-file). Maintained in-tree; no runtime/build reference to sauce4zwift or to the upstream checkout. |
| AES-GCM | `aes-gcm` (RustCrypto) — must support 4-byte tag (`Aes128Gcm` accepts any tag via `aead::generic_array` … use `aes-gcm`'s `Aes128Gcm::new_from_slice` plus `aead::AeadInPlace` with explicit `Tag<Aes128Gcm>` of size `U4`, or implement GCM over `aes` primitive directly). |
| JSON | `serde`, `serde_json`. |
| SQLite | `rusqlite` (bundled) with WAL. |
| Keyring | `keyring`. |
| Logging | `tracing`, `tracing-subscriber`. |
| CLI | `clap` derive. |
| Time | `chrono` or `time`. |
| Event bus | `tokio::sync::broadcast` for fan-out; `flume`/`async_channel` where single-consumer. |

**AES-GCM tag size note.** GCM with a non-default tag size (4 bytes) is
unusual but fully supported by the spec. The RustCrypto `aes-gcm` crate
exposes `AesGcm<Aes128, U4>` via the generic `AesGcm` type —
instantiate with `type Aes128Gcm4 = AesGcm<Aes128, U4>;`. Verify round-
trip against the Node implementation on a known vector before trusting
anything.

### 7.4 Protocol reference (Rust-side)

Constants:

```rust
pub const WORLD_TIME_EPOCH_MS: i64 = 1_414_016_074_400;

pub const RELAY_HOST: &str = "us-or-rly101.zwift.com";
pub const AUTH_HOST:  &str = "secure.zwift.com";

pub const TCP_PORT_SECURE:  u16 = 3025;
pub const UDP_PORT_SECURE:  u16 = 3024;
pub const TCP_PORT_PLAIN:   u16 = 3023;   // not used by this client
pub const UDP_PORT_PLAIN:   u16 = 3022;   // not used by this client

pub const CHANNEL_TIMEOUT: Duration = Duration::from_secs(30);
pub const PLAYER_STATE_TICK: Duration = Duration::from_secs(1);
pub const MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
pub const SESSION_REFRESH_FRACTION: f64 = 0.9;
pub const TOKEN_REFRESH_FRACTION:   f64 = 0.5;
```

IV layout (12 bytes):

```rust
#[repr(u16)] pub enum DeviceType  { Relay = 1, Companion = 2 }
#[repr(u16)] pub enum ChannelType { UdpClient = 1, UdpServer = 2, TcpClient = 3, TcpServer = 4 }

pub struct RelayIv { pub device: DeviceType, pub channel: ChannelType, pub conn_id: u16, pub seqno: u32 }

impl RelayIv {
    pub fn to_bytes(&self) -> [u8; 12] {
        let mut b = [0u8; 12];                     // NB: zero-init, not uninit
        b[2..4].copy_from_slice(&(self.device   as u16).to_be_bytes());
        b[4..6].copy_from_slice(&(self.channel  as u16).to_be_bytes());
        b[6..8].copy_from_slice(&self.conn_id.to_be_bytes());
        b[8..12].copy_from_slice(&self.seqno.to_be_bytes());
        b
    }
}
```

Header codec:

```rust
bitflags! { pub struct HeaderFlags: u8 { const RELAY_ID = 0x4; const CONN_ID = 0x2; const SEQNO = 0x1; } }

pub struct Header { pub flags: HeaderFlags, pub relay_id: Option<u32>, pub conn_id: Option<u16>, pub seqno: Option<u32> }

// Encode: 1 byte flags, then present fields in order relay_id (BE u32),
// conn_id (BE u16), seqno (BE u32). The full encoded header is the AES-GCM AAD.
```

TCP framing:

```
wire = [BE u16 frame_size][header_bytes][ciphertext || tag4]
plaintext_for_encrypt = [u8 version=2][u8 hello?0:1][ClientToServer proto bytes]
```

UDP framing: identical minus the 2-byte frame-size prefix and the
2-byte version/hello prefix; plaintext is just the proto bytes.

### 7.5 Auth client pseudocode

```rust
pub struct ZwiftAuth { http: reqwest::Client, tokens: RwLock<Option<Tokens>> }

impl ZwiftAuth {
    pub async fn login(&self, u: &str, p: &str) -> Result<Tokens> {
        let resp = self.http.post(format!("https://{AUTH_HOST}/auth/realms/zwift/protocol/openid-connect/token"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(url_encode(&[("client_id","Zwift Game Client"),("grant_type","password"),("username",u),("password",p)]))
            .send().await?.error_for_status()?.json::<TokenResp>().await?;
        let tokens = Tokens::from_resp(resp, now());
        *self.tokens.write().await = Some(tokens.clone());
        self.schedule_refresh(tokens.expires_in / 2);  // preemptive
        Ok(tokens)
    }

    pub async fn bearer(&self) -> Result<String> { /* return access_token, refresh if ≥ 50% life */ }
}
```

Scope: `Source: Sauce for Zwift` and a `User-Agent` header may be required
for some endpoints — copy whatever `ZwiftAPI.fetch()` sends verbatim from
`zwift.mjs` to minimize surprise.

### 7.6 Relay session pseudocode

```rust
pub struct RelaySession {
    pub relay_id: u32,
    pub aes_key: [u8; 16],
    pub tcp_servers: Vec<TcpServer>,
    pub expiration: Duration,           // total, from login
    pub logged_in_at: Instant,
}

pub async fn login(api: &ZwiftAuth) -> Result<RelaySession> {
    let mut aes_key = [0u8; 16];
    OsRng.fill_bytes(&mut aes_key);
    let body = LoginRequest { aes_key: aes_key.to_vec() }.encode_to_vec();
    let resp = http.post(format!("https://{RELAY_HOST}/api/users/login"))
        .bearer_auth(api.bearer().await?)
        .header("Content-Type", "application/x-protobuf-lite")
        .body(body).send().await?.error_for_status()?.bytes().await?;
    let pr = LoginResponse::decode(resp)?;
    Ok(RelaySession { relay_id: pr.relay_session_id, aes_key,
        tcp_servers: pr.session.unwrap_or_default().tcp_config.unwrap_or_default().servers,
        expiration: Duration::from_secs(60 * pr.expiration as u64),
        logged_in_at: Instant::now(),
    })
}
```

A supervisor task wakes at `logged_in_at + 0.9 * expiration` and calls
`/relay/session/refresh`; on failure, tear down channels and re-`login`.

### 7.7 Channel state machine (identical for TCP / UDP except transport)

```
[Closed]
   │ connect()
   ▼
[Connecting] ── error ──▶ [Backoff (1.2^n s)] ── timer ──▶ [Connecting]
   │ established + hello ACK
   ▼
[Active] ── inbound ─▶ decrypt ─▶ emit ServerToClient
   │ watchdog (>30s silent)
   │ OR explicit close
   ▼
[Closed]
```

Watchdog timer fires every `CHANNEL_TIMEOUT / 2`; last-activity is any
successful receive.

Idle suspension: when watched-athlete telemetry reports
`speed == 0 && cadence == 0 && power == 0`, schedule UDP channel
shutdown after ~60 s; resume on any non-zero field.

### 7.8 Stats engine

Port `shared/sauce/data.mjs` → `zwift_stats::rolling`, with:

- `RollingAverage<T>` (time + value pairs in `VecDeque`, maintained
  sums).
- `RollingPower` (embeds NP/XP accumulators).
- `DataCollector` (primary rolling + cloned per-period peaks).
- `ZonesAccumulator`, `WBalAccumulator`.

Preserve:

- **1-s bucketing** before rolling pushes. NP's internal window is 30 s
  of 4th-power mean, evaluated when active time ≥ 300 s.
- **Peak periods** `[5, 15, 60, 300, 1200, 3600]` s for power;
  `[60, 300, 1200, 3600]` s for speed/HR/cadence/draft.
- TSS: `TSS = (s · NP · IF) / (FTP · 3600) · 100` with
  `IF = NP / FTP`. (Source of truth is
  `Sauce.power.calcTSS`, `shared/sauce/power.mjs`.)

Per-athlete GC: drop `AthleteData` after 1 h of no updates; drop group
entries after 90 s. Run GC on a 10-s tick.

Group detection: port `_computeGroups` (stats.mjs:4458-4584). Greedy
clustering by gap with threshold 2 s (0.8 s without draft), with
Jaccard-based identity preservation across frames.

Segment detection: depends on `shared/routes.mjs` and
`shared/curves.mjs`. These are large static tables (world geometry and
segment definitions) and should be included as generated data files in
`zwift-routes`. A reasonable first pass is to `serde`-serialize the
tables from the JS side once and load them as `rkyv`/`bincode` blobs at
runtime.

### 7.9 Web surface

Serve the same JSON WebSocket protocol under `/api/socket`:

```rust
enum ClientMsg { Request { uid: u64, method: Method, arg: Value } }
enum Method    { Subscribe, Unsubscribe, Rpc }

enum ServerMsg {
    Response { uid: u64, success: bool, data: Value },
    Event    { uid: u64, success: bool, data: Value },
}
```

Where `data` for events is the formatted stats payload. Keep field
names byte-for-byte compatible with the existing v1/v2 formatters so
that unmodified browser widgets continue to work. The simplest route
is: port `_formatAthleteData{,V2}` verbatim into Rust with the same
field casing.

Serve `/pages/*` as static files from a vendored widget tree shipped
alongside the Rust binary. The widgets are copied once from
sauce4zwift's `pages/` tree into ranchero (target location: `pages/`
under the workspace root) and then maintained in-tree — no runtime
reference to the sauce4zwift checkout.

### 7.10 Persistence

- `zwift_daemon::store`: rusqlite-backed KV (single table
  `store(id TEXT PRIMARY KEY, data BLOB)`, WAL mode).
- `zwift_daemon::athletes_db`: per-athlete profile cache (id, fname,
  lname, ftp, weight, badges JSON, last_seen, etc.).
- `zwift_daemon::segments_db`: segment leaderboard cache with TTL.

Use `keyring` with service `"Zwift Credentials - Sauce for Zwift"` so an
existing Sauce install's credentials are picked up unchanged.

### 7.11 Compatibility tests

Before declaring a Rust port done, verify:

1. **AES-GCM interop.** Encrypt a known
   `(key, iv, aad, plaintext)` vector with both the JS client and the
   Rust client — outputs must be byte-identical.
2. **Header codec round-trip.** Fuzz `encode(decode(x)) == x` for all
   8 flag combinations.
3. **Login.** Rust login must produce a `ServerToClient` on TCP and
   receive one UDP packet within 5 s of `establish()`.
4. **Metric parity.** Feed a recorded `ServerToClient` stream (capture
   one from the JS monitor) through both engines; compare published
   metrics at each tick. Accept floating-point drift ≤ 1e-6 on sums,
   exact match on counts/zones.
5. **WebSocket parity.** Point an unmodified Sauce widget page at the
   Rust daemon; widgets must render correctly.

### 7.12 Known footguns

- **`Buffer.allocUnsafe` for the IV.** The JS code leaves IV bytes 0-1
  uninitialized. In Node this almost always reads as zero from the
  buffer pool, which is what the Zwift server expects. In Rust, zero
  these explicitly.
- **Cadence overflow.** Values above `240 × 1e6 / 60` are Zwift's "lag
  burst" values and must be clamped or dropped. See `cadenceMax`,
  zwift.mjs:57.
- **`keepAlive` on TCP.** Do **not** enable TCP keepalive; use the
  application-level 1 Hz `ClientToServer` heartbeat. (Node issue #40764
  is a socket-level reason; a Rust port doesn't suffer the same bug but
  the server already has its own liveness model — keep the heartbeat.)
- **Protobuf `keepCase`.** The JS client runs protobufjs with
  `keepCase = true` so proto names like `_speed` stay underscored. prost
  will produce `speed` in snake-case; this is fine internally, but when
  serializing v2 payloads for the WebSocket make sure field names match
  what existing widgets read.
- **Two accounts.** Main and monitor are independent `ZwiftAuth` +
  `RelaySession` pairs. Wire them identically and keep them separate.
- **`client_id`.** The literal value `"Zwift Game Client"` (with the
  space) is required. URL-encode it as `Zwift+Game+Client` in the
  form body.

---

## 8. Appendix A — Key protobuf messages

The schema is vendored at `crates/zwift-proto/proto/*.proto`, sourced
from upstream `zoffline/zwift-offline` (proto2, multi-file). Names
below are upstream's; sauce4zwift's renamed equivalents from its
single-file proto3 fork are noted in parentheses where they differ.

The messages a Rust port *must* decode correctly are:

- `LoginRequest`, `LoginResponse` (in `login.proto`) — relay handshake.
- `ClientToServer` (in `udp-node-msgs.proto`) — every outbound packet.
- `ServerToClient` (in `udp-node-msgs.proto`) — every inbound packet.
- `PlayerState` (in `udp-node-msgs.proto`) — per-rider telemetry.
- `WorldAttribute` + `WA_TYPE` (sauce4zwift: `WorldUpdate` +
  `WorldUpdatePayloadType`, in `udp-node-msgs.proto`) — event envelope
  + tag enum.
- `TcpAddress`, `TcpConfig` (sauce4zwift: `TCPServer`, `TCPConfig`, in
  `per-session-info.proto`); `RelayAddress`, `UdpConfig`,
  `RelayAddressesVOD`, `UdpConfigVOD` (sauce4zwift: `UDPServer`,
  `UDPConfig`, `UDPServerVODPool`, `UDPConfigVOD`, in
  `udp-node-msgs.proto`) — server pool records.
- `SegmentResult` (`segment-result.proto`), `RideOn`, `PlayerLeftWorld`
  (both `udp-node-msgs.proto`), `Event`, `EventSubgroup`
  (`events.proto`), `Segment` — payloads of interest. (sauce4zwift's
  `PlayerJoinedWorld` does not appear by that name upstream; verify
  the corresponding upstream payload during STEP 06 elaboration.)

Everything else in the upstream tree is useful but not load-bearing for
the live-data core; vendor only the files needed (see STEP-06).

## 9. Appendix B — Runtime knobs

| Knob | Default | Where |
|---|---|---|
| Web server bind | `localhost:1080` | `app.mjs:336`, overridable via settings. |
| Token refresh fraction | 0.5 | `zwift.mjs:361`, `397`. |
| Session refresh fraction | 0.9 | `zwift.mjs:1926`. |
| Channel timeout | 30 s | `zwift.mjs:1053`. |
| Watchdog interval | timeout / 2 | `zwift.mjs:1168`. |
| `PlayerState` tick | 1 s | `zwift.mjs:1761`. |
| Min state refresh | 3 s | `zwift.mjs:1474`. |
| Backoff base × multiplier | 1000 ms × 1.2 | `zwift.mjs:1880`. |
| Error count limit per channel | 10 | `zwift.mjs:1064`. |
| Athlete GC TTL | 1 h | `stats.mjs:4075`. |
| Group GC TTL | 90 s | `stats.mjs:4075`. |
| Rolling peak periods (power) | 5,15,60,300,1200,3600 s | `stats.mjs:2697-2714`. |
| NP window | 30 s | `shared/sauce/power.mjs:161`. |
| NP min active time | 300 s | `shared/sauce/power.mjs`. |

---

*End of document.*
