// Shared helpers for the rust-script files in this directory. Not a runnable
// script — each script pulls it in with `include!("shared.rs")` and must
// define `const SCRIPT_NAME: &str`, which prefixes every `fail` message.
//
// std paths are fully qualified (no `use` statements) so the include never
// collides with a script's own imports, and every item is
// `#[allow(dead_code)]` because no single script uses all of them.
//
// CAVEAT: rust-script caches compiled scripts by the content of the main
// script file only, so edits here are invisible to already-built scripts.
// After changing this file, rerun dependents with `rust-script --force`.
// CI is unaffected — runners compile every script from scratch.

/// Walks up from the current directory to the repo root, so scripts behave
/// the same whether invoked from the root, from `scripts/`, or by Tauri/npm.
#[allow(dead_code)]
fn repo_root() -> std::path::PathBuf {
    let start =
        std::env::current_dir().unwrap_or_else(|e| fail(&format!("cannot read cwd: {e}")));
    for dir in start.ancestors() {
        if dir.join("package.json").is_file() && dir.join("src-tauri").is_dir() {
            return dir.to_path_buf();
        }
    }
    fail(&format!("repo root not found at or above {}", start.display()));
}

/// Runs `program` with `args` in `cwd`; exits with the child's code on failure.
#[allow(dead_code)]
fn run<S: AsRef<std::ffi::OsStr>>(cwd: &std::path::Path, program: &str, args: &[S]) {
    let status = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap_or_else(|e| fail(&format!("cannot run {program}: {e}")));
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Runs a sibling rust-script from the repo root. Set `RUST_SCRIPT` to point
/// at a specific rust-script binary; otherwise it is resolved from `PATH`.
#[allow(dead_code)]
fn rust_script(root: &std::path::Path, script: &str) {
    println!("Running {script}...");
    let runner = std::env::var("RUST_SCRIPT").unwrap_or_else(|_| "rust-script".to_string());
    run(root, &runner, &[script]);
}

#[allow(dead_code)]
fn copy(from: &std::path::Path, to: &std::path::Path) {
    std::fs::copy(from, to).unwrap_or_else(|e| {
        fail(&format!("cannot copy {} to {}: {e}", from.display(), to.display()))
    });
}

#[allow(dead_code)]
fn fail(message: &str) -> ! {
    eprintln!("{SCRIPT_NAME}: {message}");
    std::process::exit(1);
}
