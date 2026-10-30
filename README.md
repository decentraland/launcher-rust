# Decentraland Launcher (Rust)

## Project Overview

The **Decentraland Launcher (Rust)** is a cross-platform desktop application that manages the installation and launching of the Decentraland client (Explorer). It is built with Rust and Tauri for performance, security, and a smaller binary footprint. The launcher ensures users always run the latest version of the Explorer by handling download, installation, and startup logic.

## Features

- Automatic updates for Decentraland Explorer
- Cross-platform support (Windows, macOS)
- Lightweight architecture using Rust and native system WebViews
- URI scheme support: `decentraland://`
- Secure download and launch process
- Unified frontend-backend communication via single union-type enum

## Architecture Overview

The architecture uses:
- **Rust (core)**: Handles system-level logic such as update checks, downloads, and launching the Explorer.
- **Frontend (TypeScript)**: Renders the UI via Tauri using web technologies.
- **Tauri bridge**: Facilitates IPC between frontend and backend.

```
src/
├── core/        # Rust logic for download, update, launch
├── src-tauri/   # Tauri app entry point and configuration
└── frontend/    # Web-based UI (React/Vue/Svelte/etc.)
```

## Progress Reporting and Error Handling

Users receive concise, user-friendly error messages. Detailed stack traces are logged, and critical errors are captured in Sentry.

Dedicated `StepError` type standardizes error handling. Errors are explicitly mapped and categorized to ensure consistent behavior and improved UX.

More details: [https://github.com/decentraland/launcher-rust/issues/57](https://github.com/decentraland/launcher-rust/issues/57)


## Installation

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Node.js
brew install node # Or use your favorite package manager

# Install rust-script (runs the sidecar build scripts in ./scripts)
cargo install rust-script --locked

# Clone and install
git clone https://github.com/decentraland/launcher-rust.git
cd launcher-rust
npm install
```

## Running the Project

### Development

```bash
npm run tauri dev
```

## Building the Project

1. Ensure your rust version is 1.88.0 (it has not been tested with versions above, CI uses the specified version)
2. Ensure you have TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD env variables (for more information refer to https://v2.tauri.app/plugin/updater/#building)
3. Ensure you have VITE_AWS_S3_BUCKET_PUBLIC_URL env var
4. Execute in the root dir: npm i
5. Execute tauri build: npm run tauri build

## Crash watchdog sidecar

`client-crash-watchdog/` builds `dcl_watchdog`, a Tauri sidecar (`bundle.externalBin`) the
launcher starts next to every Explorer it launches. When the Explorer exits with a non-zero
status the watchdog writes `{APP_DIR}/crash-reports/<session_id>.json` and reopens the
launcher with `--crash-report-with-attachment <path>`, which shows the crash-report dialog
instead of the launch flow. A clean exit ends the watchdog silently.

The sidecar must be staged in `src-tauri/binaries/` before **any** `cargo` command in
`src-tauri` (build, clippy, test): `tauri-build` copies it at compile time and fails when it is
missing. `npm run tauri dev` / `tauri build` do this through `beforeDevCommand` /
`beforeBuildCommand`; run it by hand with:

```bash
npm run prebuild-sidecars   # or: rust-script scripts/pre-build-sidecars.rs
```

`TAURI_ENV_TARGET_TRIPLE=universal-apple-darwin` builds both macOS arches and stages the fat
binary under the universal and both per-arch names.

### Crash report delivery

The report is recorded as a Sentry event and, when the Explorer left a session file, filed as
an Intercom ticket through `intercom-proxy` using Decentraland Signed Fetch
(`core/src/infra/signed_fetch`, `core/src/infra/intercom_proxy`). The Explorer writes
`{APP_DIR}/session-info-<session_id>.json` after login and deletes it on a clean quit:

```json
{
  "version": 1,
  "session_id": "<the --session_id the launcher passed>",
  "wallet": "0x…",
  "explorer_version": "…",
  "identity": { "address": "0x…", "key": "…", "expiration": "…", "ephemeralAuthChain": [ … ] }
}
```

The launcher reads the file only in the report flow, refuses an expired identity, never logs
it, and deletes the file once the report is submitted or dismissed. Without a valid identity
the report reaches Sentry only. The proxy environment follows the install's `dclenv` (`org`
by default in production builds, `zone` otherwise).

## Delta updates

When an Explorer release carries a delta set, the launcher updates an installed build by
downloading per-file [HDiffPatch](https://github.com/sisong/HDiffPatch) patches instead of the
full zip. The producer and the contract live in unity-explorer (`scripts/delta`,
`docs/delta-updates.md`); the launcher side is `core/src/delta/` and the decision is made by the
launch FSM in `core/src/fsm/`.

For the fetched latest version the launcher reads
`<bucket>/@dcl/unity-explorer/releases/<tag>/delta/<platform>/manifest.json`. The manifest is
validated like the producer validates its builds (`core/src/delta/manifest.rs`): every path,
link target, digest and mode must obey the rules before it touches the disk. When the installed
version is one of its `bases`, the patches are downloaded under `downloads/delta/<tag>/`, each
checked by sha256, and `<app>/<tag>/` is built from the untouched `latest/` build plus the
patches, verifying every file against the manifest. The usual install commit then renames it to
`latest/` and keeps the previous build for rollback. Anything else takes the zip:

| Decision leaf | Meaning | Reported |
|---|---|---|
| `delta_disabled` | `--delta-updates off` | analytics only |
| `fresh_install` | nothing installed | analytics only |
| `manifest_missing` | the release has no delta set (404) | analytics only |
| `base_not_listed` | the installed version is not a base of the release | analytics only |
| `manifest_unavailable` | the manifest could not be fetched or is invalid | Sentry |

The strategy and its leaf go out as `UPDATE_STRATEGY_DECIDED`; a delta that fails mid-way falls
back to the zip within the same launch and sends `UPDATE_STRATEGY_FALLBACK` with the phase.

### Error codes

| Code | Meaning |
|---|---|
| `E4001_DELTA_PATCHER_UNAVAILABLE` | `hpatchz` is not beside the launcher executable |
| `E4003_DELTA_MANIFEST_UNAVAILABLE` | the manifest request failed with something other than 404 |
| `E4004_DELTA_MANIFEST_INVALID` | the manifest breaks the contract |
| `E4005_DELTA_INSTALLED_BUILD_DIFFERS` | a file of `latest/` is not the file the manifest expects |
| `E4006_DELTA_PATCH_DOWNLOAD_FAILED` | a patch could not be downloaded or its sha256 differs |
| `E4007_DELTA_PATCH_APPLY_FAILED` | `hpatchz` failed on a patch |
| `E4008_DELTA_BUILD_FAILED_VERIFICATION` | a rebuilt file does not match the manifest |
| `E4009_DELTA_INSUFFICIENT_SPACE` | the patches plus the new build do not fit on the disk |

Each is one Sentry issue; none is shown to the user, since the zip takes over.

### hpatchz sidecar

`scripts/pre-build-hpatchz.rs` downloads the HDiffPatch release pinned by sha256 (the same build
the producer encodes with) and stages `hpatchz` in `src-tauri/binaries/` as a second
`externalBin`. `npm run prebuild-sidecars` runs it with the watchdog script. Outside the bundle,
`DCL_HPATCHZ=<path>` points the launcher at an `hpatchz` binary.

### Running the integration tests locally

`core/tests/` runs the whole funnel against a bucket served from localhost, inside a temporary
app directory. The delta cases need the HDiffPatch tools and are skipped without them:

```bash
rust-script scripts/pre-build-hpatchz.rs      # stages src-tauri/target/hdiffpatch/<platform>/
export HDIFFZ=$PWD/src-tauri/target/hdiffpatch/windows64/hdiffz.exe
export DCL_HPATCHZ=$PWD/src-tauri/target/hdiffpatch/windows64/hpatchz.exe
cd core && cargo test
```

To try real releases, serve a folder laid out like the bucket (`@dcl/unity-explorer/releases/
<tag>/Decentraland_<platform>.zip`, a `latest.json`, and the `delta/` folder the unity-explorer
`delta` tool writes with `--local`) and run the windowless core binary against it:

```bash
DCL_LAUNCHER_DATA_DIR=/tmp/launcher-lab DCL_HPATCHZ=... \
  core/target/release/dcl_launcher_core --use-bucket-url http://127.0.0.1:8765 --skip-analytics
```

## Development Guidelines

- Use `npm run format` for formatting and linting in both Rust and JavaScript.
- Follow Conventional Commits: `feat:`, `fix:`, `chore:`
- Keep frontend TypeScript enum in sync with Rust `enum` for proper communication.


### Environment Variables

Environment variables are typically defined in the shell configuration.

Example:

```
~/.zshrc
```

This allows tools such as `cargo`, `vite`, and `tauri` to access configuration without project-specific IDE settings.


### Debugging

Debugging is performed directly on the compiled binary using `lldb`.

`lldb` is independent from Neovim or any editor plugins.
This approach keeps debugging reproducible outside the editor.


### Workflow

The project is intentionally structured so that **core logic is developed independently from the UI layer**.

Typical workflow:

1. Develop and test the **core Rust logic** separately.

2. Build the Rust components

3. Run the frontend development server

4. Launch or debug the binary using `lldb`.


### Frontend / Core Separation

The architecture separates application logic from rendering.

* **Core**
  Portable Rust logic with no frontend dependency.

* **Frontend**
  UI layer using Vite and Tauri.

Because of this separation:

* Core logic can be reused with other frontends.
* The UI can be mocked during development.


### Mocking UI Interaction

Frontend rendering commands can be mocked by overriding the application state.

Example location:

```
Home.tsx:77
```

The UI exposes a **single entry point for state updates**.

This approach is inspired by the Elm architecture.

Key idea:

* a **single update loop**
* explicit state transitions
* avoidance of fragmented state typical in MVC/MVVM architectures.

Reference:

[https://guide.elm-lang.org/architecture/](https://guide.elm-lang.org/architecture/)


### Logs

You can find the logs in this location

MacOS
```
~/Library/Logs/DecentralandLauncherLight/
```

Windows
```
C:\Users\<YourUsername>\AppData\Roaming\DecentralandLauncherLight\
```

### App Directory

Launcher operates within a single directory

MacOS
```
~/Library/Application Support/DecentralandLauncherLight/
```

Windows
```
C:\Users\<YourUsername>\AppData\Local\DecentralandLauncherLight\
```

`DCL_LAUNCHER_DATA_DIR=<path>` replaces it, which the integration tests and local experiments
use to keep a real install untouched.

### Command Line Arguments (CLI)

The application supports command-line arguments.
The complete and up-to-date list is defined here:
https://github.com/decentraland/launcher-rust/blob/main/core/src/environment.rs

`--crash-report-with-attachment <path>` is reserved for `dcl_watchdog`: it selects the
crash-report dialog flow and points at the crash attachment JSON to report.

`--delta-updates off` makes every update download the full zip (see "Delta updates").
`--use-bucket-url <url>` replaces the compiled-in bucket for every release artifact.

### Configuration File

The configuration file is located at `{APP_DIR}/config.json` and uses JSON format.

Example:

```json
{
  "analytics-user-id": "f31c8100-xxxx-xxxx-xxxx-46fc9b13ed0e",
  "client-additional-arguments": "--example-client-arg",
  "cmd-arguments": "--example-launcher-arg",
  "crash-report-dialog-disabled": false
}
```

#### Fields

- **analytics-user-id**
  UUID used as a stable analytics user identifier.

- **client-additional-arguments**
  A string of arguments passed directly to the client on launch.
  Parsed using the same rules as terminal argument strings.

- **cmd-arguments**
  A string of arguments applied by the launcher in addition to the CLI arguments
  for the current execution.
  Parsed using the same rules as terminal argument strings.

- **crash-report-dialog-disabled**
  Boolean. Set by "Don't show this again" on the crash-report dialog. When `true`
  the crash watchdog still records an unexpected Explorer exit but does not reopen
  the launcher to ask for a report. Delete the key or set `false` to re-enable.

#### Usage Examples

- Test a specific build version and suppress the version-check popup using:
  `--skip-version-check`

- Control the updater behavior:
  - Disable updates: `--never-trigger-updater`
  - Force updates: `--always-trigger-updater`

### Customisation of the .dmg installer

Customisation for .dmg on macOS is performed by the CI step: Build & replace custom DMG (macOS). Be aware that building locally won't trigger .dmg customisation and you will get partly customized output from the tauri build command.

### Auto authentitication process

Launcher has the auto-authentication on first launch feature. Launcher installer carries an embedded one-time UUID auth token that allows new users to get logged in automatically on their first run, bypassing the manual login step.

The implementation includes:
- Embedded token in installer: The installers (both .exe on Windows and .dmg on macOS) are provided with an embedded short-lived token unique per user's download. On Windows, payload with the token and its length is appended to the signed installer executable without breaking its code signature. On macOS, the download URL itself contains the token, which the system stores in the file’s metadata (the WhereFrom attribute).
- Token extraction logic: Upon installation, the launcher decodes the embedded token using platform-specific strategies (detailed below). On Windows, the NSIS installer triggers a separate rust executable that is shipped alongside the main installer and it reads the installer file’s tail end (last 4 bytes to determine token length, then the preceding bytes for the token in UTF-8). On macOS, user opens the .app file inside .dmg and the .app automatically does: self installation to `/Applications/` folder, reading url from the where from attr (if exists) via xattr to obtain the token.
- Propagation to Explorer client: After extraction, the launcher saves the token into a temporary text file `auth-token-bridge.txt` in its working directory. The client (with changes from decentraland/unity-explorer#5636) will detect this file on startup and consume the token to authenticate the user automatically. This approach bridges the launcher and Unity client, allowing the token to be used for login without manual input.
- First-launch auto login: With the token handed off, the Unity client uses it to validate the user’s session via the backend auth service and logs the user in on first launch. If the token is valid, the user lands in-world signed in with their account – no sign-in prompt needed. The token is a one-time credential. If the token is missing, invalid, or expired, the client will safely fall back to the normal login flow, so there’s no change in behaviour for the classic authentication approach.

#### Consumption via .txt token file

Once the token is extracted on either platform, the launcher uses a simple file-based bridge to hand it off
to the Explorer client:
- The token is saved to a file named auth-token-bridge.txt located in the launcher’s directory (alongside the Unity client executable). This file contains the UUID token string.
- On the next launch, the Unity Explorer client (with the changes from PR #5636) looks for this file on startup. If found, it reads the token and uses it to automatically authenticate the user. Internally, the client sends the token to the authentication backend to validate it and fetch the user’s identity, then logs the user in to their account, skipping the usual login UI.
- After a successful login, the client signals that the token was consumed by deleting the `auth-token-bridge.txt` . The token is only used once on first launch.

### Deeplinks

The launcher registers the `decentraland://` scheme. Every link opened by the OS (from a website, a
Discord message, etc.) goes to the launcher. If the launcher is already
open, the link is forwarded to it and starts the same flow again.

The launcher does not modify or alter the deeplink before passing it to the Explorer.

#### Startup vs runtime deeplinks

The launcher picks one of two paths (`should_use_deeplink_bridge` in `core/src/deeplink_bridge.rs`):

- **Startup deeplink.** No Explorer started by the launcher is running. The launcher updates the build
  if needed and starts the Explorer with the deeplink as the first command-line argument. The Explorer
  parses it like any other launch argument
  ([`ApplicationParametersParser`](https://github.com/decentraland/unity-explorer/blob/dev/Explorer/Assets/DCL/Infrastructure/Global/AppArgs/ApplicationParametersParser.cs)).
- **Runtime deeplink.** An Explorer is already running, for example the user is in-world and clicks
  "Jump in" on a website. Opening a second client would be wrong, so the launcher hands the link to the
  running one through a file (see below). The client applies it in place, e.g. teleports to the
  `position`.

So the runtime path only covers links that arrive **after** the Explorer has started; the first launch
always uses command-line arguments.

These keys skip the bridge and always start a new Explorer: `open-deeplink-in-new-instance=true`,
`multi-instance=true` and `local-scene=true` in the deeplink, or `--open-deeplink-in-new-instance`,
`--multi-instance` and `--local-scene` as launcher arguments. The launcher treats deeplink parameters
like its own command-line arguments and adjusts its behavior accordingly.

#### File-based bridge

1. The launcher writes `{APP_DIR}/deeplink-bridge.json`:
   ```json
   { "deeplink": "decentraland://?position=10,20" }
   ```
2. The running Explorer polls for the file
   ([`DeepLinkSentinel`](https://github.com/decentraland/unity-explorer/blob/dev/Explorer/Assets/DCL/RuntimeDeepLink/DeepLinkSentinel.cs)),
   deletes it and applies the link
   ([`DeepLinkHandleImplementation`](https://github.com/decentraland/unity-explorer/blob/dev/Explorer/Assets/DCL/RuntimeDeepLink/DeepLinkHandleImplementation.cs)).
3. The launcher waits for the file to disappear. Once it does, on macOS it brings the Explorer window to
   the front.
4. If the file is still there after 15 seconds (`OPEN_DEEPLINK_TIMEOUT` in
   `core/src/deeplink_bridge.rs`), the launcher removes it and reports `E3001_OPEN_DEEPLINK_TIMEOUT`.
   This happens whenever the Explorer does not consume the file in time, for example because its main
   thread is blocked or it cannot reach the file. The timeout is not
   retried. A `signin` deeplink is the exception: the client keeps it until the login screen claims it,
   so the launcher leaves the file in place and finishes without an error.

"Already running" means a process the launcher started itself, tracked in
`{APP_DIR}/running-instances.json`. An Explorer started by hand or from the Unity editor is not in that
list, so a deeplink would start a second client. Add `bridgeOnly=true` to the deeplink (or
`--bridgeOnly` to the launcher arguments) to always write the bridge file instead, whatever is
running. In that mode the launcher does not bring any window to the front.

#### Which build runs

The launcher always starts the build in `{APP_DIR}/latest/`. `{APP_DIR}/version.json` records which
version that is, next to the install time of each version:

```json
{ "version": "v0.111.0", "v0.111.0": "1760000000", "v0.110.0": "1759000000" }
```

Other version folders next to it (e.g. `v0.110.0/`) are kept for rollback only and are never launched.
The launcher keeps the two most recent of them and deletes older ones.

On every launch it compares `version.json` with the latest release. If the Explorer executable in
`latest/` is missing, or the latest release is a different version, it downloads that release and
replaces `latest/`.
Deleting `latest/` is therefore safe: the next launch restores it.

#### Testing a deeplink with your own build

- **Build running outside the launcher (Unity editor or a local build).** Start your build, then open
  `decentraland://?position=10,20&bridgeOnly=true`. The launcher writes the bridge file and your
  running build consumes it. The installed build stays untouched. The Explorer always looks for the
  bridge file in the default App Directory, so this does not work with `DCL_LAUNCHER_DATA_DIR`.
- **Startup deeplinks through the launcher.** Replace the contents of `{APP_DIR}/latest/` with your
  build and keep `version.json` as it is. The launcher starts whatever is in `latest/`. Your build is
  replaced as soon as a newer release is published; delete `latest/` when you are done to get the
  official build back on the next launch.

`{APP_DIR}` is the [App Directory](#app-directory).


## License

Licensed under the Apache License 2.0. See `LICENSE` file for details.
