//! `hpatchz`, the `HDiffPatch` applier shipped as a sidecar next to the launcher executable.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, anyhow};

use crate::errors::{DCLError, DCLErrorTyped};

pub const PATCHER_BINARY_NAME: &str = "hpatchz";
/// Overrides the sidecar location; set by the integration tests and the CLI.
pub const PATCHER_ENV: &str = "DCL_HPATCHZ";

#[derive(Debug, Clone)]
pub struct Patcher {
    exe: PathBuf,
}

impl Patcher {
    /// `$DCL_HPATCHZ`, else `hpatchz` beside the launcher executable.
    pub fn locate() -> DCLErrorTyped<Self> {
        let exe = match std::env::var_os(PATCHER_ENV).filter(|value| !value.is_empty()) {
            Some(value) => PathBuf::from(value),
            None => {
                let launcher =
                    std::env::current_exe().context("Cannot resolve the launcher executable")?;
                let dir = launcher
                    .parent()
                    .ok_or_else(|| anyhow!("The launcher executable has no parent directory"))?;
                dir.join(format!(
                    "{PATCHER_BINARY_NAME}{}",
                    std::env::consts::EXE_SUFFIX
                ))
            }
        };
        if !exe.is_file() {
            return Err(DCLError::E4001_DELTA_PATCHER_UNAVAILABLE {
                path: exe.to_string_lossy().into_owned(),
            });
        }
        Ok(Self { exe })
    }

    /// Writes `out` as `old` plus `patch`; `old` may be an empty file for a whole-file patch.
    pub fn apply(&self, old: &Path, patch: &Path, out: &Path) -> anyhow::Result<()> {
        let mut command = Command::new(&self.exe);
        command
            .arg("-f")
            .arg(old)
            .arg(patch)
            .arg(out)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = command
            .output()
            .with_context(|| format!("cannot run {}", self.exe.display()))?;
        if !output.status.success() {
            return Err(anyhow!(
                "hpatchz exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(())
    }
}
