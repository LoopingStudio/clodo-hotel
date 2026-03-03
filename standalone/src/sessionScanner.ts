import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { SESSION_ACTIVE_WINDOW_MS } from './constants.js';
import type { ProjectSessions, SessionInfo } from './types.js';

const CLAUDE_PROJECTS_DIR = path.join(os.homedir(), '.claude', 'projects');

/**
 * Extract the project path (cwd) from a JSONL file by reading its first record.
 */
function extractProjectPath(jsonlFile: string): string | null {
	try {
		const fd = fs.openSync(jsonlFile, 'r');
		const buf = Buffer.alloc(8192);
		const bytesRead = fs.readSync(fd, buf, 0, buf.length, 0);
		fs.closeSync(fd);

		const text = buf.slice(0, bytesRead).toString('utf-8');
		for (const line of text.split('\n')) {
			if (!line.trim()) continue;
			try {
				const record = JSON.parse(line) as Record<string, unknown>;
				if (typeof record.cwd === 'string' && record.cwd.length > 0) return record.cwd;
			} catch { /* skip malformed lines */ }
		}
		return null;
	} catch {
		return null;
	}
}

/**
 * Scan ~/.claude/projects/ for Claude sessions active in the last SESSION_ACTIVE_WINDOW_MS.
 * Groups sessions by project path (extracted from JSONL cwd field).
 */
export function scanSessions(trackedJsonlFiles: Set<string>): ProjectSessions[] {
	const projectMap = new Map<string, ProjectSessions>();
	const now = Date.now();

	let dirs: string[];
	try {
		dirs = fs.readdirSync(CLAUDE_PROJECTS_DIR);
	} catch {
		return [];
	}

	for (const dirName of dirs) {
		const dirPath = path.join(CLAUDE_PROJECTS_DIR, dirName);
		let jsonlFiles: string[];
		try {
			const stat = fs.statSync(dirPath);
			if (!stat.isDirectory()) continue;
			jsonlFiles = fs.readdirSync(dirPath).filter(f => f.endsWith('.jsonl'));
		} catch {
			continue;
		}

		for (const jsonlName of jsonlFiles) {
			const jsonlFile = path.join(dirPath, jsonlName);
			let lastModified: number;
			try {
				lastModified = fs.statSync(jsonlFile).mtimeMs;
			} catch {
				continue;
			}

			if (now - lastModified > SESSION_ACTIVE_WINDOW_MS) continue;

			const sessionId = jsonlName.replace('.jsonl', '');
			const projectPath = extractProjectPath(jsonlFile) ?? `/${dirName.replace(/-/g, '/')}`;

			const session: SessionInfo = {
				sessionId,
				jsonlFile,
				lastModified,
				projectPath,
				isTracked: trackedJsonlFiles.has(jsonlFile),
			};

			if (!projectMap.has(projectPath)) {
				projectMap.set(projectPath, {
					dirName: path.basename(projectPath),
					projectPath,
					sessions: [],
				});
			}
			projectMap.get(projectPath)!.sessions.push(session);
		}
	}

	// Sort sessions within each project by most recent first
	for (const project of projectMap.values()) {
		project.sessions.sort((a, b) => b.lastModified - a.lastModified);
	}

	// Sort projects by most recently active
	return Array.from(projectMap.values()).sort((a, b) => {
		const aLatest = a.sessions[0]?.lastModified ?? 0;
		const bLatest = b.sessions[0]?.lastModified ?? 0;
		return bLatest - aLatest;
	});
}

/**
 * Find all sessions modified within the last SESSION_AUTO_ADD_WINDOW_MS (1h).
 * Used for auto-adding agents on startup.
 */
export function findRecentSessions(
	windowMs: number,
	trackedJsonlFiles: Set<string>,
): SessionInfo[] {
	const now = Date.now();
	return scanSessions(trackedJsonlFiles)
		.flatMap(p => p.sessions)
		.filter(s => now - s.lastModified <= windowMs && !s.isTracked);
}
