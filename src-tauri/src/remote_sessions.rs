//! Sessions of other computers paired through the Relay, listed with this computer's own sessions.
//!
//! While the app runs, every few seconds each paired computer is asked (through its Node and the Relay) what its sessions
//! are doing, what was said in them and what they wait for. They become ordinary entries of the session list, with a
//! `remote` origin: the computer's name in front of the project, and only the controls that computer allows (a prompt, a
//! stop, allowing a permission once). Prompts, approvals and stops for them go back through the Relay.

use std::{collections::HashMap, path::Path, thread, time::Duration};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    domain::{AgentKind, AgentSession, PermissionAction, PermissionProfile, PermissionRequest, RemoteOrigin, SessionActivity, SessionControlOrigin, SessionSource, SessionStatus},
    node_relay, node_service, protocol,
    state::AppState,
};

const PREFIX: &str = "remote:";
const REFRESH: Duration = Duration::from_secs(6);
/// The conversations read per computer and refresh: the most recently active sessions.
const CONVERSATIONS: usize = 8;

/// `remote:<node>:<session>` → the computer and its own session id.
pub fn parse(session_id: &str) -> Option<(&str, &str)> {
    session_id.strip_prefix(PREFIX)?.split_once(':')
}

pub fn is_remote(session_id: &str) -> bool {
    session_id.starts_with(PREFIX)
}

fn request_id() -> String {
    format!("{:x}{:x}", crate::state::now_millis(), std::process::id()) + &format!("{:08x}", rand_part())
}

fn rand_part() -> u32 {
    let mut bytes = [0u8; 4];
    let _ = ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes);
    u32::from_le_bytes(bytes)
}

fn directory() -> Result<std::path::PathBuf, String> {
    node_service::default_state_directory()
}

/// Sends a prompt to a session of another computer.
pub fn prompt(session_id: &str, prompt: &str) -> Result<(), String> {
    let (node, session) = parse(session_id).ok_or("Not a session of another computer")?;
    node_relay::send_prompt(&directory()?, node, session, &request_id(), prompt).map(|_| ())
}

/// Allows once or denies what a session of another computer waits for.
pub fn resolve_permission(session_id: &str, permission_id: &str, action: PermissionAction) -> Result<(), String> {
    let (node, session) = parse(session_id).ok_or("Not a session of another computer")?;
    let action = match action {
        PermissionAction::AllowOnce => "allow_once",
        PermissionAction::Deny => "deny",
        _ => return Err("Only allowing once or denying can be done from another computer".into()),
    };
    node_relay::answer_permission(&directory()?, node, session, permission_id, action, &request_id()).map(|_| ())
}

/// Stops the turn a session of another computer is running.
pub fn interrupt(session_id: &str) -> Result<(), String> {
    let (node, session) = parse(session_id).ok_or("Not a session of another computer")?;
    node_relay::interrupt_session(&directory()?, node, session, &request_id()).map(|_| ())
}

fn conversation_activities(conversation: &Value) -> Vec<SessionActivity> {
    conversation["messages"].as_array().into_iter().flatten().filter_map(|message| {
        let user = message["role"] == "user";
        Some(SessionActivity {
            id: format!("remote-{}", message["id"].as_str()?),
            kind: if user { "prompt" } else { "message" }.into(),
            title: String::new(),
            detail: message["text"].as_str().map(str::to_string),
            status: "completed".into(),
            created_at: message["createdAt"].as_i64().unwrap_or_default(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        })
    }).collect()
}

fn remote_profile() -> PermissionProfile {
    PermissionProfile {
        mode: crate::domain::AccessMode::Custom,
        label: "Remote".into(),
        approval_policy: "on-request".into(),
        approvals_reviewer: None,
        can_respond_from_lume: true,
        available_actions: vec![PermissionAction::AllowOnce, PermissionAction::Deny],
    }
}

/// One entry of the session list for a session of another computer.
fn session_of(node_id: &str, computer: &str, summary: &Value, conversation: Option<&Value>, permission: Option<&Value>) -> Option<AgentSession> {
    let id = summary["id"].as_str()?;
    let agent: AgentKind = serde_json::from_value(summary["agent"].clone()).unwrap_or(AgentKind::Unknown);
    let status: SessionStatus = serde_json::from_value(summary["status"].clone()).unwrap_or(SessionStatus::WaitingForInput);
    let project = summary["project"].as_str().unwrap_or_default();
    let pending_permission = permission.and_then(|value| {
        Some(PermissionRequest {
            id: value["id"].as_str()?.to_string(),
            kind: value["kind"].as_str().unwrap_or("command").to_string(),
            summary: value["summary"].as_str().unwrap_or_default().to_string(),
            resource: String::new(),
            risk: value["risk"].as_str().unwrap_or("medium").to_string(),
            requested_at: value["requestedAt"].as_str().unwrap_or_default().to_string(),
        })
    });
    let mut activities = conversation.map(conversation_activities).unwrap_or_default();
    activities.sort_by_key(|activity| activity.created_at);
    Some(AgentSession {
        id: format!("{PREFIX}{node_id}:{id}"),
        agent,
        agent_label: summary["agentLabel"].as_str().unwrap_or("Agent").to_string(),
        session_name: summary["name"].as_str().unwrap_or_default().to_string(),
        project: if project.is_empty() { computer.to_string() } else { format!("{computer} · {project}") },
        source: SessionSource::Desktop,
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status,
        status_label: summary["statusLabel"].as_str().unwrap_or_default().to_string(),
        started_at: summary["startedAt"].as_str().unwrap_or_default().to_string(),
        updated_at: summary["updatedAt"].as_i64().unwrap_or_default(),
        process_id: None,
        native_session_id: None,
        working_directory: None,
        permission_profile: remote_profile(),
        pending_permission,
        pending_question: None,
        last_response: None,
        results: Vec::new(),
        activities,
        rate_limits: Vec::new(),
        rate_limits_error: None,
        prompt_token_usage: Vec::new(),
        forked_from: None,
        remote: Some(RemoteOrigin {
            computer_id: node_id.to_string(),
            computer: computer.to_string(),
            can_prompt: summary["canPrompt"].as_bool().unwrap_or(false),
            can_interrupt: summary["canInterrupt"].as_bool().unwrap_or(false),
        }),
    })
}

/// Everything one computer shows: its sessions, the conversations of the most recent ones and what they wait for.
/// Errors from a computer that is offline or forbids a part are left out, never shown as sessions.
fn computer_sessions(directory: &Path, node_id: &str) -> Result<Vec<AgentSession>, String> {
    let mut answers = node_relay::query_many(directory, node_id, &["health", "sessions"])?.into_iter();
    let health = answers.next().ok_or("the Node did not answer")??;
    let snapshot = answers.next().ok_or("the Node did not answer")??;
    let computer = health["displayName"].as_str().unwrap_or(node_id).to_string();
    let mut summaries = snapshot["sessions"].as_array().cloned().unwrap_or_default();
    summaries.sort_by_key(|summary| std::cmp::Reverse(summary["updatedAt"].as_i64().unwrap_or_default()));
    // One more connection for the details, asking all of them at once.
    let mut commands = Vec::new();
    let mut wanted: Vec<(String, &'static str)> = Vec::new();
    for summary in summaries.iter().take(CONVERSATIONS) {
        let Some(id) = summary["id"].as_str() else { continue };
        commands.push(json!({ "cmd": "session", "id": id }));
        wanted.push((id.to_string(), "session"));
        if summary["needsPermission"] == true {
            commands.push(json!({ "cmd": "permission", "id": id }));
            wanted.push((id.to_string(), "permission"));
        }
    }
    let mut conversations: HashMap<String, Value> = HashMap::new();
    let mut permissions: HashMap<String, Value> = HashMap::new();
    if !commands.is_empty() {
        if let Ok(results) = node_relay::query_commands(directory, node_id, &commands) {
            for ((id, kind), result) in wanted.into_iter().zip(results) {
                let Ok(value) = result else { continue };
                if kind == "session" {
                    conversations.insert(id, value);
                } else if !value["permission"].is_null() {
                    permissions.insert(id, value["permission"].clone());
                }
            }
        }
    }
    Ok(summaries
        .iter()
        .filter_map(|summary| {
            let id = summary["id"].as_str()?;
            session_of(node_id, &computer, summary, conversations.get(id), permissions.get(id))
        })
        .collect())
}

/// Keeps the remote sessions current while the app runs.
pub fn start(state: AppState, app: AppHandle) {
    let _ = thread::Builder::new().name("lume-remote-sessions".into()).spawn(move || loop {
        let sessions = directory()
            .and_then(|directory| {
                let remotes = node_relay::remotes(&directory)?;
                Ok(remotes
                    .iter()
                    .flat_map(|remote| computer_sessions(&directory, &remote.node_id).unwrap_or_default())
                    .collect::<Vec<_>>())
            })
            .unwrap_or_default();
        if state.replace_remote_sessions(sessions).unwrap_or(false) {
            protocol::emit_sessions_changed(&app);
        }
        thread::sleep(REFRESH);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_session_carries_its_computer_and_only_the_allowed_controls() {
        let summary = json!({ "id": "s1", "agent": "codex", "agentLabel": "Codex", "name": "Cart", "project": "shop", "status": "permission_required",
            "statusLabel": "Needs permission", "startedAt": "1", "updatedAt": 5, "needsPermission": true, "needsAnswer": false, "canPrompt": true, "canInterrupt": true });
        let conversation = json!({ "messages": [{ "id": "m2", "role": "assistant", "text": "Done.", "createdAt": 2 }, { "id": "m1", "role": "user", "text": "Fix it", "createdAt": 1 }] });
        let permission = json!({ "id": "perm", "kind": "command", "summary": "npm test", "risk": "low", "requestedAt": "now" });
        let session = session_of("node-1", "Home PC", &summary, Some(&conversation), Some(&permission)).unwrap();
        assert_eq!(session.id, "remote:node-1:s1");
        assert_eq!(parse(&session.id), Some(("node-1", "s1")));
        assert_eq!(session.project, "Home PC · shop");
        assert_eq!(session.activities.iter().map(|activity| activity.kind.as_str()).collect::<Vec<_>>(), ["prompt", "message"]);
        let capabilities = protocol::SessionCapabilities::for_session(&session);
        assert!(capabilities.can_prompt && capabilities.can_interrupt && capabilities.can_approve);
        assert!(!capabilities.can_terminate && !capabilities.can_attach_images && !capabilities.can_take_control);
        let blocked = session_of("node-1", "Home PC", &json!({ "id": "s2", "agent": "claude_code", "status": "running" }), None, None).unwrap();
        let capabilities = protocol::SessionCapabilities::for_session(&blocked);
        assert!(!capabilities.can_prompt && !capabilities.can_interrupt && !capabilities.can_approve, "an older Node says nothing, so nothing is offered");
    }
}
