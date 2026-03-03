// ── Timing (ms) ──────────────────────────────────────────────
pub const JSONL_POLL_INTERVAL_MS: u64 = 1000;
pub const FILE_WATCHER_POLL_INTERVAL_MS: u64 = 1000;
pub const TOOL_DONE_DELAY_MS: u64 = 300;
pub const PERMISSION_TIMER_DELAY_MS: u64 = 7000;
pub const TEXT_IDLE_DELAY_MS: u64 = 5000;

// ── Session Scanner ───────────────────────────────────────────
pub const SESSION_ACTIVE_WINDOW_MS: u128 = 24 * 60 * 60 * 1000;  // 24h
pub const SESSION_AUTO_ADD_WINDOW_MS: u128 = 60 * 60 * 1000;      // 1h

// ── Display Truncation ──────────────────────────────────────
pub const BASH_COMMAND_DISPLAY_MAX_LENGTH: usize = 30;
pub const TASK_DESCRIPTION_DISPLAY_MAX_LENGTH: usize = 40;

// ── PNG / Asset Parsing ─────────────────────────────────────
pub const PNG_ALPHA_THRESHOLD: u8 = 128;
pub const WALL_PIECE_WIDTH: u32 = 16;
pub const WALL_PIECE_HEIGHT: u32 = 32;
pub const WALL_GRID_COLS: u32 = 4;
pub const WALL_BITMASK_COUNT: u32 = 16;
pub const FLOOR_PATTERN_COUNT: u32 = 7;
pub const FLOOR_TILE_SIZE: u32 = 16;
pub const CHAR_FRAME_W: u32 = 16;
pub const CHAR_FRAME_H: u32 = 32;
pub const CHAR_FRAMES_PER_ROW: u32 = 7;
pub const CHAR_COUNT: u32 = 6;

// ── User-Level Persistence ────────────────────────────────────
pub const CLODO_HOTEL_DIR: &str = ".clodo-hotel";
pub const LAYOUT_FILE_NAME: &str = "layout.json";
pub const LAYOUT_FILE_POLL_INTERVAL_MS: u64 = 2000;
pub const STANDALONE_STATE_FILE: &str = "standalone-state.json";

// ── Permission Exempt Tools ───────────────────────────────────
pub const PERMISSION_EXEMPT_TOOLS: &[&str] = &["Task", "AskUserQuestion"];

// ── Synthetic tool ID for thinking/text generation ───────────
pub const THINKING_TOOL_ID: &str = "__thinking__";
pub const THINKING_TOOL_STATUS: &str = "Thinking...";
