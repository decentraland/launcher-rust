use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::process::exit;

use anyhow::Result;
use anyhow::anyhow;
use dcl_launcher_core::{auto_auth::auth_token_storage::AuthTokenStorage, log, logs};
use std::io;

use base64::prelude::*;

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
    // TODO
    // read the token from installer.exe
    // parse token

    log::info!("Start auto auth script");
    if AuthTokenStorage::has_token() {
        log::info!("Token already installed");
        return Ok(());
    }

    let args: Vec<String> = std::env::args().collect();
    log::info!("Args: {args:?}");

    let installer_path = args
        .first()
        .ok_or_else(|| anyhow!("Installer path is not provided"))?;

    let token_binary = read_token(installer_path)?;

    // TODO actual encoding (json?)
    let token = BASE64_STANDARD.encode(token_binary);
    AuthTokenStorage::write_token(token.as_str())?;

    log::info!("Token write complete");
    Ok(())
}

fn read_token(path: &str) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let file_size = file.metadata()?.len();

    // TODO replace to actual strategy
    let read_size = 64.min(file_size as usize);

    file.seek(SeekFrom::End(-(read_size as i64)))?;

    let mut buffer = vec![0u8; read_size];
    file.read_exact(&mut buffer)?;
    Ok(buffer)
}
