// Tauri requires this on Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod constants;
mod types;
mod session_scanner;
mod timer_manager;
mod layout_persistence;
mod asset_loader;
mod transcript_parser;
mod file_watcher;
mod agent_server;

use std::sync::Arc;
use tokio::sync::Mutex;

use tauri::Emitter;

use agent_server::{
    add_session_as_agent, remove_agent, restore_agents, send_existing_agents, persist_agents,
    load_persisted_state,
};
use asset_loader::get_assets_root;
use constants::SESSION_AUTO_ADD_WINDOW_MS;
use layout_persistence::{
    read_layout_from_file, write_layout_to_file, start_layout_watcher, mark_own_write,
};
use session_scanner::{scan_sessions, find_recent_sessions, claude_projects_dir};
use types::{AppState, SharedState};

fn is_valid_layout(layout: &serde_json::Value) -> bool {
    layout.get("version").and_then(|v| v.as_i64()).map(|v| v == 1).unwrap_or(false)
        && layout.get("tiles").map(|t| t.is_array()).unwrap_or(false)
}

async fn emit_available_sessions(state: &SharedState, app_handle: &tauri::AppHandle) {
    let known = {
        let s = state.lock().await;
        s.known_jsonl_files.clone()
    };
    let projects = scan_sessions(&known);
    let _ = app_handle.emit(
        "pa-message",
        serde_json::json!({ "type": "availableSessions", "projects": projects }),
    );
}

/// Single entry point from the webview for all messages.
/// Re-dispatches based on `message.type`.
#[tauri::command]
async fn handle_message(
    message: serde_json::Value,
    state: tauri::State<'_, SharedState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let state_arc = state.inner().clone();
    let msg_type = message.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "webviewReady" => handle_webview_ready(state_arc, app_handle).await,

        "scanSessions" | "openClaude" => {
            let known = {
                let s = state_arc.lock().await;
                s.known_jsonl_files.clone()
            };
            let projects = scan_sessions(&known);
            let _ = app_handle.emit(
                "pa-message",
                serde_json::json!({ "type": "availableSessions", "projects": projects }),
            );
        }

        "addSession" => {
            let session_id = message
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let jsonl_file = message
                .get("jsonlFile")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let folder_name = message
                .get("folderName")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if !session_id.is_empty() && !jsonl_file.is_empty() {
                let project_dir = std::path::Path::new(&jsonl_file)
                    .parent()
                    .and_then(|p| p.parent()) // ~/.claude/projects/<hash>/ → ~/.claude/projects/
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                add_session_as_agent(
                    session_id,
                    project_dir,
                    jsonl_file,
                    folder_name,
                    &state_arc,
                    &app_handle,
                )
                .await;

                emit_available_sessions(&state_arc, &app_handle).await;
            }
        }

        "closeAgent" | "removeSession" => {
            if let Some(id) = message.get("id").and_then(|v| v.as_u64()) {
                remove_agent(id as u32, &state_arc, &app_handle).await;

                emit_available_sessions(&state_arc, &app_handle).await;
            }
        }

        "saveLayout" => {
            if let Some(layout) = message.get("layout") {
                if let Err(e) = write_layout_to_file(layout) {
                    eprintln!("[Pixel Agents] saveLayout error: {e}");
                } else {
                    mark_own_write(&state_arc).await;
                }
            }
        }

        "saveAgentSeats" => {
            if let Some(seats_val) = message.get("seats") {
                if let Ok(seats_map) = serde_json::from_value::<
                    std::collections::HashMap<String, types::SeatMeta>,
                >(seats_val.clone())
                {
                    let mut s = state_arc.lock().await;
                    s.agent_seats.clear();
                    for (k, v) in seats_map {
                        if let Ok(id) = k.parse::<u32>() {
                            s.agent_seats.insert(id, v);
                        }
                    }
                    drop(s);
                    persist_agents(&state_arc).await;
                }
            }
        }

        "setSoundEnabled" => {
            if let Some(enabled) = message.get("enabled").and_then(|v| v.as_bool()) {
                let mut s = state_arc.lock().await;
                s.sound_enabled = enabled;
                drop(s);
                persist_agents(&state_arc).await;
            }
        }

        "importLayoutData" => {
            if let Some(layout) = message.get("layout") {
                if is_valid_layout(layout) {
                    if let Err(e) = write_layout_to_file(layout) {
                        eprintln!("[Pixel Agents] importLayoutData error: {e}");
                    } else {
                        mark_own_write(&state_arc).await;
                        let _ = app_handle.emit(
                            "pa-message",
                            serde_json::json!({ "type": "layoutLoaded", "layout": layout }),
                        );
                    }
                }
            }
        }

        "exportLayout" => {
            if let Some(layout_val) = read_layout_from_file() {
                let ah = app_handle.clone();
                tokio::task::spawn_blocking(move || {
                    use tauri_plugin_dialog::DialogExt;
                    if let Some(file_path) = ah
                        .dialog()
                        .file()
                        .set_file_name("pixel-agents-layout.json")
                        .add_filter("JSON", &["json"])
                        .blocking_save_file()
                    {
                        if let Ok(path) = file_path.into_path() {
                            if let Ok(json) = serde_json::to_string_pretty(&layout_val) {
                                let _ = std::fs::write(path, json);
                            }
                        }
                    }
                })
                .await
                .ok();
            }
        }

        "importLayout" => {
            let ah = app_handle.clone();
            let picked = tokio::task::spawn_blocking(move || {
                use tauri_plugin_dialog::DialogExt;
                ah.dialog()
                    .file()
                    .add_filter("JSON", &["json"])
                    .blocking_pick_file()
                    .and_then(|fp| fp.into_path().ok())
                    .and_then(|path| {
                        let raw = std::fs::read_to_string(&path).ok()?;
                        let layout: serde_json::Value = serde_json::from_str(&raw).ok()?;
                        if is_valid_layout(&layout) { Some(layout) } else { None }
                    })
            })
            .await
            .ok()
            .flatten();

            if let Some(layout) = picked {
                if write_layout_to_file(&layout).is_ok() {
                    mark_own_write(&state_arc).await;
                    let _ = app_handle.emit(
                        "pa-message",
                        serde_json::json!({ "type": "layoutLoaded", "layout": layout }),
                    );
                }
            }
        }

        "openSessionsFolder" => {
            let dir = claude_projects_dir();
            if dir.exists() {
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&dir).spawn();
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
            }
        }

        "relaunchApp" => {
            use tauri_plugin_process::ProcessExt;
            app_handle.restart();
        }

        _ => {
            // Unknown message type — silently ignore
        }
    }

    Ok(())
}

/// Sequence executed on `webviewReady`:
/// 1. Restore agents (cold start only)
/// 2. emit settingsLoaded
/// 3–6. Load + emit assets (characters, floor, wall, furniture)
/// 7. send_existing_agents → existingAgents  (BEFORE layoutLoaded)
/// 8. read_layout → layoutLoaded
/// 9. Start layout watcher
/// 10. Auto-add recent sessions (if no agents)
/// 11. scan_sessions → availableSessions
async fn handle_webview_ready(state: SharedState, app_handle: tauri::AppHandle) {
    // 1. Restore agents
    let persisted = load_persisted_state();
    restore_agents(&persisted.agents, &state, &app_handle).await;

    // Restore seats
    {
        let mut s = state.lock().await;
        for (k, v) in persisted.agent_seats {
            s.agent_seats.insert(k, v);
        }
        s.sound_enabled = persisted.sound_enabled;
    }

    // 2. settingsLoaded
    let sound_enabled = {
        let s = state.lock().await;
        s.sound_enabled
    };
    let _ = app_handle.emit(
        "pa-message",
        serde_json::json!({ "type": "settingsLoaded", "soundEnabled": sound_enabled }),
    );

    let assets_root = get_assets_root(&app_handle);

    // 3. characterSpritesLoaded
    if let Some(char_data) = asset_loader::load_character_sprites(&assets_root) {
        let _ = app_handle.emit(
            "pa-message",
            serde_json::json!({
                "type": "characterSpritesLoaded",
                "characters": char_data["characters"],
            }),
        );
    }

    // 4. floorTilesLoaded
    if let Some(floor_data) = asset_loader::load_floor_tiles(&assets_root) {
        let _ = app_handle.emit(
            "pa-message",
            serde_json::json!({
                "type": "floorTilesLoaded",
                "sprites": floor_data["sprites"],
            }),
        );
    }

    // 5. wallTilesLoaded
    if let Some(wall_data) = asset_loader::load_wall_tiles(&assets_root) {
        let _ = app_handle.emit(
            "pa-message",
            serde_json::json!({
                "type": "wallTilesLoaded",
                "sprites": wall_data["sprites"],
            }),
        );
    }

    // 6. furnitureAssetsLoaded
    if let Some(furniture_data) = asset_loader::load_furniture_assets(&assets_root) {
        let _ = app_handle.emit(
            "pa-message",
            serde_json::json!({
                "type": "furnitureAssetsLoaded",
                "catalog": furniture_data["catalog"],
                "sprites": furniture_data["sprites"],
            }),
        );
    }

    // 7. existingAgents (BEFORE layoutLoaded)
    send_existing_agents(&state, &app_handle).await;

    // 8. layoutLoaded
    let layout = read_layout_from_file()
        .or_else(|| asset_loader::load_default_layout(&assets_root));

    if let Some(ref layout_val) = layout {
        let _ = app_handle.emit(
            "pa-message",
            serde_json::json!({ "type": "layoutLoaded", "layout": layout_val }),
        );
    }

    // 9. Start layout watcher
    {
        let has_watcher = {
            let s = state.lock().await;
            s.layout_watcher_task.is_some()
        };
        if !has_watcher {
            start_layout_watcher(state.clone(), app_handle.clone()).await;
        }
    }

    // 10. Auto-add recent sessions if no agents
    let no_agents = {
        let s = state.lock().await;
        s.agents.is_empty()
    };
    if no_agents {
        let known = {
            let s = state.lock().await;
            s.known_jsonl_files.clone()
        };
        let recent = find_recent_sessions(SESSION_AUTO_ADD_WINDOW_MS, &known);
        for session in recent {
            let project_dir = std::path::Path::new(&session.jsonl_file)
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let folder = std::path::Path::new(&session.project_path)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
            add_session_as_agent(
                session.session_id,
                project_dir,
                session.jsonl_file,
                folder,
                &state,
                &app_handle,
            )
            .await;
        }
    }

    // 11. availableSessions
    emit_available_sessions(&state, &app_handle).await;
}

async fn check_for_updates(app: tauri::AppHandle) {
    use tauri_plugin_updater::UpdaterExt;
    let update = match app.updater() {
        Ok(u) => match u.check().await {
            Ok(Some(u)) => u,
            _ => return,
        },
        Err(_) => return,
    };
    let version = update.version.clone();
    let _ = app.emit(
        "pa-message",
        serde_json::json!({ "type": "updateAvailable", "version": version }),
    );
    if update.download_and_install(|_, _| {}, || {}).await.is_ok() {
        let _ = app.emit("pa-message", serde_json::json!({ "type": "updateReady" }));
    }
}

fn main() {
    let shared_state: SharedState = Arc::new(Mutex::new(AppState::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                check_for_updates(handle).await;
            });
            Ok(())
        })
        .manage(shared_state)
        .invoke_handler(tauri::generate_handler![handle_message])
        .run(tauri::generate_context!())
        .expect("error while running Pixel Agents");
}
