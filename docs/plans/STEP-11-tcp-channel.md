# Step 11 — TCP channel (stub)

## Goal

Secure TCP/3025 channel per spec §4.7:

- `TcpStream::connect` with 31 s timeout; **do not** enable keepalive
  (spec §7.12 footgun).
- After `connect`, send hello `ClientToServer` with full header
  (relayId + connId + seqno flags set).
- Exponential reconnect backoff `1000 * 1.2^n` ms.
- On reconnect, prefer the previously-used server IP to preserve
  server-side per-connection state.
- Shared state machine with UDP: Closed → Connecting → Active → Closed.

## Tests-first outline

- State-machine transitions under injected transport failures.
- Backoff schedule matches `1000 * 1.2^n` exactly.
- Reconnect server selection prefers the last-used IP.
- 1 Hz `ClientToServer` heartbeat fires regardless of inbound cadence.

To be fully elaborated when we start work on this step.
