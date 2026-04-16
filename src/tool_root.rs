// src/tool_root.rs
//
// Centralised resolution of the WeaveLang "tool root" directory — the directory
// that contains `assets/`, `config.toml`, and other runtime resources.
//
// Resolution order (first match wins):
//   1. Explicit `--tool-root` CLI flag (when provided)
//   2. `WEAVELANG_ROOT` environment variable
//   3. Parent directory of the running executable
//   4. Current working directory (legacy fallback)
//
// Once resolved, every part of the codebase should obtain the root via the
// value produced here rather than calling `std::env::current_dir()` ad-hoc.

use std::path::PathBuf;

/// Resolve the tool root directory using the priority chain described above.
///
/// `cli_override` is the value of the `--tool-root` CLI argument, if any.
pub fn resolve_tool_root(cli_override: Option<&PathBuf>) -> Result<PathBuf, String> {
    // 1. Explicit CLI flag
    if let Some(root) = cli_override {
        return validate(root.clone(), "CLI --tool-root");
    }

    // 2. Environment variable
    if let Ok(val) = std::env::var("WEAVELANG_ROOT") {
        if !val.is_empty() {
            return validate(PathBuf::from(val), "WEAVELANG_ROOT env var");
        }
    }

    // 3. Executable's parent directory
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.to_path_buf();
            if candidate.join("assets").is_dir() {
                println!("[INFO] Tool root resolved from executable location: {}", candidate.display());
                return Ok(candidate);
            }
        }
    }

    // 4. Current working directory (legacy / dev fallback)
    match std::env::current_dir() {
        Ok(cwd) => {
            if cwd.join("assets").is_dir() {
                println!("[INFO] Tool root resolved from current directory: {}", cwd.display());
                Ok(cwd)
            } else {
                Err(format!(
                    "Could not locate WeaveLang assets. No 'assets/' directory found in:\n\
                     • executable dir\n\
                     • current dir ({})\n\
                     Set WEAVELANG_ROOT or pass --tool-root to specify the correct location.",
                    cwd.display()
                ))
            }
        }
        Err(e) => Err(format!("Cannot determine current directory: {e}")),
    }
}

fn validate(path: PathBuf, source: &str) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!(
            "Tool root from {source} ('{}') is not a valid directory.",
            path.display()
        ));
    }
    if !path.join("assets").is_dir() {
        eprintln!(
            "[WARN] Tool root from {source} ('{}') has no 'assets/' subdirectory.",
            path.display()
        );
    }
    println!("[INFO] Tool root resolved from {source}: {}", path.display());
    Ok(path)
}
