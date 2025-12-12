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

## Installation

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Node.js
brew install node # Or use your favorite package manager

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

## Development Guidelines

- Use `npm run format` for formatting and linting in both Rust and JavaScript.
- Follow Conventional Commits: `feat:`, `fix:`, `chore:`
- Keep frontend TypeScript enum in sync with Rust `enum` for proper communication.


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

### Command Line Arguments (CLI)

The application supports command-line arguments.
The complete and up-to-date list is defined here:
https://github.com/decentraland/launcher-rust/blob/main/core/src/environment.rs

### Configuration File

The configuration file is located at `{APP_DIR}/config.json` and uses JSON format.

Example:

```json
{
  "analytics-user-id": "f31c8100-xxxx-xxxx-xxxx-46fc9b13ed0e",
  "client-additional-arguments": "--example-client-arg",
  "cmd-arguments": "--example-launcher-arg"
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


## License

Licensed under the Apache License 2.0. See `LICENSE` file for details.
