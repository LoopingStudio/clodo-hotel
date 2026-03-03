import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { fileURLToPath } from 'url';
import http from 'http';
import express from 'express';
import { WebSocketServer, type WebSocket } from 'ws';
import type { WebviewLike } from './src/types.js';
import { DEFAULT_PORT, SESSION_AUTO_ADD_WINDOW_MS } from './src/constants.js';
import {
	loadFurnitureAssets, loadFloorTiles, loadWallTiles, loadCharacterSprites, loadDefaultLayout,
	sendAssetsToWebview, sendFloorTilesToWebview, sendWallTilesToWebview, sendCharacterSpritesToWebview,
} from './src/assetLoader.js';
import { readLayoutFromFile, writeLayoutToFile, watchLayoutFile } from './src/layoutPersistence.js';
import { scanSessions, findRecentSessions } from './src/sessionScanner.js';
import {
	addSessionAsAgent, removeAgent, restoreAgents, sendExistingAgents,
	persistAgents, loadPersistedState,
} from './src/agentServer.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PORT = process.env.PORT ? parseInt(process.env.PORT) : DEFAULT_PORT;

// ── Asset paths ───────────────────────────────────────────────
// Assumes the main project is built: dist/assets/ and dist/webview/ exist
const projectRoot = path.join(__dirname, '..');
const assetsRoot = path.join(projectRoot, 'dist');
const webviewDir = path.join(projectRoot, 'dist', 'webview');

// ── State ─────────────────────────────────────────────────────
const agents = new Map<number, import('./src/types.js').AgentState>();
const knownJsonlFiles = new Set<string>();
const fileWatchers = new Map<number, fs.FSWatcher>();
const pollingTimers = new Map<number, ReturnType<typeof setInterval>>();
const waitingTimers = new Map<number, ReturnType<typeof setTimeout>>();
const permissionTimers = new Map<number, ReturnType<typeof setTimeout>>();
const jsonlPollTimers = new Map<number, ReturnType<typeof setInterval>>();
const nextAgentId = { current: 1 };

let persistedState = loadPersistedState();
let agentSeats = persistedState.agentSeats;
let soundEnabled = persistedState.soundEnabled;
let layoutWatcher: ReturnType<typeof watchLayoutFile> | null = null;
let defaultLayout: Record<string, unknown> | null = null;

// Active WebSocket connections
const clients = new Set<WebSocket>();

function doPersist(): void {
	persistAgents(agents, agentSeats, soundEnabled);
}

// Broadcast to all connected clients
function broadcast(msg: unknown): void {
	const data = JSON.stringify(msg);
	for (const client of clients) {
		if (client.readyState === 1 /* OPEN */) {
			client.send(data);
		}
	}
}

const webviewLike: WebviewLike = {
	postMessage(msg: unknown): void {
		broadcast(msg);
	},
};

// ── Express setup ─────────────────────────────────────────────
const app = express();

if (fs.existsSync(webviewDir)) {
	app.use(express.static(webviewDir));
	app.get('/', (_req, res) => {
		res.sendFile(path.join(webviewDir, 'index.html'));
	});
} else {
	app.get('/', (_req, res) => {
		res.send('<h2>Pixel Agents — run <code>npm run build</code> in the project root first</h2>');
	});
}

const server = http.createServer(app);
const wss = new WebSocketServer({ server, path: '/ws' });

// ── WebSocket handler ─────────────────────────────────────────
wss.on('connection', (ws: WebSocket) => {
	clients.add(ws);
	console.log(`[Server] Client connected (${clients.size} total)`);

	ws.on('close', () => {
		clients.delete(ws);
		console.log(`[Server] Client disconnected (${clients.size} total)`);
	});

	ws.on('message', (data) => {
		let message: Record<string, unknown>;
		try {
			message = JSON.parse(data.toString()) as Record<string, unknown>;
		} catch {
			return;
		}

		const clientWebview: WebviewLike = {
			postMessage(msg: unknown): void {
				if (ws.readyState === 1) ws.send(JSON.stringify(msg));
			},
		};

		const type = message.type as string;

		if (type === 'webviewReady') {
			handleWebviewReady(clientWebview);
		} else if (type === 'openClaude') {
			// In standalone mode, respond with available sessions
			const projects = scanSessions(knownJsonlFiles);
			clientWebview.postMessage({ type: 'availableSessions', projects });
		} else if (type === 'scanSessions') {
			const projects = scanSessions(knownJsonlFiles);
			clientWebview.postMessage({ type: 'availableSessions', projects });
		} else if (type === 'addSession') {
			const sessionId = message.sessionId as string;
			const jsonlFile = message.jsonlFile as string;
			const projectDir = path.dirname(jsonlFile);
			const folderName = message.folderName as string | undefined;
			addSessionAsAgent(
				sessionId, projectDir, jsonlFile, folderName,
				nextAgentId, agents, knownJsonlFiles,
				fileWatchers, pollingTimers, waitingTimers, permissionTimers, jsonlPollTimers,
				webviewLike, doPersist,
			);
		} else if (type === 'closeAgent' || type === 'removeSession') {
			const id = message.id as number;
			removeAgent(id, agents, knownJsonlFiles, fileWatchers, pollingTimers, waitingTimers, permissionTimers, jsonlPollTimers, doPersist);
			broadcast({ type: 'agentClosed', id });
			// Refresh the session picker so the removed session reappears as available
			const projects = scanSessions(knownJsonlFiles);
			broadcast({ type: 'availableSessions', projects });
		} else if (type === 'saveLayout') {
			layoutWatcher?.markOwnWrite();
			writeLayoutToFile(message.layout as Record<string, unknown>);
		} else if (type === 'saveAgentSeats') {
			agentSeats = message.seats as typeof agentSeats;
			doPersist();
		} else if (type === 'setSoundEnabled') {
			soundEnabled = message.enabled as boolean;
			doPersist();
		} else if (type === 'importLayoutData') {
			const layout = message.layout as Record<string, unknown>;
			if (layout?.version === 1 && Array.isArray(layout.tiles)) {
				layoutWatcher?.markOwnWrite();
				writeLayoutToFile(layout);
				broadcast({ type: 'layoutLoaded', layout });
			}
		} else if (type === 'exportLayout') {
			const layout = readLayoutFromFile();
			if (layout) {
				clientWebview.postMessage({ type: 'exportLayoutData', layout });
			}
		}
	});
});

async function handleWebviewReady(clientWebview: WebviewLike): Promise<void> {
	// Restore persisted agents only on cold start (agents Map is empty).
	// On browser refresh the server still holds the agents — restoreAgents
	// would overwrite them and create duplicate file-watchers.
	if (agents.size === 0) {
		restoreAgents(
			persistedState.agents, nextAgentId, agents, knownJsonlFiles,
			fileWatchers, pollingTimers, waitingTimers, permissionTimers,
			webviewLike,
		);
	}

	// Send settings
	clientWebview.postMessage({ type: 'settingsLoaded', soundEnabled });

	// Load and send assets
	try {
		if (fs.existsSync(path.join(assetsRoot, 'assets'))) {
			defaultLayout = loadDefaultLayout(assetsRoot);

			const charSprites = await loadCharacterSprites(assetsRoot);
			if (charSprites) sendCharacterSpritesToWebview(clientWebview, charSprites);

			const floorTiles = await loadFloorTiles(assetsRoot);
			if (floorTiles) sendFloorTilesToWebview(clientWebview, floorTiles);

			const wallTiles = await loadWallTiles(assetsRoot);
			if (wallTiles) sendWallTilesToWebview(clientWebview, wallTiles);

			const assets = await loadFurnitureAssets(assetsRoot);
			if (assets) sendAssetsToWebview(clientWebview, assets);
		}
	} catch (err) {
		console.error('[Server] Error loading assets:', err);
	}

	// Send existing agents BEFORE layoutLoaded — the webview buffers agents in
	// pendingAgents and flushes them inside the layoutLoaded handler. If layoutLoaded
	// arrives first, pendingAgents is still empty and no characters are spawned.
	sendExistingAgents(agents, agentSeats, clientWebview);

	// Send layout
	const layout = readLayoutFromFile() ?? defaultLayout;
	clientWebview.postMessage({ type: 'layoutLoaded', layout });

	// Start layout watcher (once)
	if (!layoutWatcher) {
		layoutWatcher = watchLayoutFile((newLayout) => {
			broadcast({ type: 'layoutLoaded', layout: newLayout });
		});
	}

	// Auto-add sessions active in the last hour (only on first client connect)
	if (clients.size === 1 && agents.size === 0) {
		const recent = findRecentSessions(SESSION_AUTO_ADD_WINDOW_MS, knownJsonlFiles);
		for (const session of recent) {
			const projectDir = path.dirname(session.jsonlFile);
			const folderName = path.basename(session.projectPath);
			addSessionAsAgent(
				session.sessionId, projectDir, session.jsonlFile, folderName,
				nextAgentId, agents, knownJsonlFiles,
				fileWatchers, pollingTimers, waitingTimers, permissionTimers, jsonlPollTimers,
				webviewLike, doPersist,
			);
		}
		if (recent.length > 0) {
			console.log(`[Server] Auto-added ${recent.length} recent session(s)`);
		}
	}

	// Send available sessions so the picker is populated immediately
	const projects = scanSessions(knownJsonlFiles);
	clientWebview.postMessage({ type: 'availableSessions', projects });
}

// ── Graceful shutdown ─────────────────────────────────────────
process.on('SIGINT', () => {
	console.log('\n[Server] Shutting down...');
	layoutWatcher?.dispose();
	for (const id of agents.keys()) {
		removeAgent(id, agents, knownJsonlFiles, fileWatchers, pollingTimers, waitingTimers, permissionTimers, jsonlPollTimers, doPersist);
	}
	server.close(() => process.exit(0));
});

// ── Start ─────────────────────────────────────────────────────
server.listen(PORT, () => {
	console.log(`\n🎮 Pixel Agents running at http://localhost:${PORT}`);
	console.log(`   Claude projects: ${os.homedir()}/.claude/projects/`);
	console.log(`   Layout file:     ${os.homedir()}/.pixel-agents/layout.json\n`);
});
