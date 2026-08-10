// Implementations live in dcl-launcher-shared (the base crate for common
// shareables); these re-exports keep core-internal paths stable.
#[cfg(target_os = "macos")]
pub use dcl_launcher_shared::is_running_from_dmg;
#[cfg(target_os = "macos")]
pub use dcl_launcher_shared::macos::{dmg_backing_file, dmg_mount_path, where_from_attr};
