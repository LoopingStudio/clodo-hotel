use std::path::{Path, PathBuf};
use std::collections::HashMap;
use tauri::Emitter;

use crate::constants::{PIXEL_AGENTS_DIR, STANDALONE_STATE_FILE, JSONL_POLL_INTERVAL_MS};
use crate::file_watcher::{start_file_watching, stop_file_watching};
use crate::timer_manager::{cancel_waiting_timer, cancel_permission_timer};
use crate::types::{AgentState, PersistedAgent, PersistedState, SharedState};

fn state_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(PIXEL_AGENTS_DIR)
        .join(STANDALONE_STATE_FILE)
}

fn ensure_state_dir() {
    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(PIXEL_AGENTS_DIR);
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
}

pub fn load_persisted_state() -> PersistedState {
    let path = state_file_path();
    if !path.exists() {
        return PersistedState::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_persisted_state(state_dto: &PersistedState) {
    ensure_state_dir();
    let path = state_file_path();
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string_pretty(state_dto) {
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub async fn persist_agents(state: &SharedState) {
    let (agents_vec, seats, sound) = {
        let s = state.lock().await;
        let agents_vec: Vec<PersistedAgent> = s
            .agents
            .values()
            .map(|a| PersistedAgent {
                id: a.id,
                session_id: a.session_id.clone(),
                jsonl_file: a.jsonl_file.clone(),
                project_dir: a.project_dir.clone(),
                folder_name: a.folder_name.clone(),
            })
            .collect();
        let seats = s.agent_seats.clone();
        let sound = s.sound_enabled;
        (agents_vec, seats, sound)
    };
    save_persisted_state(&PersistedState {
        agents: agents_vec,
        agent_seats: seats,
        sound_enabled: sound,
    });
}

/// Add a session as an agent. Returns the agent id (or existing id if duplicate).
pub async fn add_session_as_agent(
    session_id: String,
    project_dir: String,
    jsonl_file: String,
    folder_name: Option<String>,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) -> u32 {
    // Check for duplicate
    {
        let s = state.lock().await;
        if let Some(existing) = s.agents.values().find(|a| a.jsonl_file == jsonl_file) {
            return existing.id;
        }
    }

    let file_exists = Path::new(&jsonl_file).exists();
    let initial_offset = if file_exists {
        std::fs::metadata(&jsonl_file).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    let id = {
        let mut s = state.lock().await;
        let id = s.next_agent_id;
        s.next_agent_id += 1;
        let mut agent = AgentState::new(
            id,
            session_id,
            project_dir,
            jsonl_file.clone(),
            initial_offset,
            folder_name.clone(),
        );
        // Existing sessions start idle — if Claude is still running, the watcher
        // will receive a tool_use and set is_waiting = false + emit agentToolStart.
        if file_exists {
            agent.is_waiting = true;
        }
        s.agents.insert(id, agent);
        s.known_jsonl_files.insert(jsonl_file.clone());
        id
    };

    persist_agents(state).await;

    let _ = app_handle.emit(
        "pa-message",
        serde_json::json!({ "type": "agentCreated", "id": id, "folderName": folder_name }),
    );

    if file_exists {
        start_file_watching(id, state.clone(), app_handle.clone()).await;
    } else {
        start_jsonl_poll(id, jsonl_file, state.clone(), app_handle.clone()).await;
    }

    id
}

/// Poll every second until the JSONL file appears, then start file watching.
async fn start_jsonl_poll(
    agent_id: u32,
    jsonl_file: String,
    state: SharedState,
    app_handle: tauri::AppHandle,
) {
    let state_c = state.clone();
    let ah = app_handle.clone();

    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(
            std::time::Duration::from_millis(JSONL_POLL_INTERVAL_MS),
        );
        loop {
            interval.tick().await;
            if !{
                let s = state_c.lock().await;
                s.agents.contains_key(&agent_id)
            } {
                break;
            }
            if Path::new(&jsonl_file).exists() {
                // Update offset to end of file and start watching
                let offset = std::fs::metadata(&jsonl_file).map(|m| m.len()).unwrap_or(0);
                {
                    let mut s = state_c.lock().await;
                    if let Some(agent) = s.agents.get_mut(&agent_id) {
                        agent.file_offset = offset;
                    }
                    s.jsonl_poll_tasks.remove(&agent_id);
                }
                start_file_watching(agent_id, state_c.clone(), ah.clone()).await;
                break;
            }
        }
    });

    let mut s = state.lock().await;
    s.jsonl_poll_tasks.insert(agent_id, handle);
}

/// Remove an agent and stop all its associated tasks/timers.
pub async fn remove_agent(
    agent_id: u32,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    let jsonl_file = {
        let s = state.lock().await;
        s.agents.get(&agent_id).map(|a| a.jsonl_file.clone())
    };

    // Abort poll tasks
    {
        let mut s = state.lock().await;
        if let Some(h) = s.jsonl_poll_tasks.remove(&agent_id) {
            h.abort();
        }
    }

    stop_file_watching(agent_id, state).await;
    cancel_waiting_timer(agent_id, state).await;
    cancel_permission_timer(agent_id, state).await;

    {
        let mut s = state.lock().await;
        if let Some(file) = &jsonl_file {
            s.known_jsonl_files.remove(file);
        }
        s.agents.remove(&agent_id);
    }

    persist_agents(state).await;

    let _ = app_handle.emit(
        "pa-message",
        serde_json::json!({ "type": "agentClosed", "id": agent_id }),
    );
}

/// Restore persisted agents on cold start (only if agents map is empty).
pub async fn restore_agents(
    persisted: &[PersistedAgent],
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    let is_empty = {
        let s = state.lock().await;
        s.agents.is_empty()
    };
    if !is_empty {
        return;
    }

    let mut max_id: u32 = 0;

    for p in persisted {
        if !Path::new(&p.jsonl_file).exists() {
            continue;
        }

        let offset = std::fs::metadata(&p.jsonl_file).map(|m| m.len()).unwrap_or(0);

        {
            let mut s = state.lock().await;
            let mut agent = AgentState::new(
                p.id,
                p.session_id.clone(),
                p.project_dir.clone(),
                p.jsonl_file.clone(),
                offset,
                p.folder_name.clone(),
            );
            // Default to idle: the session has likely ended since last launch.
            // If Claude is still active, the file watcher will receive a tool_use
            // and set is_waiting = false + emit agentToolStart to walk them back.
            agent.is_waiting = true;
            s.agents.insert(p.id, agent);
            s.known_jsonl_files.insert(p.jsonl_file.clone());
            if p.id > max_id {
                max_id = p.id;
            }
        }

        start_file_watching(p.id, state.clone(), app_handle.clone()).await;
    }

    {
        let mut s = state.lock().await;
        if max_id >= s.next_agent_id {
            s.next_agent_id = max_id + 1;
        }
    }
}

/// Emit `existingAgents` with all current agent ids and seat metadata.
/// Also re-sends any active tool statuses and waiting states.
pub async fn send_existing_agents(state: &SharedState, app_handle: &tauri::AppHandle) {
    let (agent_ids, agent_meta, folder_names, active_statuses, waiting_ids) = {
        let s = state.lock().await;
        let mut ids: Vec<u32> = s.agents.keys().cloned().collect();
        ids.sort();

        let mut folder_names: HashMap<u32, String> = HashMap::new();
        let mut active: Vec<(u32, String, String)> = Vec::new(); // (id, toolId, status)
        let mut waiting: Vec<u32> = Vec::new();

        for (id, agent) in &s.agents {
            if let Some(ref name) = agent.folder_name {
                folder_names.insert(*id, name.clone());
            }
            for (tool_id, status) in &agent.active_tool_statuses {
                active.push((*id, tool_id.clone(), status.clone()));
            }
            if agent.is_waiting {
                waiting.push(*id);
            }
        }

        (ids, s.agent_seats.clone(), folder_names, active, waiting)
    };

    // Convert agent_meta to JSON-friendly form (keys are u32, convert to string for JSON)
    let meta_json: serde_json::Value = {
        let mut m = serde_json::Map::new();
        for (k, v) in &agent_meta {
            m.insert(k.to_string(), serde_json::to_value(v).unwrap_or_default());
        }
        serde_json::Value::Object(m)
    };

    let folder_json: serde_json::Value = {
        let mut m = serde_json::Map::new();
        for (k, v) in &folder_names {
            m.insert(k.to_string(), serde_json::Value::String(v.clone()));
        }
        serde_json::Value::Object(m)
    };

    let _ = app_handle.emit(
        "pa-message",
        serde_json::json!({
            "type": "existingAgents",
            "agents": agent_ids,
            "agentMeta": meta_json,
            "folderNames": folder_json,
            "waitingIds": waiting_ids,
        }),
    );

    // Re-send active tool statuses
    for (id, tool_id, status) in active_statuses {
        let _ = app_handle.emit(
            "pa-message",
            serde_json::json!({
                "type": "agentToolStart",
                "id": id,
                "toolId": tool_id,
                "status": status,
            }),
        );
    }
}
