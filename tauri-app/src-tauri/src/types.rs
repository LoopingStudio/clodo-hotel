use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use serde::{Deserialize, Serialize};

// ── Runtime agent state (not serialized directly) ────────────
pub struct AgentState {
    pub id: u32,
    pub session_id: String,
    pub project_dir: String,
    pub jsonl_file: String,
    pub file_offset: u64,
    pub line_buffer: String,
    pub active_tool_ids: HashSet<String>,
    pub active_tool_statuses: HashMap<String, String>,
    pub active_tool_names: HashMap<String, String>,
    /// parentToolId → set of sub-tool-ids
    pub active_subagent_tool_ids: HashMap<String, HashSet<String>>,
    /// parentToolId → (subToolId → toolName)
    pub active_subagent_tool_names: HashMap<String, HashMap<String, String>>,
    pub is_waiting: bool,
    pub permission_sent: bool,
    pub had_tools_in_turn: bool,
    pub folder_name: Option<String>,
}

impl AgentState {
    pub fn new(
        id: u32,
        session_id: String,
        project_dir: String,
        jsonl_file: String,
        file_offset: u64,
        folder_name: Option<String>,
    ) -> Self {
        Self {
            id,
            session_id,
            project_dir,
            jsonl_file,
            file_offset,
            line_buffer: String::new(),
            active_tool_ids: HashSet::new(),
            active_tool_statuses: HashMap::new(),
            active_tool_names: HashMap::new(),
            active_subagent_tool_ids: HashMap::new(),
            active_subagent_tool_names: HashMap::new(),
            is_waiting: false,
            permission_sent: false,
            had_tools_in_turn: false,
            folder_name,
        }
    }
}

// ── Seat / visual metadata ────────────────────────────────────
#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SeatMeta {
    pub palette: u32,
    pub hue_shift: i32,
    pub seat_id: Option<String>,
}

// ── Shared application state ─────────────────────────────────
pub struct AppState {
    pub agents: HashMap<u32, AgentState>,
    pub known_jsonl_files: HashSet<String>,
    /// agent id → seat/palette meta
    pub agent_seats: HashMap<u32, SeatMeta>,
    pub sound_enabled: bool,
    pub next_agent_id: u32,
    /// Tokio timer handles — aborted to cancel
    pub waiting_timers: HashMap<u32, JoinHandle<()>>,
    pub permission_timers: HashMap<u32, JoinHandle<()>>,
    /// File polling task handle per agent
    pub polling_tasks: HashMap<u32, JoinHandle<()>>,
    /// JSONL-appears-poll task handle per agent
    pub jsonl_poll_tasks: HashMap<u32, JoinHandle<()>>,
    /// Layout file watcher task
    pub layout_watcher_task: Option<JoinHandle<()>>,
    /// Suppress the next layout-file-change event (we wrote it ourselves)
    pub layout_own_write: bool,
    /// Last seen mtime of layout.json in millis
    pub last_layout_mtime: u128,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            known_jsonl_files: HashSet::new(),
            agent_seats: HashMap::new(),
            sound_enabled: true,
            next_agent_id: 1,
            waiting_timers: HashMap::new(),
            permission_timers: HashMap::new(),
            polling_tasks: HashMap::new(),
            jsonl_poll_tasks: HashMap::new(),
            layout_watcher_task: None,
            layout_own_write: false,
            last_layout_mtime: 0,
        }
    }
}

pub type SharedState = Arc<Mutex<AppState>>;

// ── Persistence DTOs ─────────────────────────────────────────
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PersistedAgent {
    pub id: u32,
    pub session_id: String,
    pub jsonl_file: String,
    pub project_dir: String,
    pub folder_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    #[serde(default)]
    pub agents: Vec<PersistedAgent>,
    #[serde(default)]
    pub agent_seats: HashMap<u32, SeatMeta>,
    #[serde(default = "default_sound_enabled")]
    pub sound_enabled: bool,
}

fn default_sound_enabled() -> bool {
    true
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            agents: vec![],
            agent_seats: HashMap::new(),
            sound_enabled: true,
        }
    }
}

// ── Session scanner DTOs ─────────────────────────────────────
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
    pub jsonl_file: String,
    pub last_modified: u64,
    pub project_path: String,
    pub is_tracked: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSessions {
    pub dir_name: String,
    pub project_path: String,
    pub sessions: Vec<SessionInfo>,
}
