# Clodo Hotel

A desktop application that turns your Claude Code agents into animated pixel art characters in a virtual office.

Each Claude Code session you open spawns a character that walks around, sits at desks, and visually reflects what the agent is doing — typing when writing code, reading when searching files, waiting when it needs your attention.

![Clodo Hotel screenshot](webview-ui/public/Screenshot.jpg)

## Features

- **One agent, one character** — every Claude Code session gets its own animated character
- **Live activity tracking** — characters animate based on what the agent is actually doing (writing, reading, running commands)
- **Office layout editor** — design your office with floors, walls, and furniture using a built-in editor
- **Speech bubbles** — visual indicators when an agent is waiting for input or needs permission
- **Sound notifications** — optional chime when an agent finishes its turn
- **Sub-agent visualization** — Task tool sub-agents spawn as separate characters linked to their parent
- **Persistent layouts** — your office design is saved to `~/.clodo-hotel/layout.json`
- **Dynamic dock icon** — the macOS dock icon reflects agent state (idle / active / waiting)
- **Auto-update** — the app checks for new releases on launch and updates in the background
- **Diverse characters** — 6 diverse characters based on the amazing work of [JIK-A-4, Metro City](https://jik-a-4.itch.io/metrocity-free-topdown-character-pack)

<p align="center">
  <img src="webview-ui/public/characters.png" alt="Clodo Hotel characters" width="320" height="72" style="image-rendering: pixelated;">
</p>

## Requirements

- macOS
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) installed and configured

## Getting Started

Download the latest release from [Releases](https://github.com/LoopingStudio/clodo-hotel/releases/latest) and install it.

### Usage

1. Launch **Clodo Hotel** from your Applications folder
2. Click **+ Agent** to pick a recent Claude Code session and spawn its character
3. Watch the characters react in real time as Claude works
4. Click a character to select it, then click a seat to reassign it
5. Click **Layout** to open the office editor and customize your space

## How It Works

Clodo Hotel watches Claude Code's JSONL transcript files (`~/.claude/projects/`) to track what each agent is doing. When an agent uses a tool (like writing a file or running a command), the app detects it and updates the character's animation accordingly. No modifications to Claude Code are needed — it's purely observational.

The interface runs a lightweight game loop with canvas rendering, BFS pathfinding, and a character state machine (idle → walk → type/read). Everything is pixel-perfect at integer zoom levels.
## Tech Stack

- **Backend**: Rust, Tauri 2
- **Frontend**: React 19, TypeScript, Vite, Canvas 2D

## Contributions

See [CONTRIBUTORS.md](CONTRIBUTORS.md) for instructions on how to contribute.

Please read our [Code of Conduct](CODE_OF_CONDUCT.md) before participating.

## License

This project is licensed under the [MIT License](LICENSE).
