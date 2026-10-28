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

pub const MAX_SESSIONS: usize = 200;
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
    /// True when the Lume app answered with what its sessions are doing now; false for the last state it saved to disk,
    /// which does not know what a session is doing or waiting for.
    #[serde(default)]
    pub live: bool,
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
        return Ok(SessionsSnapshot { generated_at: now, app_found: false, live: false, sessions: Vec::new() });
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
    Ok(SessionsSnapshot { generated_at: now, app_found: true, live: false, sessions })
}

const MAX_MESSAGES: usize = 80;
const MAX_MESSAGE_CHARS: usize = 6_000;
/// A Relay frame holds 256 KiB of ciphertext; stay well below it once base64 and the envelope are added.
const MAX_TRANSCRIPT_BYTES: usize = 120 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub id: String,
    /// `user` or `assistant`.
    pub role: String,
    pub text: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub session: SessionSummary,
    pub generated_at: i64,
    /// True when older messages, or the tail of a very long one, were left out.
    pub truncated: bool,
    pub messages: Vec<ConversationMessage>,
}

fn clip(text: &str) -> (String, bool) {
    let text = text.trim();
    if text.chars().count() <= MAX_MESSAGE_CHARS {
        return (text.to_string(), false);
    }
    (format!("{}…", text.chars().take(MAX_MESSAGE_CHARS).collect::<String>()), true)
}

/// What was said in a session: the person's prompts and the agent's messages, in order. Commands, file
/// diffs, tool output and attachments are left out; they say far more about the machine than the chat does.
pub fn conversation_of(session: &AgentSession, now: i64) -> Conversation {
    let mut activities: Vec<_> = session.activities.iter().filter(|activity| matches!(activity.kind.as_str(), "prompt" | "message")).collect();
    activities.sort_by_key(|activity| activity.created_at);
    let mut messages: Vec<ConversationMessage> = activities
        .iter()
        .filter_map(|activity| {
            let (text, _) = clip(activity.detail.as_deref().unwrap_or_default());
            // Claude Code records its own notices (a finished background command, a reminder) as prompts.
            let said_by_a_person = activity.kind != "prompt" || crate::adapters::visible_claude_user_text(&text);
            (!text.is_empty() && said_by_a_person).then(|| ConversationMessage {
                id: activity.id.clone(),
                role: if activity.kind == "prompt" { "user" } else { "assistant" }.into(),
                text,
                created_at: activity.created_at,
            })
        })
        .collect();
    if !messages.iter().any(|message| message.role == "assistant") {
        // Some agents only record their final answers as results.
        for result in &session.results {
            let (text, _) = clip(&result.response);
            if !text.is_empty() {
                messages.push(ConversationMessage { id: result.id.clone(), role: "assistant".into(), text, created_at: result.created_at });
            }
        }
        messages.sort_by_key(|message| message.created_at);
    }
    let mut truncated = false;
    if messages.len() > MAX_MESSAGES {
        messages.drain(..messages.len() - MAX_MESSAGES);
        truncated = true;
    }
    let mut size: usize = messages.iter().map(|message| message.text.len() + message.id.len() + 64).sum();
    while size > MAX_TRANSCRIPT_BYTES && messages.len() > 1 {
        let dropped = messages.remove(0);
        size -= dropped.text.len() + dropped.id.len() + 64;
        truncated = true;
    }
    Conversation { session: summarize(session), generated_at: now, truncated, messages }
}

pub fn read_conversation(database: &Path, session_id: &str, now: i64) -> Result<Conversation, String> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|error| error.to_string())?;
    connection.busy_timeout(std::time::Duration::from_secs(2)).map_err(|error| error.to_string())?;
    let payload: String = connection
        .query_row("SELECT payload FROM managed_sessions WHERE session_id = ?1", [session_id], |row| row.get(0))
        .map_err(|_| "no such session".to_string())?;
    let session = serde_json::from_str::<AgentSession>(&payload).map_err(|_| "the session could not be read".to_string())?;
    Ok(conversation_of(&session, now))
}

pub fn conversation(session_id: &str) -> Result<Conversation, String> {
    if session_id.is_empty() || session_id.len() > 256 {
        return Err("a session id is required".into());
    }
    let path = app_database_path().filter(|path| path.is_file()).ok_or("the Lume app has not been opened on this computer yet")?;
    read_conversation(&path, session_id, now_millis())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as i64).unwrap_or_default()
}

pub fn snapshot() -> Result<SessionsSnapshot, String> {
    let now = now_millis();
    match app_database_path() {
        Some(path) => read(&path, now),
        None => Ok(SessionsSnapshot { generated_at: now, app_found: false, live: false, sessions: Vec::new() }),
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
        connection.execute_batch("CREATE TABLE managed_sessions (session_id TEXT PRIMARY KEY, payload TEXT NOT NULL, updated_at INTEGER NOT NULL);").unwrap();
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

    fn activity(id: &str, kind: &str, detail: &str, at: i64) -> serde_json::Value {
        serde_json::json!({ "id": id, "kind": kind, "title": "t", "detail": detail, "status": "completed", "createdAt": at, "files": [] })
    }

    fn session_with(activities: Vec<serde_json::Value>, results: Vec<serde_json::Value>) -> AgentSession {
        serde_json::from_value(serde_json::json!({
            "id": "s1", "agent": "codex", "agentLabel": "Codex", "sessionName": "Cart", "project": "/home/ana/secret/app",
            "source": "cli", "status": "running", "statusLabel": "Running", "startedAt": "1", "updatedAt": 5,
            "processId": null, "nativeSessionId": null, "workingDirectory": "/home/ana/secret/app",
            "permissionProfile": { "mode": "read_only", "label": "Read only", "approvalPolicy": "on-request", "canRespondFromLume": true, "availableActions": [] },
            "activities": activities, "results": results
        })).unwrap()
    }

    #[test]
    fn the_conversation_has_only_what_was_said() {
        let session = session_with(vec![
            activity("m2", "message", "Done: the total is fixed.", 30),
            activity("p1", "prompt", "Fix the cart total", 10),
            activity("c1", "command", "cat ~/.ssh/id_rsa", 20),
            activity("n1", "prompt", "<task-notification>done</task-notification>", 15),
            activity("f1", "file", "diff --git a/secret.env b/secret.env", 25),
        ], vec![]);
        let conversation = conversation_of(&session, 99);
        let roles: Vec<_> = conversation.messages.iter().map(|message| (message.role.as_str(), message.text.as_str())).collect();
        assert_eq!(roles, [("user", "Fix the cart total"), ("assistant", "Done: the total is fixed.")]);
        let text = serde_json::to_string(&conversation).unwrap();
        for private in ["id_rsa", "secret.env", "/home/ana/secret"] {
            assert!(!text.contains(private), "{private} leaked");
        }
        assert!(!conversation.truncated);
    }

    #[test]
    fn answers_recorded_only_as_results_still_show() {
        let session = session_with(vec![activity("p1", "prompt", "Hi", 1)], vec![serde_json::json!({ "id": "r1", "response": "Hello!", "createdAt": 2 })]);
        let conversation = conversation_of(&session, 1);
        assert_eq!(conversation.messages.iter().map(|message| message.role.as_str()).collect::<Vec<_>>(), ["user", "assistant"]);
    }

    #[test]
    fn long_conversations_are_cut_from_the_oldest_end_and_stay_under_a_frame() {
        let activities = (0..300).map(|index| activity(&format!("m{index}"), "message", &"x".repeat(5_000), index)).collect();
        let conversation = conversation_of(&session_with(activities, vec![]), 1);
        assert!(conversation.truncated);
        assert!(conversation.messages.len() <= MAX_MESSAGES);
        assert!(serde_json::to_vec(&conversation).unwrap().len() < MAX_TRANSCRIPT_BYTES + 8 * 1024);
        assert_eq!(conversation.messages.last().unwrap().id, "m299", "the newest messages are kept");
        let huge = conversation_of(&session_with(vec![activity("m", "message", &"y".repeat(50_000), 1)], vec![]), 1);
        assert!(huge.messages[0].text.chars().count() <= MAX_MESSAGE_CHARS + 1);
    }

    #[test]
    fn reads_one_session_from_the_database() {
        let path = std::env::temp_dir().join(format!("lume-conversation-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let session = serde_json::to_string(&session_with(vec![activity("p1", "prompt", "Hi", 1)], vec![])).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE managed_sessions (session_id TEXT PRIMARY KEY, payload TEXT NOT NULL, updated_at INTEGER NOT NULL);").unwrap();
        connection.execute("INSERT INTO managed_sessions VALUES ('s1', ?1, 5)", [session]).unwrap();
        drop(connection);
        assert_eq!(read_conversation(&path, "s1", 1).unwrap().messages.len(), 1);
        assert!(read_conversation(&path, "missing", 1).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
