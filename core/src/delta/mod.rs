//! Delta updates: per-file `HDiffPatch` patches instead of the full zip.
//!
//! The producer lives in unity-explorer `scripts/delta`, documented in its `docs/delta-updates.md`.
//! TODO: we will need to merge 2 impls into one, it shouldn't be a problem since both are in rust

pub mod apply;
pub mod download;
pub mod fetch;
pub mod manifest;
pub mod patcher;

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use self::manifest::Sha256Digest;

const HASH_BUFFER_BYTES: usize = 1 << 20;

pub fn sha256_file(path: &Path) -> io::Result<Sha256Digest> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    let digest: [u8; 32] = hasher.finalize().into();
    Ok(Sha256Digest::from(digest))
}
