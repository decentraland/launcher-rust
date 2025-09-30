mod auth_token_storage;

use anyhow::{Result, anyhow};

#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

use auth_token_storage::AuthTokenStorage;

pub struct AutoAuth {}

impl AutoAuth {
    pub fn try_obtain_auth_token() {
        if AuthTokenStorage::has_token() {
            log::info!("Token already obtained");
            return;
        }

        match Self::obtain_token_internal() {
            Ok(token) => {
                let Some(token) = token else {
                    log::warn!("Token value is empty");
                    return;
                };

                log::info!("Token obtained");
                if let Err(e) = AuthTokenStorage::write_token(token.as_str()) {
                    log::error!("Cannot write token: {e}");
                }
            }
            Err(e) => {
                log::error!("Obtain auth token error: {e}");
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn obtain_token_internal() -> Result<Option<String>> {
        use anyhow::Context;
        use std::borrow::ToOwned;

        let path = std::env::current_exe()?;
        log::info!("Exe path: {}", path.display());
        let dmg_mount_path = dmg_mount_path(&path)?;
        log::info!("Exe is running from dmg: {dmg_mount_path:?}");

        let Some(dmg_mount_path) = dmg_mount_path else {
            return Ok(None);
        };

        let Some(dmg_dir) = dmg_mount_path.parent() else {
            return Err(anyhow!("Dmg doesn't have a parent"));
        };
        log::info!("Dmg parent: {}", dmg_dir.display());

        let dmg_file_path = dmg_backing_file(&dmg_dir.to_string_lossy())
            .with_context(|| "Cannot resolve mount path: {dmg_dir}")?
            .ok_or_else(|| anyhow!("Dmg original file not found"))?;
        let where_from = where_from_attr(dmg_file_path.as_path())
            .with_context(|| "Cannot read where from attr: {dmg_file_path}")?;

        log::info!("Where from attr: {where_from:?}");

        let Some(where_from) = where_from else {
            return Err(anyhow!("Dmg does not have where from data"));
        };

        // TODO trim redundant data and purify token
        let token = where_from
            .first()
            .map(ToOwned::to_owned);

        Ok(token)
    }

    #[cfg(target_os = "windows")]
    // For dev only, remove later ====
    #[allow(clippy::unnecessary_wraps)]
    #[allow(clippy::missing_const_for_fn)]
    // ====
    fn obtain_token_internal() -> Result<Option<String>> {
        //TODO
        Err(anyhow!("Not implemented"))
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
        let urls: Vec<String> = arr
            .into_iter()
            .filter_map(plist::Value::into_string)
            .collect();
        if urls.is_empty() {
            Ok(None)
        } else {
            Ok(Some(urls))
        }
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
fn dmg_backing_file(mount_point: &str) -> Result<Option<PathBuf>> {
    let output = std::process::Command::new("hdiutil")
        .args(["info", "-plist"])
        .output()?;
    let plist = plist::Value::from_reader_xml(&*output.stdout)?;
    let dict = plist
        .as_dictionary()
        .ok_or_else(|| anyhow!("Cannot convert plist to dictionary"))?;
    let images = dict
        .get("images")
        .ok_or_else(|| anyhow!("No images found in plist"))?
        .as_array()
        .ok_or_else(|| anyhow!("Images is not an array"))?;
    for image in images {
        if let Some(props) = image.as_dictionary() {
            if let (Some(img_path), Some(system_entities)) =
                (props.get("image-path"), props.get("system-entities"))
            {
                if let (Some(img_path), Some(system_entities)) =
                    (img_path.as_string(), system_entities.as_array())
                {
                    for ent in system_entities {
                        if let Some(ent) = ent.as_dictionary() {
                            if let Some(mount_point_str) =
                                ent.get("mount-point").and_then(|v| v.as_string())
                            {
                                if mount_point_str == mount_point {
                                    return Ok(Some(PathBuf::from(img_path)));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

#[cfg(target_os = "macos")]
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

    #[test]
    fn test_dmg_mount_path_integration() -> Result<()> {
        let path = std::option_env!("TEST_EXE_MOUNT_PATH");
        let Some(path) = path else {
            println!("TEST_EXE_MOUNT_PATH is not provided, ignoring test");
            return Ok(());
        };

        let path = Path::new(path);
        let dmg_mount_path = dmg_mount_path(path)?;
        println!("Exe is running from dmg: {dmg_mount_path:?}");
        Ok(())
    }

    #[test]
    fn test_dmg_backing_file_integration() -> Result<()> {
        let path = std::option_env!("TEST_DMG_BACKING");
        let Some(path) = path else {
            println!("TEST_DMG_BACKING is not provided, ignoring test");
            return Ok(());
        };

        let dmg_mount_path = dmg_backing_file(path)?;
        println!("Exe is running from backing dmg: {dmg_mount_path:?}");
        Ok(())
    }
}
