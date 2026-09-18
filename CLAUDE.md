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

### Canary releases

New users are split between the canary (nightly/experimental) build and the regular release.
The only backend state is `canary.json` next to `latest.json` in the bucket — it carries
`browser_download_url`, `version` and `cohort` (0..100, the percentage of new users to route).
No counter service: `core/src/canary.rs` hashes the persisted anonymous analytics id (FNV-1a,
rolled by hand because `DefaultHasher` is not stable across toolchains) into one of 100 buckets,
so a user's assignment never flips between launches.

- Only *new* users are auto-routed — `installs::latest_dir_exists()` is the new-user signal, since
  `latest/` only ever appears after a successful install. Existing users stay on the regular track.
- `--prefer-canary-release` forces the canary build locally for anyone; `--use-canary-json-url`
  points at a custom `canary.json` and implies that preference on its own.
- A `canary.json` fetch failure on the *cohort* path falls back to the regular release — it must
  never block a first install. On the explicit opt-in path the error propagates instead, so a
  broken canary setup is visible to whoever asked for it.
- `RELEASE_CHANNEL_SELECTED` reports channel + version + `new_user` + `forced_locally`; every
  event already carries the anonymous id, which is what makes the split measurable in Segment.
- Canary builds may be served from a url outside the bucket layout, so `DownloadStep` trusts the
  version declared by the release instead of parsing it out of the url (regular releases still parse).

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
