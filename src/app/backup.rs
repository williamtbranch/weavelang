// src/app/backup.rs
//
// Auto-backup and crash-recovery support.
//
// Strategy:
//   • A `.lock` file is created when a project is opened and removed on clean
//     shutdown.  If the lock file survives (power loss, kill -9, etc.) the next
//     session knows the previous exit was unclean.
//   • A `.backup` file is written periodically (every 10 minutes while dirty)
//     and whenever an LLM generation job completes.  It is deleted on every
//     explicit "save project".
//   • On project load, if a backup exists that is newer than the project file
//     the user is offered a restore (GUI dialog or CLI prompt).

use crate::app::state::AppState;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ── Path helpers ──────────────────────────────────────────────────────────

/// `foo.wvl` → `foo.wvl.backup`
pub fn backup_path(project_path: &Path) -> PathBuf {
    let mut p = project_path.as_os_str().to_os_string();
    p.push(".backup");
    PathBuf::from(p)
}

/// `foo.wvl` → `foo.wvl.lock`
pub fn lock_path(project_path: &Path) -> PathBuf {
    let mut p = project_path.as_os_str().to_os_string();
    p.push(".lock");
    PathBuf::from(p)
}

// ── Lock file ─────────────────────────────────────────────────────────────

/// Create (or overwrite) a lock file for the given project.
pub fn write_lock(project_path: &Path) {
    let lock = lock_path(project_path);
    let content = format!(
        "pid={}\ntimestamp={:?}\n",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    if let Err(e) = fs::write(&lock, content) {
        eprintln!("[BACKUP] Failed to write lock file: {}", e);
    }
}

/// Remove the lock file (clean shutdown).
pub fn remove_lock(project_path: &Path) {
    let _ = fs::remove_file(lock_path(project_path));
}

// ── Backup file ───────────────────────────────────────────────────────────

/// Serialise `state` to `<project>.backup` (compact JSON, no pretty-print).
pub fn write_backup(state: &AppState, project_path: &Path) -> Result<(), String> {
    let bp = backup_path(project_path);
    let bytes =
        serde_json::to_vec(state).map_err(|e| format!("Backup serialisation failed: {}", e))?;
    fs::write(&bp, bytes).map_err(|e| format!("Backup write failed: {}", e))?;
    Ok(())
}

/// Delete the backup file (called after a successful manual save).
pub fn remove_backup(project_path: &Path) {
    let _ = fs::remove_file(backup_path(project_path));
}

// ── Recovery check ────────────────────────────────────────────────────────

/// Metadata returned when a recoverable backup is detected.
pub struct BackupInfo {
    pub backup_path: PathBuf,
    pub backup_modified: SystemTime,
    pub project_modified: Option<SystemTime>,
    /// `true` when a stale `.lock` file was found — strong evidence of a crash.
    pub stale_lock: bool,
}

/// Check whether a recovery backup exists for `project_path`.
///
/// Returns `Some(BackupInfo)` when a `.backup` file exists **and** is newer
/// than the project file itself.  If the backup is stale (older than the
/// project) it is silently removed.
pub fn check_recovery(project_path: &Path) -> Option<BackupInfo> {
    let bp = backup_path(project_path);
    let lp = lock_path(project_path);

    if !bp.exists() {
        // No backup → nothing to recover.  Clean up any stale lock.
        let _ = fs::remove_file(&lp);
        return None;
    }

    let stale_lock = lp.exists();

    let backup_modified = fs::metadata(&bp).ok()?.modified().ok()?;
    let project_modified = fs::metadata(project_path)
        .ok()
        .and_then(|m| m.modified().ok());

    let backup_is_newer = match project_modified {
        Some(proj_time) => backup_modified > proj_time,
        None => true, // project file missing — backup is all we have
    };

    if backup_is_newer {
        Some(BackupInfo {
            backup_path: bp,
            backup_modified,
            project_modified,
            stale_lock,
        })
    } else {
        // Backup is older than the saved project → discard it.
        let _ = fs::remove_file(&bp);
        let _ = fs::remove_file(&lp);
        None
    }
}

/// Load an `AppState` from a backup file, re-hydrating runtime services from
/// the currently-active state.
pub fn load_backup(
    backup_path: &Path,
    current_state: &AppState,
) -> Result<AppState, String> {
    let bytes = fs::read(backup_path).map_err(|e| format!("Cannot read backup: {}", e))?;
    let mut restored: AppState = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Cannot deserialise backup: {}", e))?;

    // Re-hydrate runtime-only (serde-skipped) services from current state
    restored.bridge = current_state.bridge.clone();
    restored.llm = current_state.llm.clone();
    restored.prompts = current_state.prompts.clone();
    restored.logger = current_state.logger.clone();
    restored.config = current_state.config.clone();

    // Re-hydrate output_dir from workspace config
    if let Some(cfg) = &restored.config {
        if let Some(ref out_dir) = cfg.output_dir {
            let resolved = if PathBuf::from(out_dir).is_absolute() {
                PathBuf::from(out_dir)
            } else {
                PathBuf::from(&cfg.content_project_dir).join(out_dir)
            };
            restored.output_dir = Some(resolved.to_string_lossy().to_string());
        }
    }

    Ok(restored)
}
