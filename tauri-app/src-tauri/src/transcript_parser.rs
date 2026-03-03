use std::path::Path;
use tauri::Emitter;

use crate::constants::{
    PERMISSION_EXEMPT_TOOLS, TOOL_DONE_DELAY_MS, TEXT_IDLE_DELAY_MS,
    BASH_COMMAND_DISPLAY_MAX_LENGTH, TASK_DESCRIPTION_DISPLAY_MAX_LENGTH,
};
use crate::timer_manager::{
    cancel_waiting_timer, start_waiting_timer, start_permission_timer,
    cancel_permission_timer, clear_agent_activity,
};
use crate::types::SharedState;

pub fn format_tool_status(tool_name: &str, input: &serde_json::Value) -> String {
    let base = |key: &str| -> String {
        input
            .get(key)
            .and_then(|v| v.as_str())
            .map(|p| Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or(p).to_string())
            .unwrap_or_default()
    };

    match tool_name {
        "Read" => format!("Reading {}", base("file_path")),
        "Edit" => format!("Editing {}", base("file_path")),
        "Write" => format!("Writing {}", base("file_path")),
        "Bash" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let display = if cmd.len() > BASH_COMMAND_DISPLAY_MAX_LENGTH {
                format!("{}…", &cmd[..BASH_COMMAND_DISPLAY_MAX_LENGTH])
            } else {
                cmd.to_string()
            };
            format!("Running: {display}")
        }
        "Glob" => "Searching files".to_string(),
        "Grep" => "Searching code".to_string(),
        "WebFetch" => "Fetching web content".to_string(),
        "WebSearch" => "Searching the web".to_string(),
        "Task" => {
            let desc = input
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if desc.is_empty() {
                "Running subtask".to_string()
            } else if desc.len() > TASK_DESCRIPTION_DISPLAY_MAX_LENGTH {
                format!("Subtask: {}…", &desc[..TASK_DESCRIPTION_DISPLAY_MAX_LENGTH])
            } else {
                format!("Subtask: {desc}")
            }
        }
        "AskUserQuestion" => "Waiting for your answer".to_string(),
        "EnterPlanMode" => "Planning".to_string(),
        "NotebookEdit" => "Editing notebook".to_string(),
        other => format!("Using {other}"),
    }
}

pub async fn process_transcript_line(
    agent_id: u32,
    line: &str,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    let record: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return,
    };

    let record_type = record.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match record_type {
        "assistant" => handle_assistant(agent_id, &record, state, app_handle).await,
        "user" => handle_user(agent_id, &record, state, app_handle).await,
        "system" => {
            if record.get("subtype").and_then(|v| v.as_str()) == Some("turn_duration") {
                handle_turn_duration(agent_id, state, app_handle).await;
            }
        }
        "progress" => handle_progress(agent_id, &record, state, app_handle).await,
        _ => {}
    }
}

async fn handle_assistant(
    agent_id: u32,
    record: &serde_json::Value,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    let content = match record
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    {
        Some(c) => c.clone(),
        None => return,
    };

    let has_tool_use = content.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"));
    let has_text = content.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"));

    if has_tool_use {
        cancel_waiting_timer(agent_id, state).await;

        let mut has_non_exempt = false;
        let mut tool_messages: Vec<serde_json::Value> = Vec::new();

        // Lock to update agent state
        {
            let mut s = state.lock().await;
            if let Some(agent) = s.agents.get_mut(&agent_id) {
                agent.is_waiting = false;
                agent.had_tools_in_turn = true;
            }
        }

        let _ = app_handle.emit(
            "pa-message",
            serde_json::json!({ "type": "agentStatus", "id": agent_id, "status": "active" }),
        );

        for block in &content {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                continue;
            }
            let id = match block.get("id").and_then(|v| v.as_str()) {
                Some(i) => i.to_string(),
                None => continue,
            };
            let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
            let status = format_tool_status(&name, &input);

            if !PERMISSION_EXEMPT_TOOLS.contains(&name.as_str()) {
                has_non_exempt = true;
            }

            {
                let mut s = state.lock().await;
                if let Some(agent) = s.agents.get_mut(&agent_id) {
                    agent.active_tool_ids.insert(id.clone());
                    agent.active_tool_statuses.insert(id.clone(), status.clone());
                    agent.active_tool_names.insert(id.clone(), name.clone());
                }
            }

            tool_messages.push(serde_json::json!({
                "type": "agentToolStart",
                "id": agent_id,
                "toolId": id,
                "status": status,
            }));
        }

        for msg in tool_messages {
            let _ = app_handle.emit("pa-message", msg);
        }

        if has_non_exempt {
            start_permission_timer(agent_id, state, app_handle).await;
        }
    } else if has_text {
        let had_tools = {
            let s = state.lock().await;
            s.agents.get(&agent_id).map(|a| a.had_tools_in_turn).unwrap_or(false)
        };
        if !had_tools {
            start_waiting_timer(agent_id, TEXT_IDLE_DELAY_MS, state, app_handle).await;
        }
    }
}

async fn handle_user(
    agent_id: u32,
    record: &serde_json::Value,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    let content = record.get("message").and_then(|m| m.get("content")).cloned();

    match &content {
        Some(serde_json::Value::Array(blocks)) => {
            let has_tool_result = blocks.iter().any(|b| {
                b.get("type").and_then(|t| t.as_str()) == Some("tool_result")
            });

            if has_tool_result {
                let mut completed_tools: Vec<String> = Vec::new();
                let mut task_completions: Vec<String> = Vec::new();

                for block in blocks {
                    if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                        continue;
                    }
                    let tool_use_id = match block.get("tool_use_id").and_then(|v| v.as_str()) {
                        Some(id) => id.to_string(),
                        None => continue,
                    };

                    // Check if this is a Task completion
                    let is_task = {
                        let s = state.lock().await;
                        s.agents.get(&agent_id)
                            .and_then(|a| a.active_tool_names.get(&tool_use_id))
                            .map(|n| n == "Task")
                            .unwrap_or(false)
                    };

                    if is_task {
                        task_completions.push(tool_use_id.clone());
                    }

                    {
                        let mut s = state.lock().await;
                        if let Some(agent) = s.agents.get_mut(&agent_id) {
                            if is_task {
                                agent.active_subagent_tool_ids.remove(&tool_use_id);
                                agent.active_subagent_tool_names.remove(&tool_use_id);
                            }
                            agent.active_tool_ids.remove(&tool_use_id);
                            agent.active_tool_statuses.remove(&tool_use_id);
                            agent.active_tool_names.remove(&tool_use_id);
                        }
                    }

                    completed_tools.push(tool_use_id);
                }

                // Emit subagentClear for Task completions
                for parent_id in &task_completions {
                    let _ = app_handle.emit(
                        "pa-message",
                        serde_json::json!({
                            "type": "subagentClear",
                            "id": agent_id,
                            "parentToolId": parent_id,
                        }),
                    );
                }

                // Emit agentToolDone after delay
                for tool_id in completed_tools {
                    let ah = app_handle.clone();
                    let aid = agent_id;
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(TOOL_DONE_DELAY_MS))
                            .await;
                        let _ = ah.emit(
                            "pa-message",
                            serde_json::json!({ "type": "agentToolDone", "id": aid, "toolId": tool_id }),
                        );
                    });
                }

                // Check if all tools done
                let all_done = {
                    let s = state.lock().await;
                    s.agents.get(&agent_id).map(|a| a.active_tool_ids.is_empty()).unwrap_or(true)
                };
                if all_done {
                    let mut s = state.lock().await;
                    if let Some(agent) = s.agents.get_mut(&agent_id) {
                        agent.had_tools_in_turn = false;
                    }
                }
            } else {
                // New user prompt (tool_result array that has no tool_result blocks)
                cancel_waiting_timer(agent_id, state).await;
                clear_agent_activity(agent_id, state, app_handle).await;
                let mut s = state.lock().await;
                if let Some(agent) = s.agents.get_mut(&agent_id) {
                    agent.had_tools_in_turn = false;
                }
            }
        }
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {
            // String content = new user text prompt
            cancel_waiting_timer(agent_id, state).await;
            clear_agent_activity(agent_id, state, app_handle).await;
            let mut st = state.lock().await;
            if let Some(agent) = st.agents.get_mut(&agent_id) {
                agent.had_tools_in_turn = false;
            }
        }
        _ => {}
    }
}

async fn handle_turn_duration(
    agent_id: u32,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    cancel_waiting_timer(agent_id, state).await;
    cancel_permission_timer(agent_id, state).await;

    let had_tools = {
        let s = state.lock().await;
        s.agents.get(&agent_id).map(|a| !a.active_tool_ids.is_empty()).unwrap_or(false)
    };

    if had_tools {
        {
            let mut s = state.lock().await;
            if let Some(agent) = s.agents.get_mut(&agent_id) {
                agent.active_tool_ids.clear();
                agent.active_tool_statuses.clear();
                agent.active_tool_names.clear();
                agent.active_subagent_tool_ids.clear();
                agent.active_subagent_tool_names.clear();
            }
        }
        let _ = app_handle.emit(
            "pa-message",
            serde_json::json!({ "type": "agentToolsClear", "id": agent_id }),
        );
    }

    {
        let mut s = state.lock().await;
        if let Some(agent) = s.agents.get_mut(&agent_id) {
            agent.is_waiting = true;
            agent.permission_sent = false;
            agent.had_tools_in_turn = false;
        }
    }

    let _ = app_handle.emit(
        "pa-message",
        serde_json::json!({ "type": "agentStatus", "id": agent_id, "status": "waiting" }),
    );
}

async fn handle_progress(
    agent_id: u32,
    record: &serde_json::Value,
    state: &SharedState,
    app_handle: &tauri::AppHandle,
) {
    let parent_tool_id = match record
        .get("parentToolUseID")
        .and_then(|v| v.as_str())
    {
        Some(id) => id.to_string(),
        None => return,
    };

    let data = match record.get("data") {
        Some(d) => d.clone(),
        None => return,
    };

    let data_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if data_type == "bash_progress" || data_type == "mcp_progress" {
        let is_active = {
            let s = state.lock().await;
            s.agents
                .get(&agent_id)
                .map(|a| a.active_tool_ids.contains(&parent_tool_id))
                .unwrap_or(false)
        };
        if is_active {
            start_permission_timer(agent_id, state, app_handle).await;
        }
        return;
    }

    // agent_progress: check parent is a Task
    let parent_is_task = {
        let s = state.lock().await;
        s.agents
            .get(&agent_id)
            .and_then(|a| a.active_tool_names.get(&parent_tool_id))
            .map(|n| n == "Task")
            .unwrap_or(false)
    };
    if !parent_is_task {
        return;
    }

    let msg = match data.get("message") {
        Some(m) => m.clone(),
        None => return,
    };

    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let inner_content = msg
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    if msg_type == "assistant" {
        let mut has_non_exempt = false;
        let mut tool_messages: Vec<serde_json::Value> = Vec::new();

        for block in &inner_content {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                continue;
            }
            let id = match block.get("id").and_then(|v| v.as_str()) {
                Some(i) => i.to_string(),
                None => continue,
            };
            let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
            let status = format_tool_status(&name, &input);

            if !PERMISSION_EXEMPT_TOOLS.contains(&name.as_str()) {
                has_non_exempt = true;
            }

            {
                let mut s = state.lock().await;
                if let Some(agent) = s.agents.get_mut(&agent_id) {
                    let sub_ids = agent
                        .active_subagent_tool_ids
                        .entry(parent_tool_id.clone())
                        .or_default();
                    sub_ids.insert(id.clone());

                    let sub_names = agent
                        .active_subagent_tool_names
                        .entry(parent_tool_id.clone())
                        .or_default();
                    sub_names.insert(id.clone(), name.clone());
                }
            }

            tool_messages.push(serde_json::json!({
                "type": "subagentToolStart",
                "id": agent_id,
                "parentToolId": parent_tool_id,
                "toolId": id,
                "status": status,
            }));
        }

        for msg_val in tool_messages {
            let _ = app_handle.emit("pa-message", msg_val);
        }

        if has_non_exempt {
            start_permission_timer(agent_id, state, app_handle).await;
        }
    } else if msg_type == "user" {
        let mut completed: Vec<String> = Vec::new();

        for block in &inner_content {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            let sub_id = match block.get("tool_use_id").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            {
                let mut s = state.lock().await;
                if let Some(agent) = s.agents.get_mut(&agent_id) {
                    if let Some(ids) = agent.active_subagent_tool_ids.get_mut(&parent_tool_id) {
                        ids.remove(&sub_id);
                    }
                    if let Some(names) = agent.active_subagent_tool_names.get_mut(&parent_tool_id) {
                        names.remove(&sub_id);
                    }
                }
            }
            completed.push(sub_id);
        }

        for sub_id in completed {
            let ah = app_handle.clone();
            let aid = agent_id;
            let ptid = parent_tool_id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(TOOL_DONE_DELAY_MS)).await;
                let _ = ah.emit(
                    "pa-message",
                    serde_json::json!({
                        "type": "subagentToolDone",
                        "id": aid,
                        "parentToolId": ptid,
                        "toolId": sub_id,
                    }),
                );
            });
        }

        // Check if still non-exempt tools remain
        let still_non_exempt = {
            let s = state.lock().await;
            if let Some(agent) = s.agents.get(&agent_id) {
                agent.active_subagent_tool_names.values().any(|names| {
                    names.values().any(|name| !PERMISSION_EXEMPT_TOOLS.contains(&name.as_str()))
                })
            } else {
                false
            }
        };
        if still_non_exempt {
            start_permission_timer(agent_id, state, app_handle).await;
        }
    }
}
