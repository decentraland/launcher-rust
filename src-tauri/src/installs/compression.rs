use std::path::PathBuf;
use std::{fs, path::Path, io::Read};
use std::io::Cursor;
use tokio::fs::{create_dir_all, File};
use tokio::io::AsyncWriteExt;
use zip::read::ZipArchive;
use tar::Archive;

pub async fn decompress_file(source_path: &PathBuf, destination_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if !source_path.exists() {
        return Err("Source file does not exist".into());
    }

    create_dir_all(destination_path).await?;

    let zip_data = fs::read(source_path)?;
    let cursor = Cursor::new(zip_data);

    let mut zip = ZipArchive::new(cursor)?;

    let mut tar_file_data: Option<Vec<u8>> = None;

    // Iterate through the ZIP files to find the tar file
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        if file.name().ends_with(".tar") {
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
                create_dir_all(output_path).await?;
            } else {
                let mut output_file = std::fs::File::create(output_path)?;
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
                create_dir_all(&output_path).await?;
            } else {
                let mut content = Vec::new();
                file.read_to_end(&mut content)?;

                let mut output_file = File::create(output_path).await?;
                output_file.write_all(&content).await?;
            }
        }
    }

    Ok(())
}

