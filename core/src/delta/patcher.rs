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
            .arg(long_path(old)?)
            .arg(long_path(patch)?)
            .arg(long_path(out)?)
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

/// `hpatchz` hands its paths to `CreateFileW` as given, and Windows refuses a plain path past
/// `MAX_PATH` (260) unless `LongPathsEnabled` is set machine-wide, which it rarely is. A verbatim
/// `\\?\` path skips that check; it is what `std::fs` does internally for its own calls. The
/// prefix also skips normalization, so `to_verbatim` makes the path absolute with `\`
/// separators first.
#[cfg(windows)]
fn long_path(path: &Path) -> std::io::Result<PathBuf> {
    omnipath::windows::WinPathExt::to_verbatim(path)
}

/// Only Windows has a path limit `hpatchz` trips over.
#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
fn long_path(path: &Path) -> std::io::Result<PathBuf> {
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[rstest::rstest]
    #[case(r"C:\Users\a\x.hdiff", r"\\?\C:\Users\a\x.hdiff")]
    #[case(
        r"C:\Users\a\delta\from/v1/b/x.hdiff",
        r"\\?\C:\Users\a\delta\from\v1\b\x.hdiff"
    )]
    #[case(r"C:\Users\a\..\b\x.hdiff", r"\\?\C:\Users\b\x.hdiff")]
    #[case(r"\\server\share\x.hdiff", r"\\?\UNC\server\share\x.hdiff")]
    #[case(r"\\?\C:\already\verbatim", r"\\?\C:\already\verbatim")]
    fn long_path_is_verbatim_and_normalized(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(
            long_path(Path::new(input)).ok(),
            Some(PathBuf::from(expected))
        );
    }

    #[cfg(windows)]
    #[test]
    fn long_path_makes_a_relative_path_absolute() {
        let path = long_path(Path::new("x.hdiff")).ok();
        let cwd = std::env::current_dir().ok();
        let expected = cwd.map(|cwd| PathBuf::from(format!(r"\\?\{}\x.hdiff", cwd.display())));
        assert_eq!(path, expected);
    }

    #[cfg(not(windows))]
    #[test]
    fn long_path_is_unchanged_off_windows() {
        let path = Path::new("/tmp/delta/from/v1/x.hdiff");
        assert_eq!(long_path(path).ok().as_deref(), Some(path));
    }
}
