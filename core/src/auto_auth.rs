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
        let is_dmg = Self::is_running_from_dmg(&path)?;
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
    fn is_running_from_dmg(exe_path: &Path) -> Result<bool> {
        use std::ffi::CString;
        use std::ffi::CStr;
        use libc::statfs;
        use libc::MNT_RDONLY;

        let cpath = CString::new(exe_path.to_string_lossy().to_string())?;
        let mut sfs: statfs = unsafe { std::mem::zeroed() };

        let res = unsafe { statfs(cpath.as_ptr(), &mut sfs) };
        if res != 0 {
            return Err(anyhow::anyhow!("Cannot read statfs"));
        }

        let mntfrom = unsafe { CStr::from_ptr(sfs.f_mntfromname.as_ptr()) }
            .to_string_lossy()
            .to_string();
        let mnton = unsafe { CStr::from_ptr(sfs.f_mntonname.as_ptr()) }
            .to_string_lossy()
            .to_string();

        let is_readonly = sfs.f_flags & MNT_RDONLY as u32 != 0;
        log::info!("exe mount data, from: {mntfrom}, on: {mnton}, readonly: {is_readonly}");

        Ok(mntfrom.starts_with("/dev/disk")
            && mnton.starts_with("/Volumes/")
            && is_readonly)
    }
}
