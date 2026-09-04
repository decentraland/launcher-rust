# Launcher Rust — Agent Context

## Build & Test

```bash
cargo build
cargo test
```

Test lanes: `npm run test-unit` runs each crate's inline unit tests (`--lib`/`--bins`); `npm run e2e` runs the `#[ignore]`-gated integration suites (tests-e2e Layer 1, src-tauri Layer 2) via `scripts/run-e2e.rs`. CI (`.github/workflows/tests.yml`) runs both lanes on Windows and macOS; src-tauri's unit tests run in the e2e job because compiling src-tauri needs staged sidecar binaries.

## Architecture

The launcher manages the download-install-launch funnel for the Decentraland Explorer. Key modules:

- `core/src/flow.rs` — orchestrates the step-based workflow (download → install → launch)
- `core/src/installs.rs` — handles Explorer installation, version management, and directory layout
- `core/src/errors.rs` — error types with user-facing messages and Sentry error codes

### Install flow

`InstallStep::execute` calls `install_explorer` then `rename_explorer_to_latest`. Both must succeed before reporting `INSTALL_VERSION_SUCCESS` to analytics. Chain them into a single `Result` so a rename failure is reported as `INSTALL_VERSION_ERROR` — never fire SUCCESS before the full operation completes.

### File-based deeplink bridge

When a `decentraland://` link arrives while the Explorer is already running, the launcher writes the deeplink to `deeplink-bridge.json` and polls for the Explorer to consume (delete) it within 3 seconds. If the Explorer's main thread is blocked (ANR), the file is never consumed and `E3001_OPEN_DEEPLINK_TIMEOUT` fires.

## Conventions

### Error codes

- Every distinct IO failure point should have its own Sentry error code (e.g. E3005, E3006, E3007) rather than falling through to `E0000_GENERIC_ERROR`. Specific codes let Sentry pivot on each failure independently.
- User-facing messages for file-system errors on Windows should advise closing the Decentraland client before retrying — the most common cause of rename/cleanup failures is the Explorer holding file locks.

### Code style (Rust)

- Extract complex if/else branches into named functions for readability. Prefer named free functions (`fn as_rename_back_err(...)`) over closures for error mapping.
- Public functions that take user-influenced strings for path construction should validate inputs locally (e.g. reject `/` or `\` in version strings) even if upstream code already constrains them — keeps the safety invariant self-documenting.
- Compile-time values belong in `const` items, not `const fn` getters: `pub const BUILD_COMMIT: &str = match option_env!("GIT_COMMIT") { ... }` rather than a zero-arg `const fn build_commit()`.
- Don't repeat the same expression/comparison twice in a function — extract it into a named local (e.g. `let is_pr_build = BUILD_PR != "na";`).

### Windows CRT linking

- Every Windows binary must link the CRT statically (`-C target-feature=+crt-static` in each `.cargo/config.toml`). The NSIS installer no longer downloads `vc_redist.x64.exe` — the hidden PowerShell download tripped antivirus heuristics — so a dynamic CRT dependency would break clean Windows installs. `src-tauri/scripts/assert-static-crt.ps1` enforces this in CI.
- Cargo resolves `.cargo/config.toml` from the *working directory*, not the manifest path, so `pre-build-installer-hooks.ps1` (run from the repo root) picks up the root config, not `installer-hooks/.cargo/config.toml`. Both must carry the flag.

### Install edge cases

- Same-version reinstall is a valid scenario — it happens when `latest/Decentraland.exe` gets removed/corrupted or antivirus quarantines the binary. Install logic must handle `target == branch_path` (the rename-back target being the same as the decompress target).
