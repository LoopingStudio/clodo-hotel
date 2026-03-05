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
mod pty_manager;

use std::sync::Arc;
use tokio::sync::Mutex;

use tauri::{Emitter, Manager};

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
use pty_manager::{PtyState, SharedPtyState};

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

fn set_dock_badge(label: &str) {
    use cocoa::appkit::NSApp;
    use cocoa::base::id;
    use cocoa::foundation::NSString;
    use objc::{msg_send, sel, sel_impl};
    unsafe {
        let app: id = NSApp();
        let dock_tile: id = msg_send![app, dockTile];
        let ns_label: id = NSString::alloc(cocoa::base::nil).init_str(label);
        let _: () = msg_send![dock_tile, setBadgeLabel: ns_label];
    }
}

fn set_dock_icon(png_bytes: &[u8]) {
    use cocoa::appkit::NSApp;
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSData as CocoaNSData;
    use objc::{msg_send, sel, sel_impl, class};
    unsafe {
        let data: id = CocoaNSData::dataWithBytes_length_(
            nil,
            png_bytes.as_ptr() as *const std::ffi::c_void,
            png_bytes.len() as u64,
        );
        let image: id = msg_send![class!(NSImage), alloc];
        let image: id = msg_send![image, initWithData: data];
        if image != nil {
            let app: id = NSApp();
            let _: () = msg_send![app, setApplicationIconImage: image];
        }
    }
}

/// Single entry point from the webview for all messages.
/// Re-dispatches based on `message.type`.
#[tauri::command]
async fn handle_message(
    message: serde_json::Value,
    state: tauri::State<'_, SharedState>,
    pty_state: tauri::State<'_, SharedPtyState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let state_arc = state.inner().clone();
    let pty_arc = pty_state.inner().clone();
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
                pty_manager::close_pty(id as u32, &pty_arc).await;

                emit_available_sessions(&state_arc, &app_handle).await;
            }
        }

        "saveLayout" => {
            if let Some(layout) = message.get("layout") {
                if let Err(e) = write_layout_to_file(layout, &app_handle) {
                    eprintln!("[Clodo Hotel] saveLayout error: {e}");
                } else {
                    mark_own_write(&state_arc, &app_handle).await;
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
                    if let Err(e) = write_layout_to_file(layout, &app_handle) {
                        eprintln!("[Clodo Hotel] importLayoutData error: {e}");
                    } else {
                        mark_own_write(&state_arc, &app_handle).await;
                        let _ = app_handle.emit(
                            "pa-message",
                            serde_json::json!({ "type": "layoutLoaded", "layout": layout }),
                        );
                    }
                }
            }
        }

        "exportLayout" => {
            if let Some(layout_val) = read_layout_from_file(&app_handle) {
                let ah = app_handle.clone();
                tokio::task::spawn_blocking(move || {
                    use tauri_plugin_dialog::DialogExt;
                    if let Some(file_path) = ah
                        .dialog()
                        .file()
                        .set_file_name("clodo-hotel-layout.json")
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
                if write_layout_to_file(&layout, &app_handle).is_ok() {
                    mark_own_write(&state_arc, &app_handle).await;
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

        "setDockIcon" => {
            if let Some(b64) = message.get("png").and_then(|v| v.as_str()) {
                use base64::Engine;
                if let Ok(png_bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                    set_dock_icon(&png_bytes);
                }
            }
        }

        "setDockBadge" => {
            let label = message.get("label").and_then(|v| v.as_str()).unwrap_or("");
            set_dock_badge(label);
        }

        "relaunchApp" => {
            app_handle.restart();
        }

        // ── Spawn new agent with embedded terminal ────────────
        "spawnAgent" => {
            let project_dir;
            let folder_name;

            let has_dir = message.get("projectDir").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
            if has_dir {
                project_dir = message.get("projectDir").and_then(|v| v.as_str()).unwrap_or("").to_string();
                folder_name = message.get("folderName").and_then(|v| v.as_str()).map(|s| s.to_string());
            } else {
                // Open native folder picker
                let ah = app_handle.clone();
                let picked = tokio::task::spawn_blocking(move || {
                    use tauri_plugin_dialog::DialogExt;
                    ah.dialog()
                        .file()
                        .blocking_pick_folder()
                        .and_then(|fp| fp.into_path().ok())
                })
                .await
                .ok()
                .flatten();

                match picked {
                    Some(path) => {
                        folder_name = path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string());
                        project_dir = path.to_string_lossy().to_string();
                    }
                    None => return Ok(()), // User cancelled
                }
            }

            let cols = message.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
            let rows = message.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;

            // Generate a new session ID
            let session_id = uuid::Uuid::new_v4().to_string();

            // Determine JSONL file path
            let project_hash = project_dir.replace([':', '\\', '/'], "-");
            let jsonl_dir = dirs::home_dir()
                .unwrap_or_default()
                .join(".claude")
                .join("projects")
                .join(&project_hash);
            let jsonl_file = jsonl_dir.join(format!("{session_id}.jsonl")).to_string_lossy().to_string();

            // Create the agent (will start watching JSONL when it appears)
            let agent_id = add_session_as_agent(
                session_id.clone(),
                project_dir.clone(),
                jsonl_file,
                folder_name,
                &state_arc,
                &app_handle,
            ).await;

            // Spawn PTY with claude
            if let Err(e) = pty_manager::spawn_pty(
                agent_id, &session_id, &project_dir, cols, rows, &pty_arc, &app_handle,
            ).await {
                eprintln!("[Clodo Hotel] spawnAgent PTY error: {e}");
                let _ = app_handle.emit(
                    "pa-message",
                    serde_json::json!({ "type": "ptyError", "agentId": agent_id, "error": e }),
                );
            } else {
                let _ = app_handle.emit(
                    "pa-message",
                    serde_json::json!({ "type": "ptySpawned", "agentId": agent_id }),
                );
            }

            emit_available_sessions(&state_arc, &app_handle).await;
        }

        // ── PTY management ──────────────────────────────────────
        "spawnPty" => {
            let agent_id = message.get("agentId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let session_id = message.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let project_dir = message.get("projectDir").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let cols = message.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
            let rows = message.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;

            if let Err(e) = pty_manager::spawn_pty(
                agent_id, &session_id, &project_dir, cols, rows, &pty_arc, &app_handle,
            ).await {
                eprintln!("[Clodo Hotel] spawnPty error: {e}");
                let _ = app_handle.emit(
                    "pa-message",
                    serde_json::json!({ "type": "ptyError", "agentId": agent_id, "error": e }),
                );
            }
        }

        "writePty" => {
            let agent_id = message.get("agentId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let data = message.get("data").and_then(|v| v.as_str()).unwrap_or("");
            if let Err(e) = pty_manager::write_pty(agent_id, data, &pty_arc).await {
                eprintln!("[Clodo Hotel] writePty error: {e}");
            }
        }

        "resizePty" => {
            let agent_id = message.get("agentId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let cols = message.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
            let rows = message.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;
            if let Err(e) = pty_manager::resize_pty(agent_id, cols, rows, &pty_arc).await {
                eprintln!("[Clodo Hotel] resizePty error: {e}");
            }
        }

        "requestTranscript" => {
            let agent_id = message.get("agentId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let limit = message.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;

            let jsonl_file = {
                let s = state_arc.lock().await;
                s.agents.get(&agent_id).map(|a| a.jsonl_file.clone())
            };

            if let Some(file) = jsonl_file {
                if let Ok(content) = std::fs::read_to_string(&file) {
                    let lines: Vec<serde_json::Value> = content
                        .lines()
                        .rev()
                        .take(limit)
                        .filter_map(|l| serde_json::from_str(l).ok())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();

                    let _ = app_handle.emit(
                        "pa-message",
                        serde_json::json!({
                            "type": "transcriptData",
                            "agentId": agent_id,
                            "lines": lines,
                        }),
                    );
                }
            }
        }

        "closePty" => {
            let agent_id = message.get("agentId").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            pty_manager::close_pty(agent_id, &pty_arc).await;
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
    let layout = asset_loader::load_default_layout(&assets_root);

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
            Ok(None) => return,
            Err(e) => { eprintln!("[updater] check failed: {e}"); return; }
        },
        Err(e) => { eprintln!("[updater] init failed: {e}"); return; }
    };
    let version = update.version.clone();
    println!("[updater] update available: v{version}");
    let _ = app.emit(
        "pa-message",
        serde_json::json!({ "type": "updateAvailable", "version": version }),
    );
    match update.download_and_install(|_, _| {}, || {}).await {
        Ok(_) => {
            println!("[updater] update ready, waiting for relaunch");
            let _ = app.emit("pa-message", serde_json::json!({ "type": "updateReady" }));
        }
        Err(e) => eprintln!("[updater] download/install failed: {e}"),
    }
}


fn main() {
    let shared_state: SharedState = Arc::new(Mutex::new(AppState::new()));
    let shared_pty_state: SharedPtyState = Arc::new(Mutex::new(PtyState::new()));

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
        .manage(shared_pty_state)
        .invoke_handler(tauri::generate_handler![handle_message])
        .run(tauri::generate_context!())
        .expect("error while running Clodo Hotel");
}
