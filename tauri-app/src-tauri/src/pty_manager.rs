use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::Mutex;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::Emitter;

/// Try common locations for the `claude` binary, fall back to bare name (relies on PATH).
fn resolve_claude_path() -> String {
    let candidates = [
        dirs::home_dir().map(|h| h.join(".local/bin/claude")),
        Some(std::path::PathBuf::from("/usr/local/bin/claude")),
        Some(std::path::PathBuf::from("/opt/homebrew/bin/claude")),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }
    "claude".to_string()
}

/// PTY instance state
struct PtyInstance {
    /// Writer to send input to the PTY
    writer: Box<dyn Write + Send>,
    /// Master kept alive to prevent PTY from being dropped (slave is dropped after spawn)
    _master: Box<dyn MasterPty + Send>,
    /// Background reader task
    reader_task: tokio::task::JoinHandle<()>,
}

/// Separate state for PTY instances (not in AppState to avoid Mutex contention)
pub struct PtyState {
    instances: HashMap<u32, PtyInstance>,
}

impl PtyState {
    pub fn new() -> Self {
        Self {
            instances: HashMap::new(),
        }
    }
}

pub type SharedPtyState = Arc<Mutex<PtyState>>;

/// Spawn a new PTY running `claude` with the given session ID and project directory.
pub async fn spawn_pty(
    agent_id: u32,
    session_id: &str,
    project_dir: &str,
    cols: u16,
    rows: u16,
    pty_state: &SharedPtyState,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let pty_system = native_pty_system();

    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to open PTY: {e}"))?;

    // Destructure pair so we can drop slave independently
    let portable_pty::PtyPair { master, slave } = pair;

    // Resolve claude binary — app may not inherit shell PATH
    let claude_bin = resolve_claude_path();
    let mut cmd = CommandBuilder::new(&claude_bin);
    cmd.arg("--session-id");
    cmd.arg(session_id);

    // Ensure PATH includes common locations for child processes
    if let Some(path) = std::env::var_os("PATH") {
        let mut new_path = std::ffi::OsString::from("/usr/local/bin:/opt/homebrew/bin:");
        if let Some(home) = dirs::home_dir() {
            new_path.push(home.join(".local/bin").as_os_str());
            new_path.push(":");
        }
        new_path.push(&path);
        cmd.env("PATH", new_path);
    }

    // Set working directory if provided
    if !project_dir.is_empty() {
        let path = std::path::Path::new(project_dir);
        if path.exists() {
            cmd.cwd(path);
        }
    }

    let _child = slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn claude: {e}"))?;

    // Drop the slave — critical on macOS: keeping the slave FD open prevents
    // the master reader from receiving output.
    drop(slave);

    let mut reader = master
        .try_clone_reader()
        .map_err(|e| format!("Failed to clone PTY reader: {e}"))?;

    let writer = master
        .take_writer()
        .map_err(|e| format!("Failed to take PTY writer: {e}"))?;

    // Spawn background task to read PTY output and emit to webview
    let app = app_handle.clone();
    let id = agent_id;
    eprintln!("[Clodo Hotel] PTY reader task starting for agent {id}");
    let reader_task = tokio::task::spawn_blocking(move || {
        eprintln!("[Clodo Hotel] PTY reader thread running for agent {id}");
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    eprintln!("[Clodo Hotel] PTY EOF for agent {id}");
                    let _ = app.emit(
                        "pa-message",
                        serde_json::json!({
                            "type": "ptyExit",
                            "agentId": id,
                        }),
                    );
                    break;
                }
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buf[..n]).to_string();
                    eprintln!("[Clodo Hotel] PTY read {n} bytes for agent {id}");
                    let _ = app.emit(
                        "pa-message",
                        serde_json::json!({
                            "type": "ptyOutput",
                            "agentId": id,
                            "data": data,
                        }),
                    );
                }
                Err(e) => {
                    eprintln!("[Clodo Hotel] PTY read error for agent {id}: {e}");
                    break;
                }
            }
        }
    });

    let instance = PtyInstance {
        writer,
        _master: master,
        reader_task,
    };

    let mut ps = pty_state.lock().await;
    ps.instances.insert(agent_id, instance);

    Ok(())
}

/// Write input data to an agent's PTY.
pub async fn write_pty(
    agent_id: u32,
    data: &str,
    pty_state: &SharedPtyState,
) -> Result<(), String> {
    let mut ps = pty_state.lock().await;
    if let Some(instance) = ps.instances.get_mut(&agent_id) {
        instance
            .writer
            .write_all(data.as_bytes())
            .map_err(|e| format!("PTY write error: {e}"))?;
        instance
            .writer
            .flush()
            .map_err(|e| format!("PTY flush error: {e}"))?;
    }
    Ok(())
}

/// Resize an agent's PTY.
pub async fn resize_pty(
    agent_id: u32,
    cols: u16,
    rows: u16,
    pty_state: &SharedPtyState,
) -> Result<(), String> {
    let ps = pty_state.lock().await;
    if let Some(instance) = ps.instances.get(&agent_id) {
        instance
            ._master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("PTY resize error: {e}"))?;
    }
    Ok(())
}

/// Close an agent's PTY and clean up.
pub async fn close_pty(agent_id: u32, pty_state: &SharedPtyState) {
    let mut ps = pty_state.lock().await;
    if let Some(instance) = ps.instances.remove(&agent_id) {
        instance.reader_task.abort();
        // Writer and pair are dropped here, closing the PTY
    }
}

/// Check if an agent has an active PTY.
pub async fn has_pty(agent_id: u32, pty_state: &SharedPtyState) -> bool {
    let ps = pty_state.lock().await;
    ps.instances.contains_key(&agent_id)
}
