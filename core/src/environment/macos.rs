#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use anyhow::{Result, anyhow};

#[cfg(target_os = "macos")]
pub fn is_running_from_dmg() -> Result<bool> {
    let path = std::env::current_exe()?;
    let dmg_mount_path = dmg_mount_path(&path)?;
    Ok(dmg_mount_path.is_some())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub fn dmg_mount_path(exe_path: &Path) -> Result<Option<PathBuf>> {
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

    let refer_mnt = &mnton;

    if is_readonly && refer_mnt.to_lowercase().starts_with("/volumes/") {
        Ok(Some(refer_mnt.into()))
    } else {
        Ok(None)
    }
}

#[allow(unsafe_code)]
#[cfg(target_os = "macos")]
pub fn where_from_attr(dmg_path: &Path) -> Result<Option<Vec<String>>> {
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
pub fn dmg_backing_file(mount_point: &str) -> Result<Option<PathBuf>> {
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
