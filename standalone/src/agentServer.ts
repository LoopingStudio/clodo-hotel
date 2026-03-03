import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import type { AgentState, PersistedAgent, PersistedState, WebviewLike } from './types.js';
import { cancelWaitingTimer, cancelPermissionTimer } from './timerManager.js';
import { startFileWatching, stopFileWatching, readNewLines } from './fileWatcher.js';
import { PIXEL_AGENTS_DIR, STANDALONE_STATE_FILE, JSONL_POLL_INTERVAL_MS } from './constants.js';

function getStatePath(): string {
	return path.join(os.homedir(), PIXEL_AGENTS_DIR, STANDALONE_STATE_FILE);
}

function ensureDir(): void {
	const dir = path.join(os.homedir(), PIXEL_AGENTS_DIR);
	if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
}

export function loadPersistedState(): PersistedState {
	try {
		const filePath = getStatePath();
		if (!fs.existsSync(filePath)) return { agents: [], agentSeats: {}, soundEnabled: true };
		return JSON.parse(fs.readFileSync(filePath, 'utf-8')) as PersistedState;
	} catch {
		return { agents: [], agentSeats: {}, soundEnabled: true };
	}
}

export function savePersistedState(state: PersistedState): void {
	try {
		ensureDir();
		const filePath = getStatePath();
		const tmp = filePath + '.tmp';
		fs.writeFileSync(tmp, JSON.stringify(state, null, 2), 'utf-8');
		fs.renameSync(tmp, filePath);
	} catch (err) {
		console.error('[Pixel Agents] Failed to save state:', err);
	}
}

export function persistAgents(
	agents: Map<number, AgentState>,
	agentSeats: Record<number, { palette: number; hueShift: number; seatId: string | null }>,
	soundEnabled: boolean,
): void {
	const persisted: PersistedAgent[] = [];
	for (const agent of agents.values()) {
		persisted.push({
			id: agent.id,
			sessionId: agent.sessionId,
			jsonlFile: agent.jsonlFile,
			projectDir: agent.projectDir,
			folderName: agent.folderName,
		});
	}
	savePersistedState({ agents: persisted, agentSeats, soundEnabled });
}

export function addSessionAsAgent(
	sessionId: string,
	projectDir: string,
	jsonlFile: string,
	folderName: string | undefined,
	nextAgentIdRef: { current: number },
	agents: Map<number, AgentState>,
	knownJsonlFiles: Set<string>,
	fileWatchers: Map<number, fs.FSWatcher>,
	pollingTimers: Map<number, ReturnType<typeof setInterval>>,
	waitingTimers: Map<number, ReturnType<typeof setTimeout>>,
	permissionTimers: Map<number, ReturnType<typeof setTimeout>>,
	jsonlPollTimers: Map<number, ReturnType<typeof setInterval>>,
	webview: WebviewLike | undefined,
	doPersist: () => void,
): number {
	// Don't add duplicates
	for (const agent of agents.values()) {
		if (agent.jsonlFile === jsonlFile) return agent.id;
	}

	const id = nextAgentIdRef.current++;
	const agent: AgentState = {
		id,
		sessionId,
		projectDir,
		jsonlFile,
		fileOffset: 0,
		lineBuffer: '',
		activeToolIds: new Set(),
		activeToolStatuses: new Map(),
		activeToolNames: new Map(),
		activeSubagentToolIds: new Map(),
		activeSubagentToolNames: new Map(),
		isWaiting: false,
		permissionSent: false,
		hadToolsInTurn: false,
		folderName,
	};

	agents.set(id, agent);
	knownJsonlFiles.add(jsonlFile);
	doPersist();

	webview?.postMessage({ type: 'agentCreated', id, folderName });

	// If JSONL already exists, skip to end and start watching
	if (fs.existsSync(jsonlFile)) {
		try {
			agent.fileOffset = fs.statSync(jsonlFile).size;
		} catch { /* ignore */ }
		startFileWatching(id, jsonlFile, agents, fileWatchers, pollingTimers, waitingTimers, permissionTimers, webview);
	} else {
		// Poll until file appears
		const pollTimer = setInterval(() => {
			try {
				if (fs.existsSync(agent.jsonlFile)) {
					clearInterval(pollTimer);
					jsonlPollTimers.delete(id);
					agent.fileOffset = fs.statSync(agent.jsonlFile).size;
					startFileWatching(id, agent.jsonlFile, agents, fileWatchers, pollingTimers, waitingTimers, permissionTimers, webview);
				}
			} catch { /* ignore */ }
		}, JSONL_POLL_INTERVAL_MS);
		jsonlPollTimers.set(id, pollTimer);
	}

	return id;
}

export function removeAgent(
	agentId: number,
	agents: Map<number, AgentState>,
	knownJsonlFiles: Set<string>,
	fileWatchers: Map<number, fs.FSWatcher>,
	pollingTimers: Map<number, ReturnType<typeof setInterval>>,
	waitingTimers: Map<number, ReturnType<typeof setTimeout>>,
	permissionTimers: Map<number, ReturnType<typeof setTimeout>>,
	jsonlPollTimers: Map<number, ReturnType<typeof setInterval>>,
	doPersist: () => void,
): void {
	const agent = agents.get(agentId);
	if (!agent) return;

	const jpTimer = jsonlPollTimers.get(agentId);
	if (jpTimer) clearInterval(jpTimer);
	jsonlPollTimers.delete(agentId);

	stopFileWatching(agentId, agent.jsonlFile, fileWatchers, pollingTimers);
	cancelWaitingTimer(agentId, waitingTimers);
	cancelPermissionTimer(agentId, permissionTimers);

	knownJsonlFiles.delete(agent.jsonlFile);
	agents.delete(agentId);
	doPersist();
}

export function restoreAgents(
	persisted: PersistedAgent[],
	nextAgentIdRef: { current: number },
	agents: Map<number, AgentState>,
	knownJsonlFiles: Set<string>,
	fileWatchers: Map<number, fs.FSWatcher>,
	pollingTimers: Map<number, ReturnType<typeof setInterval>>,
	waitingTimers: Map<number, ReturnType<typeof setTimeout>>,
	permissionTimers: Map<number, ReturnType<typeof setTimeout>>,
	webview: WebviewLike | undefined,
): void {
	let maxId = 0;

	for (const p of persisted) {
		if (!fs.existsSync(p.jsonlFile)) continue;

		const agent: AgentState = {
			id: p.id,
			sessionId: p.sessionId,
			projectDir: p.projectDir,
			jsonlFile: p.jsonlFile,
			fileOffset: 0,
			lineBuffer: '',
			activeToolIds: new Set(),
			activeToolStatuses: new Map(),
			activeToolNames: new Map(),
			activeSubagentToolIds: new Map(),
			activeSubagentToolNames: new Map(),
			isWaiting: false,
			permissionSent: false,
			hadToolsInTurn: false,
			folderName: p.folderName,
		};

		try {
			agent.fileOffset = fs.statSync(p.jsonlFile).size;
		} catch { /* ignore */ }

		agents.set(p.id, agent);
		knownJsonlFiles.add(p.jsonlFile);
		if (p.id > maxId) maxId = p.id;

		startFileWatching(
			p.id, p.jsonlFile, agents, fileWatchers, pollingTimers,
			waitingTimers, permissionTimers, webview,
		);
	}

	if (maxId >= nextAgentIdRef.current) nextAgentIdRef.current = maxId + 1;
}

export function sendExistingAgents(
	agents: Map<number, AgentState>,
	agentSeats: Record<number, { palette: number; hueShift: number; seatId: string | null }>,
	webview: WebviewLike | undefined,
): void {
	if (!webview) return;
	const agentIds = [...agents.keys()].sort((a, b) => a - b);
	const folderNames: Record<number, string> = {};
	for (const [id, agent] of agents) {
		if (agent.folderName) folderNames[id] = agent.folderName;
	}
	webview.postMessage({ type: 'existingAgents', agents: agentIds, agentMeta: agentSeats, folderNames });

	// Re-send active tool statuses
	for (const [agentId, agent] of agents) {
		for (const [toolId, status] of agent.activeToolStatuses) {
			webview.postMessage({ type: 'agentToolStart', id: agentId, toolId, status });
		}
		if (agent.isWaiting) {
			webview.postMessage({ type: 'agentStatus', id: agentId, status: 'waiting' });
		}
	}
}

export function replayRecentLines(
	agentId: number,
	agents: Map<number, AgentState>,
	waitingTimers: Map<number, ReturnType<typeof setTimeout>>,
	permissionTimers: Map<number, ReturnType<typeof setTimeout>>,
	webview: WebviewLike | undefined,
): void {
	const agent = agents.get(agentId);
	if (!agent) return;
	// Reset offset to 0 and re-read from the beginning
	agent.fileOffset = 0;
	agent.lineBuffer = '';
	readNewLines(agentId, agents, waitingTimers, permissionTimers, webview);
}
