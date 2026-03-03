use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::SESSION_ACTIVE_WINDOW_MS;
use crate::types::{ProjectSessions, SessionInfo};

pub fn claude_projects_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".claude")
        .join("projects")
}

/// Read up to 8 KB from the start of a JSONL file and extract the `cwd` field
/// from any record that has it.
fn extract_project_path(jsonl_file: &Path) -> Option<String> {
    use std::fs::File;
    use std::io::{Read, BufReader};
    let f = File::open(jsonl_file).ok()?;
    let mut reader = BufReader::new(f);
    let mut buf = [0u8; 8192];
    let n = reader.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..n]);
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(cwd) = record.get("cwd").and_then(|v| v.as_str()) {
                if !cwd.is_empty() {
                    return Some(cwd.to_string());
                }
            }
        }
    }
    None
}

fn mtime_ms(path: &Path) -> Option<u128> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    Some(
        mtime
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Scan ~/.claude/projects/ for sessions active within SESSION_ACTIVE_WINDOW_MS.
/// Groups by project path (cwd extracted from JSONL).
pub fn scan_sessions(tracked_jsonl_files: &std::collections::HashSet<String>) -> Vec<ProjectSessions> {
    let projects_dir = claude_projects_dir();
    let mut project_map: HashMap<String, ProjectSessions> = HashMap::new();
    let now = now_ms();

    let dirs = match std::fs::read_dir(&projects_dir) {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    for entry in dirs.flatten() {
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }

        let jsonl_files: Vec<PathBuf> = match std::fs::read_dir(&dir_path) {
            Ok(inner) => inner
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
                .collect(),
            Err(_) => continue,
        };

        for jsonl_path in jsonl_files {
            let last_modified = match mtime_ms(&jsonl_path) {
                Some(m) => m,
                None => continue,
            };

            if now.saturating_sub(last_modified) > SESSION_ACTIVE_WINDOW_MS {
                continue;
            }

            let session_id = jsonl_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            let jsonl_file_str = jsonl_path.to_string_lossy().to_string();

            let project_path = extract_project_path(&jsonl_path).unwrap_or_else(|| {
                // fallback: reconstruct from directory hash
                let hash = dir_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .replace('-', "/");
                format!("/{}", hash)
            });

            let is_tracked = tracked_jsonl_files.contains(&jsonl_file_str);

            let session = SessionInfo {
                session_id,
                jsonl_file: jsonl_file_str,
                last_modified: last_modified as u64,
                project_path: project_path.clone(),
                is_tracked,
            };

            let entry = project_map
                .entry(project_path.clone())
                .or_insert_with(|| ProjectSessions {
                    dir_name: Path::new(&project_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&project_path)
                        .to_string(),
                    project_path: project_path.clone(),
                    sessions: vec![],
                });
            entry.sessions.push(session);
        }
    }

    // Sort sessions within each project by most recent first
    for project in project_map.values_mut() {
        project.sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    }

    // Sort projects by most recently active
    let mut projects: Vec<ProjectSessions> = project_map.into_values().collect();
    projects.sort_by(|a, b| {
        let a_latest = a.sessions.first().map(|s| s.last_modified).unwrap_or(0);
        let b_latest = b.sessions.first().map(|s| s.last_modified).unwrap_or(0);
        b_latest.cmp(&a_latest)
    });
    projects
}

/// Find sessions modified within `window_ms` that are not yet tracked.
pub fn find_recent_sessions(
    window_ms: u128,
    tracked_jsonl_files: &std::collections::HashSet<String>,
) -> Vec<SessionInfo> {
    let now = now_ms();
    scan_sessions(tracked_jsonl_files)
        .into_iter()
        .flat_map(|p| p.sessions)
        .filter(|s| {
            now.saturating_sub(s.last_modified as u128) <= window_ms && !s.is_tracked
        })
        .collect()
}

