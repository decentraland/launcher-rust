echo (pwd)
cargo build --manifest-path src-auto-auth/Cargo.toml --release;
cp src-auto-auth/target/release/src-auto-auth.exe src-tauri/resources/auto-auth-token-fetch.exe;
