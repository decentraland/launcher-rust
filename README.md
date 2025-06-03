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

## Development Guidelines

- Use `cargo fmt` and `clippy` for Rust formatting and linting.
- Use Prettier/ESLint for frontend.
- Follow Conventional Commits: `feat:`, `fix:`, `chore:`
- Keep frontend TypeScript enum in sync with Rust `enum` for proper communication.

## License

Licensed under the Apache License 2.0. See `LICENSE` file for details.
