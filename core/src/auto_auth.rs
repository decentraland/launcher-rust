use anyhow::Result;

#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

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
        let dmg_mount_path = dmg_mount_path(&path)?;
        log::info!("Exe is running from dmg: {dmg_mount_path:?}");

        let Some(dmg_mount_path) = dmg_mount_path else {
            return Ok(());
        };

        let Some(dmg_dir) = dmg_mount_path.parent() else {
            log::info!("Dmg doesn't have a parent");
            return Ok(());
        };
        log::info!("Dmg parent: {}", dmg_dir.display());

        //let dmg_dir = dmg_dir.to_str();
        //TODO dmg_dir to real file path -> call where_from_attr
        let _ = where_from_attr(dmg_dir);

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
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn dmg_mount_path(exe_path: &Path) -> Result<Option<PathBuf>> {
    use libc::MNT_RDONLY;
    use libc::statfs;
    use std::ffi::CStr;
    use std::ffi::CString;

    let cpath = CString::new(exe_path.to_string_lossy().to_string())?;
    let mut sfs: statfs = unsafe { std::mem::zeroed() };

    let res = unsafe { statfs(cpath.as_ptr(), &raw mut sfs) };
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

    if is_readonly && mntfrom.to_lowercase().starts_with("/volumes/") {
        Ok(Some(mntfrom.into()))
    } else {
        Ok(None)
    }
}

#[allow(unsafe_code)]
#[cfg(target_os = "macos")]
fn where_from_attr(dmg_path: &Path) -> Result<Option<Vec<String>>> {
    use libc::getxattr;
    use plist::Value;
    use std::{ffi::CString, ptr};

    let cpath = CString::new(dmg_path.to_string_lossy().to_string())?;
    let attr = CString::new("com.apple.metadata:kMDItemWhereFroms")?;

    // size
    let size = unsafe { getxattr(cpath.as_ptr(), attr.as_ptr(), ptr::null_mut(), 0, 0, 0) };
    if size <= 0 {
        return Err(anyhow::anyhow!("Cannot read size"));
    }

    // read contents
    #[allow(clippy::cast_sign_loss)]
    let mut buf = vec![0u8; size as usize];
    let ret = unsafe {
        getxattr(
            cpath.as_ptr(),
            attr.as_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            0,
            0,
        )
    };
    if ret <= 0 {
        return Err(anyhow::anyhow!("Cannot read xattr"));
    }

    // Decode binary plist to array of strings
    let mut cursor = std::io::Cursor::new(&buf[..]);
    let val = Value::from_reader(&mut cursor)?;

    if let Some(arr) = val.into_array() {
        let urls: Vec<String> = arr.into_iter().filter_map(plist::Value::into_string).collect();
        if urls.is_empty() {
            Ok(None)
        } else {
            Ok(Some(urls))
        }
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_where_from_integration() -> Result<()> {
        let path = std::option_env!("TEST_DMG_PATH");
        let Some(path) = path else {
            println!("TEST_DMG_PATH is not provided, ignoring test");
            return Ok(());
        };

        let path = Path::new(path);
        let attr = where_from_attr(path)?;
        println!("Where from attr: {attr:?}");
        Ok(())
    }
}
