use std::collections::HashSet;
use tauri::Emitter;

use crate::constants::{PERMISSION_TIMER_DELAY_MS, PERMISSION_EXEMPT_TOOLS, THINKING_TOOL_ID};
use crate::types::SharedState;

pub async fn cancel_waiting_timer(agent_id: u32, state: &SharedState) {
    let mut s = state.lock().await;
    if let Some(handle) = s.waiting_timers.remove(&agent_id) {
        handle.abort();
    }
}

pub async fn cancel_permission_timer(agent_id: u32, state: &SharedState) {
    let mut s = state.lock().await;
    if let Some(handle) = s.permission_timers.remove(&agent_id) {
        handle.abort();
    }
}

/// Start (or restart) the waiting timer.  After `delay_ms`, if not cancelled,
/// emits `agentStatus: waiting`.
pub async fn start_waiting_timer(
    agent_id: u32,
    delay_ms: u64,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    cancel_waiting_timer(agent_id, state).await;

    let state_c = state.clone();
    let ah = app_handle.clone();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        {
            let mut s = state_c.lock().await;
            s.waiting_timers.remove(&agent_id);
            if let Some(agent) = s.agents.get_mut(&agent_id) {
                agent.is_waiting = true;
                agent.active_tool_ids.remove(THINKING_TOOL_ID);
                agent.active_tool_statuses.remove(THINKING_TOOL_ID);
                agent.active_tool_names.remove(THINKING_TOOL_ID);
            }
        }
        let _ = ah.emit(
            "pa-message",
            serde_json::json!({ "type": "agentToolDone", "id": agent_id, "toolId": THINKING_TOOL_ID }),
        );
        let _ = ah.emit(
            "pa-message",
            serde_json::json!({ "type": "agentStatus", "id": agent_id, "status": "waiting" }),
        );
    });

    let mut s = state.lock().await;
    s.waiting_timers.insert(agent_id, handle);
}

/// Start (or restart) the permission timer.  After `PERMISSION_TIMER_DELAY_MS`,
/// if the agent still has non-exempt active tools, emit `agentToolPermission`.
pub async fn start_permission_timer(
    agent_id: u32,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    cancel_permission_timer(agent_id, state).await;

    let state_c = state.clone();
    let ah = app_handle.clone();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(PERMISSION_TIMER_DELAY_MS)).await;

        let exempt: HashSet<&str> = PERMISSION_EXEMPT_TOOLS.iter().copied().collect();

        // Determine if there are non-exempt tools
        let (has_non_exempt, stuck_parent_ids, agent_exists) = {
            let s = state_c.lock().await;
            let agent = match s.agents.get(&agent_id) {
                Some(a) => a,
                None => return,
            };

            let mut non_exempt = false;
            for tool_id in &agent.active_tool_ids {
                let name = agent.active_tool_names.get(tool_id).map(|s| s.as_str()).unwrap_or("");
                if !exempt.contains(name) {
                    non_exempt = true;
                    break;
                }
            }

            let mut stuck: Vec<String> = Vec::new();
            for (parent_tool_id, sub_names) in &agent.active_subagent_tool_names {
                for (_sub_id, name) in sub_names {
                    if !exempt.contains(name.as_str()) {
                        stuck.push(parent_tool_id.clone());
                        non_exempt = true;
                        break;
                    }
                }
            }
            (non_exempt, stuck, true)
        };

        if !agent_exists || !has_non_exempt {
            return;
        }

        {
            let mut s = state_c.lock().await;
            s.permission_timers.remove(&agent_id);
            if let Some(agent) = s.agents.get_mut(&agent_id) {
                agent.permission_sent = true;
            }
        }

        let _ = ah.emit(
            "pa-message",
            serde_json::json!({ "type": "agentToolPermission", "id": agent_id }),
        );
        for parent_tool_id in stuck_parent_ids {
            let _ = ah.emit(
                "pa-message",
                serde_json::json!({
                    "type": "subagentToolPermission",
                    "id": agent_id,
                    "parentToolId": parent_tool_id
                }),
            );
        }
    });

    let mut s = state.lock().await;
    s.permission_timers.insert(agent_id, handle);
}

/// Clear all active tool state for an agent and emit `agentToolsClear` + `agentStatus: active`.
pub async fn clear_agent_activity(
    agent_id: u32,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    cancel_permission_timer(agent_id, state).await;
    {
        let mut s = state.lock().await;
        if let Some(agent) = s.agents.get_mut(&agent_id) {
            agent.active_tool_ids.clear();
            agent.active_tool_statuses.clear();
            agent.active_tool_names.clear();
            agent.active_subagent_tool_ids.clear();
            agent.active_subagent_tool_names.clear();
            agent.is_waiting = false;
            agent.permission_sent = false;
        }
    }
    let _ = app_handle.emit(
        "pa-message",
        serde_json::json!({ "type": "agentToolsClear", "id": agent_id }),
    );
    let _ = app_handle.emit(
        "pa-message",
        serde_json::json!({ "type": "agentStatus", "id": agent_id, "status": "active" }),
    );
}
