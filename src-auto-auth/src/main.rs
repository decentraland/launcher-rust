use dcl_launcher_core::auto_auth::auth_token_storage::AuthTokenStorage;
use anyhow::Result;

fn main() -> Result<()>{
    // TODO
    // read path of installer from args
    // read the token from installer.exe
    // parse token
    // write token to the location
    // logging

    if AuthTokenStorage::has_token() {
        println!("Token already installed");
        return Ok(());
    }

    let token = "exampe token";
    AuthTokenStorage::write_token(token)?;


    println!("Complete");
    Ok(())
}
