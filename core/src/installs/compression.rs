use crate::errors::{DCLError, DCLErrorResult};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
use tar::Archive;
use zip::read::ZipArchive;

/// Archives are not required to carry explicit directory entries — a legal
/// zip/tar can list only files with nested paths, so every file's parent
/// chain must be created before the file itself.
fn new_file_with_parent(path: &Path) -> std::io::Result<fs::File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::File::create(path)
}

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

        // Extract the TAR contents
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.to_path_buf();
            let output_path = Path::new(destination_path).join(path);

            if entry.header().entry_type().is_dir() {
                fs::create_dir_all(output_path)?;
            } else {
                let mut output_file = new_file_with_parent(&output_path)?;
                std::io::copy(&mut entry, &mut output_file)?;
            }
        }
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

                let mut output_file = new_file_with_parent(&output_path)?;
                output_file.write_all(&content)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Legal zips may list only files with nested paths and no directory
    /// entries — extraction must create the missing parents itself.
    #[test]
    fn decompress_creates_missing_parent_dirs() -> anyhow::Result<()> {
        use zip::write::SimpleFileOptions;

        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("nested/deeper/file.txt", SimpleFileOptions::default())?;
        writer.write_all(b"payload")?;
        let bytes = writer.finish()?.into_inner();

        let base = std::env::temp_dir().join(format!("dcl-decompress-test-{}", std::process::id()));
        let source = base.join("archive.zip");
        let destination = base.join("out");
        fs::create_dir_all(&base)?;
        fs::write(&source, bytes)?;

        let result = decompress_file(&source, &destination);
        let payload = fs::read(destination.join("nested/deeper/file.txt"));
        fs::remove_dir_all(&base)?;

        result?;
        assert_eq!(payload?.as_slice(), b"payload");
        Ok(())
    }
}
