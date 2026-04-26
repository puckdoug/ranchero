# Step 05 — Credential storage (stub)

## Goal

Real implementation of the `KeyringStore` trait from STEP 02, backed by
the OS keychain via the `keyring` crate, using service name
`"Zwift Credentials - Sauce for Zwift"` so existing sauce4zwift installs
are picked up unchanged (per spec §7.10).

## Sketch

- Two keys: `"main"` and `"monitor"`, each holding a JSON
  `{"username": ..., "password": ...}` blob exactly as sauce4zwift
  writes it (see `src/secrets.mjs`).
- CLI option overrides (`--mainpassword` etc.) bypass the keyring but
  do not persist.
- `ranchero configure` is the only code path that writes; other code
  paths only read.

## Tests-first outline

- Trait-level tests run against the in-memory fake (covered in STEP 02).
- Platform tests behind `#[cfg(target_os = ...)]` exercising the real
  backend only on CI images that provide one (macOS Keychain, Windows
  Credential Manager, libsecret).
- Explicit round-trip test with a sauce4zwift-shaped blob asserting
  byte-for-byte format compatibility.

To be fully elaborated when we start work on this step.
