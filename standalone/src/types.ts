export interface WebviewLike {
	postMessage(msg: unknown): void;
}

export interface AgentState {
	id: number;
	sessionId: string;
	projectDir: string;
	jsonlFile: string;
	fileOffset: number;
	lineBuffer: string;
	activeToolIds: Set<string>;
	activeToolStatuses: Map<string, string>;
	activeToolNames: Map<string, string>;
	activeSubagentToolIds: Map<string, Set<string>>;
	activeSubagentToolNames: Map<string, Map<string, string>>;
	isWaiting: boolean;
	permissionSent: boolean;
	hadToolsInTurn: boolean;
	folderName?: string;
}

export interface PersistedAgent {
	id: number;
	sessionId: string;
	jsonlFile: string;
	projectDir: string;
	folderName?: string;
}

export interface PersistedState {
	agents: PersistedAgent[];
	agentSeats: Record<number, { palette: number; hueShift: number; seatId: string | null }>;
	soundEnabled: boolean;
}

export interface SessionInfo {
	sessionId: string;
	jsonlFile: string;
	lastModified: number;
	projectPath: string;
	isTracked: boolean;
}

export interface ProjectSessions {
	dirName: string;
	projectPath: string;
	sessions: SessionInfo[];
}
