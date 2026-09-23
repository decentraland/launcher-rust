use crate::errors::{DCLError, DCLErrorResult};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
use tar::Archive;
use zip::read::ZipArchive;

pub fn decompress_file(source_path: &PathBuf, destination_path: &PathBuf) -> DCLErrorResult {
    if !source_path.exists() {
        return DCLError::E1001_FILE_NOT_FOUND {
            expected_path: Some(source_path.to_string_lossy().into_owned()),
        }
        .into();
    }

    fs::create_dir_all(destination_path)?;

    let zip_data = fs::read(source_path)?;
    let cursor = Cursor::new(zip_data);

    let mut zip = ZipArchive::new(cursor)?;

    let mut tar_file_data: Option<Vec<u8>> = None;

    // Iterate through the ZIP files to find the tar file
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        if file.name().to_lowercase().ends_with(".tar") {
            let mut tar_data = Vec::new();
            file.read_to_end(&mut tar_data)?;
            tar_file_data = Some(tar_data);
            break;
        }
    }

    // If a TAR file was found inside the ZIP, extract it
    if let Some(tar_file_data) = tar_file_data {
        let mut archive = Archive::new(tar_file_data.as_slice());

        // unpack applies each entry's recorded Unix mode bits - the .app
        // ships executables outside Contents/MacOS (e.g. PlugIns/uuav-helper)
        // that must keep +x to be spawnable - and handles directories,
        // symlinks and parent creation, refusing paths that escape the
        // destination.
        archive.unpack(destination_path)?;
    } else {
        // If no TAR file found, extract the other files
        for i in 0..zip.len() {
            let mut file = zip.by_index(i)?;
            let output_path = Path::new(destination_path).join(file.name());

            // Create directory if it's a directory
            if file.is_dir() {
                fs::create_dir_all(&output_path)?;
            } else {
                let mut content = Vec::new();
                file.read_to_end(&mut content)?;

                let mut output_file = fs::File::create(output_path)?;
                output_file.write_all(&content)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    const HELPER_PATH: &str = "build/Decentraland.app/Contents/PlugIns/uuav-helper";
    const HELPER_CONTENT: &[u8] = b"helper bytes";
    const DATA_PATH: &str = "build/Decentraland.app/Contents/Resources/Data/config";
    const DATA_CONTENT: &[u8] = b"data bytes";

    fn append_tar_entry(
        tar: &mut tar::Builder<&mut Vec<u8>>,
        path: &str,
        content: &[u8],
        mode: u32,
    ) -> std::io::Result<()> {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        tar.append_data(&mut header, path, content)
    }

    /// A minimal `Decentraland_macos.zip`: a zip wrapping a single build.tar
    /// with one executable (0o755) and one regular (0o644) entry.
    fn zip_wrapped_tar() -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut tar_data = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_data);
            append_tar_entry(&mut tar, HELPER_PATH, HELPER_CONTENT, 0o755)?;
            append_tar_entry(&mut tar, DATA_PATH, DATA_CONTENT, 0o644)?;
            tar.finish()?;
        }

        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            zip.start_file("build.tar", zip::write::SimpleFileOptions::default())?;
            zip.write_all(&tar_data)?;
            zip.finish()?;
        }
        Ok(cursor.into_inner())
    }

    #[test]
    fn tar_entry_contents_and_permissions_survive_decompression() -> TestResult {
        let source_dir = std::env::temp_dir().join(format!(
            "dcl-launcher-decompress-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&source_dir);
        fs::create_dir_all(&source_dir)?;
        let source_path = source_dir.join("decentraland.zip");
        fs::write(&source_path, zip_wrapped_tar()?)?;
        let destination_path = source_dir.join("unpacked");

        decompress_file(&source_path, &destination_path)?;

        let helper = destination_path.join(HELPER_PATH);
        let data = destination_path.join(DATA_PATH);
        assert_eq!(fs::read(&helper)?, HELPER_CONTENT);
        assert_eq!(fs::read(&data)?, DATA_CONTENT);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let helper_mode = fs::metadata(&helper)?.permissions().mode();
            assert_eq!(helper_mode & 0o777, 0o755, "executable bit must survive");
            let data_mode = fs::metadata(&data)?.permissions().mode();
            assert_eq!(data_mode & 0o777, 0o644);
        }

        fs::remove_dir_all(&source_dir)?;
        Ok(())
    }
}
