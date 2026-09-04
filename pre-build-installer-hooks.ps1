echo (pwd)
cargo build --manifest-path installer-hooks/Cargo.toml --release;
cp installer-hooks/target/release/installer-hooks.exe src-tauri/resources/installer-hooks.exe;
