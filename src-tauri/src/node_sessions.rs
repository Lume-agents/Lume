//! A read-only view of the agent sessions the Lume app keeps on this computer.
//!
//! The Node answers `sessions` with a summary that is safe to show on a paired
//! device with the `observe` scope: which agents exist, their project name and
//! status. It never includes the conversation, the working directory, command
//! lines or the permission details; those need a later, explicit scope.

use std::env;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::domain::AgentSession;

const MAX_SESSIONS: usize = 200;
const APP_IDENTIFIER: &str = "com.tulerws.lume";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub agent: String,
    pub agent_label: String,
    pub name: String,
    pub project: String,
    pub status: String,
    pub status_label: String,
    pub started_at: String,
    pub updated_at: i64,
    pub needs_permission: bool,
    pub needs_answer: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsSnapshot {
    pub generated_at: i64,
    /// False when the Lume app has not created its database on this computer yet.
    pub app_found: bool,
    pub sessions: Vec<SessionSummary>,
}

/// The enum variants serialize to their snake_case names, which is what a device wants.
fn tag<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|value| value.as_str().map(str::to_string)).unwrap_or_default()
}

/// Only the last folder of a project path: the rest says too much about this computer.
fn project_name(project: &str) -> String {
    project.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default().to_string()
}

pub fn summarize(session: &AgentSession) -> SessionSummary {
    SessionSummary {
        id: session.id.clone(),
        agent: tag(&session.agent),
        agent_label: session.agent_label.clone(),
        name: session.session_name.clone(),
        project: project_name(&session.project),
        status: tag(&session.status),
        status_label: session.status_label.clone(),
        started_at: session.started_at.clone(),
        updated_at: session.updated_at,
        needs_permission: session.pending_permission.is_some(),
        needs_answer: session.pending_question.is_some(),
    }
}

/// Where the Lume app keeps its database (`app_data_dir` of the Tauri app).
pub fn app_database_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("LUME_APP_DB") {
        return Some(PathBuf::from(path));
    }
    #[cfg(target_os = "windows")]
    let base = env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = env::var_os("HOME").map(|home| PathBuf::from(home).join("Library").join("Application Support"));
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let base = env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share")));
    base.map(|base| base.join(APP_IDENTIFIER).join("lume.sqlite3"))
}

/// Reads the sessions without ever writing to (or migrating) the app's database.
pub fn read(database: &Path, now: i64) -> Result<SessionsSnapshot, String> {
    if !database.is_file() {
        return Ok(SessionsSnapshot { generated_at: now, app_found: false, sessions: Vec::new() });
    }
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|error| error.to_string())?;
    connection.busy_timeout(std::time::Duration::from_secs(2)).map_err(|error| error.to_string())?;
    let mut statement = connection.prepare("SELECT payload FROM managed_sessions ORDER BY updated_at DESC LIMIT ?1").map_err(|error| error.to_string())?;
    let rows = statement.query_map([MAX_SESSIONS as i64], |row| row.get::<_, String>(0)).map_err(|error| error.to_string())?;
    let mut sessions = Vec::new();
    for payload in rows {
        let payload = payload.map_err(|error| error.to_string())?;
        if let Ok(session) = serde_json::from_str::<AgentSession>(&payload) {
            sessions.push(summarize(&session));
        }
    }
    Ok(SessionsSnapshot { generated_at: now, app_found: true, sessions })
}

pub fn snapshot() -> Result<SessionsSnapshot, String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as i64).unwrap_or_default();
    match app_database_path() {
        Some(path) => read(&path, now),
        None => Ok(SessionsSnapshot { generated_at: now, app_found: false, sessions: Vec::new() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_last_folder_of_the_project() {
        assert_eq!(project_name("/home/ana/work/lume/"), "lume");
        assert_eq!(project_name("C:\\Users\\ana\\app"), "app");
        assert_eq!(project_name("solo"), "solo");
    }

    #[test]
    fn a_missing_database_is_reported_not_created() {
        let path = std::env::temp_dir().join(format!("lume-sessions-missing-{}.sqlite3", std::process::id()));
        let snapshot = read(&path, 1).unwrap();
        assert!(!snapshot.app_found && snapshot.sessions.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn reads_sessions_and_hides_the_conversation() {
        let path = std::env::temp_dir().join(format!("lume-sessions-read-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let session = serde_json::json!({
            "id": "s1", "agent": "claude_code", "agentLabel": "Claude Code", "sessionName": "Fix login",
            "project": "/home/ana/secret/app", "source": "cli", "status": "permission_required", "statusLabel": "Needs permission",
            "startedAt": "2026-10-08T10:00:00Z", "updatedAt": 5, "processId": null, "nativeSessionId": null,
            "workingDirectory": "/home/ana/secret/app", "permissionProfile": { "mode": "read_only", "label": "Read only", "approvalPolicy": "on-request", "canRespondFromLume": true, "availableActions": [] },
            "pendingPermission": { "id": "p", "kind": "shell", "summary": "rm -rf secrets", "resource": "x", "risk": "high", "requestedAt": "now" },
            "lastResponse": "the private answer"
        });
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE managed_sessions (id TEXT PRIMARY KEY, payload TEXT NOT NULL, updated_at INTEGER NOT NULL);").unwrap();
        connection.execute("INSERT INTO managed_sessions VALUES ('s1', ?1, 5)", [session.to_string()]).unwrap();
        drop(connection);
        let snapshot = read(&path, 9).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(snapshot.sessions.len(), 1, "the sample session must deserialize as an AgentSession");
        let item = &snapshot.sessions[0];
        assert_eq!((item.project.as_str(), item.status.as_str(), item.agent.as_str()), ("app", "permission_required", "claude_code"));
        assert!(item.needs_permission);
        let text = serde_json::to_string(&snapshot).unwrap();
        for private in ["secret", "rm -rf", "private answer"] {
            assert!(!text.contains(private), "{private} leaked");
        }
    }
}
