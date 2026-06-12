# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Decentraland Launcher — a cross-platform (macOS/Windows) desktop app built with **Tauri v2** that manages installation, updating, and launching of the Decentraland Explorer client. Rust backend, React/TypeScript frontend, Tauri IPC bridge.

## Build & Development Commands

```bash
npm install                  # Install all dependencies
npm run tauri dev            # Run in development mode
npm run tauri build          # Build for distribution
npm run format               # Format JS/TS (Prettier) + check Rust formatting
npm run analyze              # Run clippy on all three Rust crates with -D warnings
```

**Rust-only commands** (run from `core/`, `src-tauri/`, or `src-auto-auth/`):
```bash
cargo fmt                    # Format Rust code
cargo check                  # Type-check
cargo clippy --all-targets --all-features -- -D warnings
cargo test                   # Run tests for that crate
```

The pre-commit hook (`.githooks/pre-commit`) runs fmt, check, clippy, and test on all three Rust crates.

## Architecture

```
Frontend (React/TS)  ←—Tauri IPC Channel—→  Backend (Rust)
    src/                                     core/src/
    components/Home/Home.tsx                  flow.rs (orchestration)
                                             installs.rs → downloads.rs → compression.rs
                                             types.rs (Status/Step enums serialized to TS)
```

**Three Rust crates** (no workspace — independent Cargo.toml files):
- **`core/`** (`dcl-launcher-core`) — Main business logic: launch flow orchestration, downloads, installation, S3 version fetching, analytics, error handling, deep linking, auto-auth
- **`src-tauri/`** (`Decentraland-Launcher`) — Tauri app shell: IPC command handlers (`launch`, `retry`), self-update logic, deep link setup
- **`src-auto-auth/`** — Platform-specific token extraction (macOS DMG xattr, Windows PE tail)

**Frontend** (`src/`): React + Vite. Main UI in `components/Home/Home.tsx`. TypeScript types in `components/Home/types.ts` mirror Rust enums in `core/src/types.rs` — these must stay in sync.

**Core flow** (`core/src/flow.rs`): Fetch → Download → Install → Launch, with retry logic and status reporting over Tauri channels.

## Rust Conventions

**Rust edition 2024** for `core` and `src-auto-auth`, **2021** for `src-tauri`. Minimum Rust version: **1.88.0**.

**Strict clippy enforcement** (see `core/src/lib.rs`):
- `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::todo, clippy::dbg_macro)]`
- `#![warn(clippy::all, clippy::pedantic, clippy::nursery)]`
- Use `?` operator and proper error handling instead of unwrap/expect/panic
- Use `.get()` for safe indexing instead of `[]`

**Error handling**: Custom `StepError` enum in `core/src/errors.rs` with error codes (E0000–E3002). Use `anyhow` for generic errors, `thiserror` for enum derives.

**Serde**: Use `#[serde(rename_all = "camelCase")]` for types crossing the IPC boundary to match JavaScript conventions.

**Platform-specific code**: Use `#[cfg(target_os = "macos")]` and `#[cfg(target_os = "windows")]` for platform-conditional logic.

## Commit Convention

Conventional Commits: `feat:`, `fix:`, `chore:` with optional scopes like `(release)`, `(windows)`, `(macos)`, `(auto-auth)`.
