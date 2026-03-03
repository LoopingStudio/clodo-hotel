use std::io::{Read, Seek, SeekFrom};
use tauri::Emitter;

use crate::constants::FILE_WATCHER_POLL_INTERVAL_MS;
use crate::timer_manager::{cancel_waiting_timer, cancel_permission_timer};
use crate::transcript_parser::process_transcript_line;
use crate::types::SharedState;

/// Read any new lines from the agent's JSONL file and process them.
pub async fn read_new_lines(
    agent_id: u32,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    // Extract file info without holding the lock during I/O
    let (file_path, current_offset, current_buffer, permission_sent) = {
        let s = state.lock().await;
        let agent = match s.agents.get(&agent_id) {
            Some(a) => a,
            None => return,
        };
        (
            agent.jsonl_file.clone(),
            agent.file_offset,
            agent.line_buffer.clone(),
            agent.permission_sent,
        )
    };

    // Sync file I/O (fast, no lock held)
    let file_size = match std::fs::metadata(&file_path) {
        Ok(m) => m.len(),
        Err(_) => return,
    };

    if file_size <= current_offset {
        return;
    }

    let new_bytes = {
        let mut f = match std::fs::File::open(&file_path) {
            Ok(f) => f,
            Err(_) => return,
        };
        if f.seek(SeekFrom::Start(current_offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return;
        }
        buf
    };

    let text = current_buffer + &String::from_utf8_lossy(&new_bytes);
    let mut parts: Vec<&str> = text.split('\n').collect();
    let new_buffer = parts.pop().unwrap_or("").to_string();

    let non_empty_lines: Vec<String> = parts
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();

    // Update file offset and line buffer
    {
        let mut s = state.lock().await;
        if let Some(agent) = s.agents.get_mut(&agent_id) {
            agent.file_offset = file_size;
            agent.line_buffer = new_buffer;
        }
    }

    if !non_empty_lines.is_empty() {
        cancel_waiting_timer(agent_id, state).await;
        cancel_permission_timer(agent_id, state).await;

        if permission_sent {
            {
                let mut s = state.lock().await;
                if let Some(agent) = s.agents.get_mut(&agent_id) {
                    agent.permission_sent = false;
                }
            }
            let _ = app_handle.emit(
                "pa-message",
                serde_json::json!({ "type": "agentToolPermissionClear", "id": agent_id }),
            );
        }
    }

    for line in &non_empty_lines {
        process_transcript_line(agent_id, line, state, app_handle).await;
    }
}

/// Spawn an interval-based polling task for the agent's JSONL file.
/// The handle is stored in `AppState.polling_tasks`.
pub async fn start_file_watching(
    agent_id: u32,
    state: SharedState,
    app_handle: tauri::AppHandle,
) {
    let state_c = state.clone();
    let ah = app_handle.clone();

    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            std::time::Duration::from_millis(FILE_WATCHER_POLL_INTERVAL_MS),
        );
        loop {
            interval.tick().await;
            let exists = {
                let s = state_c.lock().await;
                s.agents.contains_key(&agent_id)
            };
            if !exists {
                break;
            }
            read_new_lines(agent_id, &state_c, &ah).await;
        }
    });

    let mut s = state.lock().await;
    s.polling_tasks.insert(agent_id, handle);
}

/// Abort the polling task for an agent (call when removing an agent).
pub async fn stop_file_watching(agent_id: u32, state: &SharedState) {
    let mut s = state.lock().await;
    if let Some(handle) = s.polling_tasks.remove(&agent_id) {
        handle.abort();
    }
}
