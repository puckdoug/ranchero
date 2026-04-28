# Step 05 — Credential storage (stub)

## Goal

Provide the implementation of the `KeyringStore` trait from STEP 02,
backed by the OS keychain via the `keyring` crate, using the service
name `"Zwift Credentials - Sauce for Zwift"` so existing sauce4zwift
installations are recognised unchanged (per spec §7.10).

## Sketch

- Two keys: `"main"` and `"monitor"`, each holding a JSON
  `{"username": ..., "password": ...}` blob in the exact format that
  sauce4zwift writes (see `src/secrets.mjs`).
- CLI option overrides (`--mainpassword` and similar) bypass the
  keyring but do not persist.
- `ranchero configure` is the only code path that writes; other code
  paths only read.

## Tests-first outline

- Trait-level tests run against the in-memory implementation (covered
  in STEP 02).
- Platform tests behind `#[cfg(target_os = ...)]` exercise the real
  backend only on CI images that provide one (macOS Keychain, Windows
  Credential Manager, libsecret).
- Explicit round-trip test with a sauce4zwift-shaped blob asserting
  byte-for-byte format compatibility.

To be fully elaborated when work begins on this step.
