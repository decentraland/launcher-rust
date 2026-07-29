# Launcher Rust — Agent Context

## Build & Test

```bash
cargo build
cargo test
```

CI runs tests on both Windows and macOS.

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

### Sentry logging

- `SentryLogger` (core/src/logs.rs) turns every `log::error!` into its own message-grouped Sentry event. If an error is already reported via `sentry::capture_error` (which carries the per-code fingerprint), its companion `log::error!` must use `target: crate::logs::SENTRY_BREADCRUMB_ONLY_TARGET` so it becomes a breadcrumb on the captured event instead of a duplicate, unfingerprinted issue.
- Demotion must stay opt-in at the call site. Never filter by module path or level blanket-wide — a future unpaired `log::error!` in that module would then silently stop creating Sentry issues.

### Code style (Rust)

- Extract complex if/else branches into named functions for readability. Prefer named free functions (`fn as_rename_back_err(...)`) over closures for error mapping.
- Public functions that take user-influenced strings for path construction should validate inputs locally (e.g. reject `/` or `\` in version strings) even if upstream code already constrains them — keeps the safety invariant self-documenting.
- Compile-time values belong in `const` items, not `const fn` getters: `pub const BUILD_COMMIT: &str = match option_env!("GIT_COMMIT") { ... }` rather than a zero-arg `const fn build_commit()`.
- Don't repeat the same expression/comparison twice in a function — extract it into a named local (e.g. `let is_pr_build = BUILD_PR != "na";`).

### Install edge cases

- Same-version reinstall is a valid scenario — it happens when `latest/Decentraland.exe` gets removed/corrupted or antivirus quarantines the binary. Install logic must handle `target == branch_path` (the rename-back target being the same as the decompress target).
