# Clodo Hotel — Compressed Reference

Standalone Tauri 2 desktop app (Rust + React): pixel art office where AI agents (Claude Code sessions) are animated characters.

## Architecture

```
tauri-app/src-tauri/src/       — Rust backend (Tauri 2 + Tokio)
  main.rs                      — Entry: setup, handle_message() dispatcher, updater, macOS dock FFI
  constants.rs                 — All backend magic numbers (timing, truncation, asset parsing)
  types.rs                     — AgentState, AppState, PersistedState, SeatMeta
  agent_server.rs              — Agent lifecycle: add, remove, restore, persist
  pty_manager.rs               — PTY lifecycle: spawn portable-pty, read/write/resize, stream output
  session_scanner.rs           — Scan ~/.claude/projects/ for JSONL sessions
  file_watcher.rs              — 500ms polling, readNewLines, partial line buffering
  transcript_parser.rs         — JSONL parsing: tool_use/tool_result → frontend events
  timer_manager.rs             — Waiting/permission timer logic (Tokio tasks)
  layout_persistence.rs        — Layout file I/O (~/.clodo-hotel/layout.json), 2s mtime watcher
  asset_loader.rs              — PNG→SpriteData (2D hex array), catalog/floor/wall/character loading

tauri-app/                     — Vite project wrapping webview-ui
  vite.config.ts               — Root = ../webview-ui, output → tauri-app/dist

webview-ui/src/                — React 19 + TypeScript (Vite)
  constants.ts                 — All webview magic numbers (grid, animation, rendering, camera, zoom, editor)
  notificationSound.ts         — Web Audio API chime on agent turn completion
  appBridge.ts                 — IPC: Tauri invoke/listen ↔ window message events
  App.tsx                      — Composition root, hooks + components + EditActionBar
  dockIcon.ts                  — macOS dock icon animation (idle/active/waiting states)
  hooks/
    useAppMessages.ts          — Message handler + agent/tool state
    useEditorActions.ts        — Editor state + callbacks
    useEditorKeyboard.ts       — Keyboard shortcut effect
  components/
    BottomToolbar.tsx           — + Agent, + Terminal, Layout toggle, Settings button
    SessionPicker.tsx           — Modal to select Claude Code sessions
    TerminalPanel.tsx           — Side panel: xterm.js for PTY agents, transcript for observed
    TranscriptView.tsx          — Read-only JSONL conversation view for observed agents
    ZoomControls.tsx            — +/- zoom (top-right)
    SettingsModal.tsx           — Settings, export/import layout, sound toggle, debug toggle
    DebugView.tsx               — Debug overlay
  office/
    types.ts                   — Interfaces (OfficeLayout, FloorColor, Character, etc.)
    toolUtils.ts               — STATUS_TO_TOOL mapping, extractToolName(), defaultZoom()
    colorize.ts                — Dual-mode: Colorize (grayscale→HSL) + Adjust (HSL shift)
    floorTiles.ts              — Floor sprite storage + colorized cache
    wallTiles.ts               — Wall auto-tile: 16 bitmask sprites from walls.png
    sprites/
      spriteData.ts            — Pixel data: characters (6 pre-colored PNGs), furniture, bubbles
      spriteCache.ts           — SpriteData → offscreen canvas, per-zoom WeakMap cache
    editor/
      editorActions.ts         — Pure layout ops: paint, place, remove, move, rotate, toggleState
      editorState.ts           — Imperative state: tools, ghost, selection, undo/redo, dirty
      EditorToolbar.tsx        — React toolbar/palette for edit mode
    layout/
      furnitureCatalog.ts      — Dynamic catalog from loaded assets + getCatalogEntry()
      layoutSerializer.ts     — OfficeLayout ↔ runtime (tileMap, furniture, seats, blocked)
      tileMap.ts               — Walkability, BFS pathfinding
    engine/
      characters.ts            — Character FSM: idle/walk/type + wander AI
      officeState.ts           — Game world: layout, characters, seats, selection, subagents
      gameLoop.ts              — rAF loop with delta time (capped 0.1s)
      renderer.ts              — Canvas: tiles, z-sorted entities, overlays, edit UI
      matrixEffect.ts          — Matrix-style spawn/despawn digital rain effect
    components/
      OfficeCanvas.tsx         — Canvas, resize, DPR, mouse hit-testing, edit interactions
      ToolOverlay.tsx           — Activity status label above hovered/selected character

scripts/                       — Asset extraction pipeline
  0-import-tileset.ts          — Interactive CLI wrapper
  1-detect-assets.ts           — Flood-fill asset detection
  2-asset-editor.html          — Browser UI for position/bounds editing
  3-vision-inspect.ts          — Claude vision auto-metadata
  4-review-metadata.html       — Browser UI for metadata review
  5-export-assets.ts           — Export PNGs + furniture-catalog.json
  asset-manager.html           — Unified editor (Stage 2+4 combined)
  export-characters.ts         — Bake palette colors into character sprite PNGs
  generate-walls.js            — Generate walls.png (4×4 grid of 16×32 auto-tile pieces)
  wall-tile-editor.html        — Browser UI for editing wall tile appearance
```

## Core Concepts

**Vocabulary**: Session = JSONL conversation file written by Claude Code. Agent = game character bound 1:1 to a session.

**Frontend ↔ Backend IPC**: `appBridge.ts` wraps Tauri's `invoke('handle_message', { message })` + `event.listen('pa-message')`. All communication goes through a single `handle_message()` Rust function that dispatches on `message.type`. Backend emits events via `app_handle.emit("pa-message", payload)`.

**Agent creation**: "+ Agent" button → `SessionPicker` modal shows recent sessions from `~/.claude/projects/` → user picks one → `addSession` message → backend creates `AgentState`, starts JSONL file watching → emits `agentCreated` → frontend creates character. Auto-add: on startup, sessions modified within 1 hour are automatically added.

## Agent Status Tracking

JSONL transcripts at `~/.claude/projects/<project-hash>/<session-id>.jsonl`. Project hash = workspace path with `:`/`\`/`/` → `-`.

**JSONL record types**: `assistant` (tool_use blocks or thinking), `user` (tool_result or text prompt), `system` with `subtype: "turn_duration"` (reliable turn-end signal), `progress` with `data.type`: `agent_progress` (sub-agent tool_use/tool_result), `bash_progress` (long-running Bash output — restarts permission timer), `mcp_progress` (MCP tool status — same timer restart).

**File watching**: 500ms polling interval. Partial line buffering for mid-write reads. Tool done messages delayed 300ms to prevent flicker.

**Backend state per agent** (`AgentState`): `id, session_id, project_dir, jsonl_file, file_offset, line_buffer, active_tool_ids, active_tool_statuses, active_tool_names, active_subagent_tool_ids, active_subagent_tool_names, is_waiting, permission_sent, had_tools_in_turn, folder_name`.

**Shared state** (`AppState`): `agents` HashMap, `known_jsonl_files`, `agent_seats` (palette/hueShift/seatId), `sound_enabled`, `next_agent_id`, timer/task handles (waiting_timers, permission_timers, polling_tasks, jsonl_poll_tasks), layout watcher state.

**Persistence**: Agents + seats + sound setting persisted to `~/.clodo-hotel/standalone-state.json` (atomic write via `.tmp` + rename). Layout persisted to `~/.clodo-hotel/layout.json` (also atomic). Layout watcher polls mtime every 2s for external changes; `markOwnWrite()` flag prevents re-reading own writes.

**Default layout**: Bundled `assets/default-layout.json` loaded on first run. Export current layout as default via settings.

## Office UI

**Rendering**: Game state in imperative `OfficeState` class (not React state). Pixel-perfect: zoom = integer device-pixels-per-sprite-pixel (1x–10x). No `ctx.scale(dpr)`. Default zoom = `Math.round(2 * devicePixelRatio)`. Z-sort all entities by Y. Pan via middle-mouse drag (`panRef`). **Camera follow**: `cameraFollowId` smoothly centers on followed agent; set on click, cleared on deselection or manual pan.

**UI styling**: Pixel art aesthetic — sharp corners (`borderRadius: 0`), solid backgrounds (`#1e1e2e`), `2px solid` borders, hard offset shadows (`2px 2px 0px #0a0a14`, no blur). CSS variables in `index.css` `:root` (`--pixel-bg`, `--pixel-border`, `--pixel-accent`, etc.). Pixel font: FS Pixel Sans.

**Characters**: FSM states — active (pathfind to seat, typing/reading animation by tool type), idle (wander randomly with BFS, return to seat for rest). 4-directional sprites, left = flipped right. Tool animations: typing (Write/Edit/Bash/Task) vs reading (Read/Grep/Glob/WebFetch). Sitting offset: -6px Y in TYPE state. Chair tiles blocked for all characters except their own seat. **Diverse palette**: `pickDiversePalette()` from least-used; first 6 unique, beyond 6 repeat with random hue shift (45–315°). Character stores `palette` (0-5) + `hueShift` (degrees).

**Spawn/despawn effect**: Matrix-style digital rain (0.3s). Restored agents use `skipSpawnEffect: true`.

**Sub-agents**: Negative IDs (from -1 down). Created on `agentToolStart` with "Subtask:" prefix. Same palette + hueShift as parent. Not persisted. Spawn at closest free seat to parent.

**Speech bubbles**: Permission ("..." amber) stays until cleared. Waiting (green checkmark) auto-fades 2s.

**Sound notifications**: Two-note chime (E5 → E6) via Web Audio API on `agentStatus: 'waiting'`. Toggled in Settings.

## Embedded Terminal

**Two agent types**: (1) **Observed agents** — picked from SessionPicker, JSONL-only, transcript view. (2) **Spawned agents** — created via "+ Terminal" button, run `claude --session-id <uuid>` in a `portable-pty`, full interactive xterm.js terminal.

**PTY architecture**: `pty_manager.rs` manages `PtyState` (separate `Arc<Mutex<>>` from `AppState` to avoid contention). `spawn_pty()` creates a `portable-pty` instance, spawns `claude` with session ID, starts a background `spawn_blocking` reader task that emits `ptyOutput` events. `write_pty()` forwards keystrokes. `resize_pty()` resizes the PTY. PTY is auto-closed when agent is removed.

**Frontend**: `TerminalPanel.tsx` renders in a right-side slide-out panel (flex layout). For PTY agents: `@xterm/xterm` + `@xterm/addon-fit` + `@xterm/addon-canvas`. For observed agents: `TranscriptView.tsx` with 2s polling of JSONL data. Panel is resizable via drag handle. Toggle by clicking a character.

**IPC messages**: `spawnAgent` (creates agent + PTY), `spawnPty` / `writePty` / `resizePty` / `closePty` (direct PTY control), `requestTranscript` → `transcriptData` (JSONL read), `ptyOutput` / `ptyExit` / `ptySpawned` / `ptyError` (PTY events).

**Seats**: Derived from chair furniture. Multi-tile chairs produce multiple seats. Facing direction: chair orientation → adjacent desk → forward. Click character → select → click seat → reassign.

**Dock icon**: macOS only. Animated icon reflects aggregate agent state (idle/active/waiting). Badge label via Cocoa FFI (`unsafe` objc calls).

**Sleep mode**: After 5min of inactivity, agents fade and animations slow.

## Layout Editor

Toggle via "Layout" button. Tools: SELECT, Floor paint, Wall paint, Erase, Furniture place, Furniture pick, Eyedropper.

**Floor**: 7 patterns from `floors.png` (grayscale 16×16), colorizable via HSBC sliders (Photoshop Colorize). Color baked per-tile.

**Walls**: Click/drag to add/remove. HSBC color sliders apply to all walls. Eyedropper on wall → Wall tool.

**Furniture**: Ghost preview (green/red). R rotates, T toggles state. Drag-to-move in SELECT. Delete + rotate buttons on selection. HSBC color sliders per-item. Pick tool copies type+color.

**Undo/Redo**: 50-level, Ctrl+Z/Y. EditActionBar: Undo, Redo, Save, Reset.

**Grid expansion**: Ghost border outside grid for expanding (max 64×64, default 20×11).

**Layout model**: `{ version: 1, cols, rows, tiles: TileType[], furniture: PlacedFurniture[], tileColors?: FloorColor[] }`.

## Asset System

**Loading**: Dev assets from `webview-ui/public/assets/`. Prod assets bundled in Tauri resource dir. PNG decoded via `png` crate → 2D hex string array (alpha≥128 = opaque).

**Catalog**: `furniture-catalog.json` — id, name, label, category, footprint, isDesk, canPlaceOnWalls, groupId?, orientation?, state?, canPlaceOnSurfaces?, backgroundTiles?. Categories: desks, chairs, storage, electronics, decor, wall, misc.

**Rotation groups**: Items sharing `groupId` form rotatable sets. Editor palette shows 1 per group.

**State groups**: on/off variants with same `groupId` + `orientation`. Auto-state swaps electronics to ON when active agent faces nearby desk.

**Background tiles**: Top N footprint rows walkable + allow furniture overlap.

**Surface placement**: `canPlaceOnSurfaces` items overlap desk tiles.

**Wall placement**: `canPlaceOnWalls` items require bottom row on wall tiles.

**Colorize module**: Two modes — Colorize (grayscale→HSL, for floors) and Adjust (HSL shift, for furniture/characters).

**Character sprites**: 6 PNGs (`char_0.png`–`char_5.png`), each 112×96 (7 frames × 3 directions × 32px). Frame order: walk1-3, type1-2, read1-2. Generated by `scripts/export-characters.ts`.

**Load order**: `characterSpritesLoaded` → `floorTilesLoaded` → `wallTilesLoaded` → `furnitureAssetsLoaded` → `layoutLoaded`.

## Condensed Lessons

- 500ms JSONL polling is reliable cross-platform (no OS file events needed)
- Partial line buffering essential for append-only file reads
- Delay `agentToolDone` 300ms to prevent React batching from hiding brief active states
- **Idle detection**: (1) `system` + `subtype: "turn_duration"` — reliable for tool-using turns (~98%). (2) Text-idle timer (5s) — for text-only turns. Cancelled by ANY new JSONL data
- User prompt `content` can be string (text) or array (tool_results) — handle both
- `/clear` creates NEW JSONL file (old file stops updating)
- OfficeCanvas selection changes are imperative; must call `onEditorSelectionChange()` for React re-render
- Tauri IPC is async; `appBridge.ts` buffers messages until listener ready

## Build & Dev

```sh
cd webview-ui && npm install && cd ../tauri-app && npm install && cd src-tauri && cargo build
# Dev: from tauri-app/
npm run tauri dev
# Build: from tauri-app/
npm run tauri build
```

## TypeScript Constraints

- No `enum` (`erasableSyntaxOnly`) — use `as const` objects
- `import type` required for type-only imports (`verbatimModuleSyntax`)
- `noUnusedLocals` / `noUnusedParameters`

## Constants

All magic numbers centralized — never add inline constants:

- **Rust backend**: `tauri-app/src-tauri/src/constants.rs` — timing intervals, truncation limits, asset parsing, permission exemptions
- **Webview**: `webview-ui/src/constants.ts` — grid/layout sizes, animation speeds, rendering, camera, zoom, editor defaults
- **CSS styling**: `webview-ui/src/index.css` `:root` — `--pixel-*` custom properties
- **Canvas overlay colors** (rgba strings) live in webview constants (canvas 2D context, not CSS)
- `webview-ui/src/office/types.ts` re-exports grid/layout constants from `constants.ts`

## Key Patterns

- `Arc<Mutex<AppState>>` shared across all Tokio tasks in backend
- Single `handle_message()` entry point dispatches all IPC
- Atomic file writes (`.tmp` + rename) for layout and state persistence
- Cancellable Tokio timers for permission/waiting detection
- macOS dock badge/icon via unsafe Cocoa FFI (`cocoa`/`objc` crates)
- Auto-updater via `tauri-plugin-updater` (GitHub releases)

## Key Decisions

- Standalone Tauri 2 app (not VS Code extension) — runs independently
- Rust backend handles all file I/O, JSONL parsing, timer management
- React webview handles rendering + UI only
- `appBridge.ts` abstracts Tauri IPC so webview code stays framework-agnostic
- Agents are session-based (user picks existing Claude Code sessions), not terminal-based
