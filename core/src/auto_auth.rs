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

        let resolved_path = resolve_dmg_file(dmg_mount_path.as_path())?;
        let where_from = where_from_attr(resolved_path.as_path())?;

        log::info!("Where from attr: {where_from:?}");

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

#[allow(unsafe_code)]
#[cfg(target_os = "macos")]
fn resolve_dmg_file(mntfrom: &Path) -> Result<PathBuf> {
    use core_foundation::{
        base::{CFRelease, TCFType},
        string::CFString,
        url::CFURL,
    };
    use core_foundation_sys::base::CFTypeRef;
    use core_foundation_sys::url::{CFURLCopyFileSystemPath, kCFURLPOSIXPathStyle};
    use std::{ffi::CString, path::PathBuf, ptr};

    unsafe {
        let session = DASessionCreate(ptr::null());
        if session.is_null() {
            return Err(anyhow::anyhow!("Could not create DASession"));
        }

        let dev_c = CString::new(mntfrom.to_string_lossy().to_string())?;
        let disk: CFTypeRef = DADiskCreateFromBSDName(ptr::null(), session, dev_c.as_ptr());
        if disk.is_null() {
            return Err(anyhow::anyhow!(
                "Could not create DADisk for {}",
                mntfrom.display()
            ));
        }

        let desc = DADiskCopyDescription(disk);
        if desc.is_null() {
            return Err(anyhow::anyhow!("Could not get disk description"));
        }

        let key = CFString::new("DAMediaPath");
        let val = core_foundation_sys::dictionary::CFDictionaryGetValue(
            desc,
            key.as_concrete_TypeRef().cast(),
        );
        if val.is_null() {
            return Err(anyhow::anyhow!("DAMediaPath not found"));
        }

        let url: CFURL = CFURL::wrap_under_get_rule(val.cast());
        let cfstr = CFURLCopyFileSystemPath(url.as_concrete_TypeRef(), kCFURLPOSIXPathStyle);
        let cfstring = CFString::wrap_under_create_rule(cfstr);

        CFRelease(desc.cast());
        CFRelease(disk.cast());
        CFRelease(session .cast());

        Ok(PathBuf::from(cfstring.to_string()))
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
#[link(name = "DiskArbitration", kind = "framework")]
unsafe extern "C" {
    pub fn DASessionCreate(
        allocator: core_foundation_sys::base::CFAllocatorRef,
    ) -> core_foundation_sys::base::CFTypeRef;

    pub fn DADiskCreateFromBSDName(
        allocator: core_foundation_sys::base::CFAllocatorRef,
        session: core_foundation_sys::base::CFTypeRef,
        bsdName: *const std::os::raw::c_char,
    ) -> core_foundation_sys::base::CFTypeRef;

    pub fn DADiskCopyDescription(
        disk: core_foundation_sys::base::CFTypeRef,
    ) -> core_foundation_sys::dictionary::CFDictionaryRef;
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

    #[test]
    fn test_resolve_dmg_file_integration() -> Result<()> {
        let path = std::option_env!("TEST_MNT_FROM");
        let Some(path) = path else {
            println!("TEST_MNT_FROM is not provided, ignoring test");
            return Ok(());
        };

        let path = Path::new(path);
        let resolved = resolve_dmg_file(path)?;
        println!("Image path: {resolved:?}");
        Ok(())
    }
}
