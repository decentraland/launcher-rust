// Avoid popup terminal window
#![windows_subsystem = "windows"]

use std::{
    fs::File,
    io,
    io::{Read, Seek, SeekFrom},
    process::exit,
};

use dcl_launcher_core::{
    anyhow::{Result, anyhow},
    auto_auth::auth_token_storage::AuthTokenStorage,
    log, logs,
};

fn main() {
    if let Err(e) = logs::dispath_logs() {
        eprintln!("Cannot initialize logs: {e}");
        exit(1);
    }
    if let Err(e) = main_internal() {
        log::error!("Error occurred running auto auth script: {e}");
    }
}

fn main_internal() -> Result<()> {
    log::info!("Start auto auth script v{}", std::env!("CARGO_PKG_VERSION"));
    if AuthTokenStorage::has_token() {
        log::info!("Token already installed");
        return Ok(());
    }

    let args: Vec<String> = std::env::args().collect();
    log::info!("Args: {args:?}");

    let installer_path = args
        .first()
        .ok_or_else(|| anyhow!("Installer path is not provided"))?;

    let token = read_token(installer_path)?;
    AuthTokenStorage::write_token(token.as_str())?;
    log::info!("Token write complete");
    Ok(())
}

// MAGIC (8B)      = ASCII "DCLSIGv1"
// DATA  (LEN B)   = UTF-8 of token (UUIDv4)
// LEN   (4B LE)   = length of DATA (uint32)
pub fn read_token(path: &str) -> io::Result<String> {
    let mut file = File::open(path)?;
    let file_size = file.metadata()?.len();

    // Seek to the last 4 bytes (LEN field)
    file.seek(SeekFrom::End(-4))?;

    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as u64;

    // Seek backward to read the DATA (token UTF-8 string)
    // Total to read: MAGIC(8) + DATA(len) + LEN(4)
    let trailer_size = 8 + len + 4;
    if trailer_size > file_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "File too small for trailer",
        ));
    }

    file.seek(SeekFrom::End(-(trailer_size as i64)))?;

    let mut trailer = vec![0u8; trailer_size as usize];
    file.read_exact(&mut trailer)?;

    // Validate MAGIC and extract DATA
    let magic = &trailer[..8];
    if magic != b"DCLSIGv1" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid trailer magic",
        ));
    }

    let data = &trailer[8..(8 + len as usize)];
    let token = std::str::from_utf8(data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid UTF-8 in token"))?
        .to_owned();

    Ok(token)
}
