# Step 17 — HTTP + WebSocket server (stub)

## Goal

Replace `sauce4zwift/src/webserver.mjs` with an axum-based server. Must
serve the exact JSON protocol widgets expect (spec §6.3 / §7.9).

- `GET /api/socket` — WebSocket upgrade. Per-client JSON frames:
  ```
  → { "type":"request", "method":"subscribe|unsubscribe|rpc",
      "uid": <int>, "arg": {...} }
  ← { "type":"response", "uid", "success", "data" }
  ← { "type":"event",    "uid":<subId>, "success":true, "data": <...> }
  ```
- `/pages/*` — static file server rooted at a configurable path
  (default: `./pages` relative to binary). The widget tree is
  vendored once from sauce4zwift's `pages/` into ranchero and
  maintained in-tree thereafter; the server must not resolve through
  any path that points back at the sauce4zwift checkout.
- Bind to `server.bind:server.port` from config (default
  `127.0.0.1:1080`).
- HTTPS auto-enables if `./https/{key,cert}.pem` exists.
- Backpressure: drop clients that exceed 8 MB buffered.

## Tests-first outline

- End-to-end: spawn the server, connect a test WS client, drive
  subscribe / event / unsubscribe flows, assert exact JSON frames.
- Backpressure: feed a stuck client; socket is closed after threshold.
- HTTPS conditional: cert files present → TLS listener, absent → HTTP.

To be fully elaborated when work on this step begins.
