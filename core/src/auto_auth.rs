use anyhow::Result;

#[cfg(target_os = "macos")]
use std::path::Path;

pub struct AutoAuth {}

impl AutoAuth {
    pub fn try_obtain_auth_token() {
        if let Err(e) = Self::obtain_token_internal() {
            log::error!("Obtain auth error: {e}");
        }
    }

    #[cfg(target_os = "macos")]
    fn obtain_token_internal() -> Result<()> {
        let path = std::env::current_exe()?;
        let parent = path.parent();

        let Some(parent) = parent else {
            log::info!("Exe has no parent and is not running in .dmg");
            return Ok(());
        };

        let is_dmg = Self::is_running_from_dmg(parent)?;
        log::info!("Exe is running from dmg: {is_dmg}");

        Ok(())
    }

    #[cfg(target_os = "windows")]
    // For dev only, remove later ====
    #[allow(clippy::unnecessary_wraps)]
    #[allow(clippy::missing_const_for_fn)]
    // ====
    fn obtain_token_internal() -> Result<()> {
        //TODO
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[allow(unsafe_code)]
    fn is_running_from_dmg(parent_path: &Path) -> Result<bool> {
        use nix::libc::statfs;
        use std::ffi::CString;

        let cpath = CString::new(parent_path.to_string_lossy().to_string())?;
        let mut sfs: statfs = unsafe { std::mem::zeroed() };
        unsafe {
            if statfs(cpath.as_ptr(), &raw mut sfs) == 0 {
                // HFS/ExFAT images mounted from DMG typically show up as "hfs", "apfs", etc
                // But the device path (sfs.f_mntfromname) starts with "/dev/disk"
                let mntfrom = std::ffi::CStr::from_ptr(sfs.f_mntfromname.as_ptr())
                    .to_string_lossy()
                    .to_string();
                Ok(mntfrom.starts_with("/dev/disk"))
            } else {
                Ok(false)
            }
        }
    }
}
