# Step 09 — Relay session (login + refresh supervisor) (stub)

## Goal

Implement the HTTPS handshake from spec §4.1 and §7.6:

1. Generate a 16-byte AES key with `OsRng`.
2. POST `LoginRequest { aes_key }` to
   `https://us-or-rly101.zwift.com/api/users/login` with
   `Content-Type: application/x-protobuf-lite`.
3. Decode `LoginResponse`; persist `relay_session_id`, `aes_key`,
   `tcp_servers`, `expiration`, `logged_in_at`.
4. Supervisor: refresh at `logged_in_at + 0.9 * expiration` via
   `/relay/session/refresh`; fall back to full re-login on failure.

## Tests-first outline

- Mocked HTTP: login request body parses as a `LoginRequest` with the
  exact 16 bytes the client generated; response is decoded end-to-end.
- Supervisor schedule: using a fake clock, assert refresh fires at
  exactly 0.9 × expiration and re-login fires after a forced refresh
  failure.
- TCP-server list filter: `realm == 0 && courseId == 0` only.

To be fully elaborated when we start work on this step.
