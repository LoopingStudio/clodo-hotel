use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::Emitter;

use crate::constants::{PIXEL_AGENTS_DIR, LAYOUT_FILE_NAME, LAYOUT_FILE_POLL_INTERVAL_MS};
use crate::types::SharedState;

pub fn get_layout_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(PIXEL_AGENTS_DIR)
        .join(LAYOUT_FILE_NAME)
}

pub fn read_layout_from_file() -> Option<serde_json::Value> {
    let path = get_layout_file_path();
    if !path.exists() {
        return None;
    }
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn write_layout_to_file(layout: &serde_json::Value) -> Result<(), String> {
    let path = get_layout_file_path();
    let dir = path.parent().unwrap_or(Path::new("/"));
    if !dir.exists() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create .pixel-agents dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(layout)
        .map_err(|e| format!("JSON serialization failed: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)
        .map_err(|e| format!("Failed to write tmp layout: {e}"))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| format!("Failed to rename layout: {e}"))?;
    Ok(())
}

fn file_mtime_ms(path: &Path) -> u128 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Atomically mark that we wrote the layout file ourselves so the watcher
/// skips the next external-change event.
pub async fn mark_own_write(state: &SharedState) {
    let path = get_layout_file_path();
    let mtime = file_mtime_ms(&path);
    let mut s = state.lock().await;
    s.layout_own_write = true;
    s.last_layout_mtime = mtime;
}

/// Spawn a tokio task that polls the layout file for external changes.
/// When a change is detected, it emits `pa-message { type: "layoutLoaded", layout }`.
pub async fn start_layout_watcher(state: SharedState, app_handle: tauri::AppHandle) {
    // Initialise mtime
    {
        let path = get_layout_file_path();
        let mut s = state.lock().await;
        if s.last_layout_mtime == 0 {
            s.last_layout_mtime = file_mtime_ms(&path);
        }
        // Cancel any existing watcher
        if let Some(old) = s.layout_watcher_task.take() {
            old.abort();
        }
    }

    // Clone Arc before moving into spawned task
    let state_for_task = state.clone();
    let handle = tokio::spawn(async move {
        let state = state_for_task;
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(LAYOUT_FILE_POLL_INTERVAL_MS));
        loop {
            interval.tick().await;
            let path = get_layout_file_path();
            let current_mtime = file_mtime_ms(&path);

            let (last_mtime, own_write) = {
                let s = state.lock().await;
                (s.last_layout_mtime, s.layout_own_write)
            };

            if current_mtime <= last_mtime {
                continue;
            }

            // mtime changed
            {
                let mut s = state.lock().await;
                s.last_layout_mtime = current_mtime;
                if s.layout_own_write {
                    s.layout_own_write = false;
                    continue;
                }
            }

            if own_write {
                continue;
            }

            // External change — read and emit
            if let Some(layout) = read_layout_from_file() {
                let _ = app_handle.emit(
                    "pa-message",
                    serde_json::json!({ "type": "layoutLoaded", "layout": layout }),
                );
            }
        }
    });

    let mut s = state.lock().await;
    s.layout_watcher_task = Some(handle);
}
