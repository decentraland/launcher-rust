//! Delta updates: moving an installed Explorer build to the latest release with per-file
//! `HDiffPatch` patches instead of the full zip.
//!
//! The producer side lives in unity-explorer `scripts/delta` and is documented in its
//! `docs/delta-updates.md`. Everything here is pure where it can be, with I/O at the edges.

pub mod manifest;
