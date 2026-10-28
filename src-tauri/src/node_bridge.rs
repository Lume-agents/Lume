//! The link between the Node (`lume node run`, a separate process) and the Lume app on the same computer.
//!
//! The Node cannot run an agent itself: a prompt from another computer has to reach the app, which owns the sessions.
//! While the app runs it listens on a loopback port and writes that port and a random secret to `node-bridge.json`
//! in the Node's state folder (readable only by the user). The Node proves it holds the secret with an HMAC over
//! the request, so another program, or a web page that guesses the port, cannot send a prompt through it.
//!
//! Commands: `prompt` sends text to one of the app's own sessions, with the request id that makes a repeat harmless
//! (the app answers as it does for a phone); `sessions` and `session` read what the app's sessions are doing now;
//! `permission` reads what a session waits for the person to allow, and `approve` allows it once or denies it;
//! `interrupt` stops the turn a session is running.
//! The app's database only keeps a scrubbed copy of a session (no status, no pending question, no transcript), so
//! anything that needs the live state has to ask the running app.

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::{
    codex_bridge::CodexBridge,
    control,
    protocol::HubCommandRequest,
    state::AppState,
};

const FILE: &str = "node-bridge.json";
const DOMAIN: &str = "lume-node-bridge-v1";
const MAX_REQUEST_BYTES: u64 = 96 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// The longest a prompt may be (characters); the same bound for the Node and for the app.
pub const MAX_PROMPT_CHARS: usize = 32_000;

#[derive(Debug, Deserialize, Serialize)]
struct Stored {
    port: u16,
    secret: String,
}

fn io(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn path(directory: &Path) -> PathBuf {
    directory.join(FILE)
}

/// What the request's signature covers: the command, who it is for, the request id and a digest of the text.
fn signed_text(command: &str, session_id: &str, request_id: &str, prompt: &str) -> String {
    format!("{DOMAIN}\n{command}\n{session_id}\n{request_id}\n{}", hex(&Sha256::digest(prompt.as_bytes())))
}

fn sign(secret: &str, text: &str) -> String {
    hex(hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes()), text.as_bytes()).as_ref())
}

fn verify(secret: &str, text: &str, signature: &str) -> bool {
    let Some(bytes) = (0..signature.len()).step_by(2).map(|at| signature.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok())).collect::<Option<Vec<u8>>>() else {
        return false;
    };
    hmac::verify(&hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes()), text.as_bytes(), &bytes).is_ok()
}

fn write_private(file: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(file, bytes).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o600)).map_err(io)?;
    }
    Ok(())
}

// ── App side ─────────────────────────────────────────────────────────────────

/// Starts listening and publishes the port and the secret for the Node.
pub fn start(app: AppHandle, state: AppState, directory: &Path) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(io)?;
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).map_err(io)?;
    let port = listener.local_addr().map_err(io)?.port();
    let mut secret = [0u8; 32];
    SystemRandom::new().fill(&mut secret).map_err(|_| "no randomness for the Node bridge".to_string())?;
    let secret = hex(&secret);
    write_private(&path(directory), &serde_json::to_vec(&Stored { port, secret: secret.clone() }).map_err(io)?)?;
    thread::Builder::new()
        .name("lume-node-bridge".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let (app, state, secret) = (app.clone(), state.clone(), secret.clone());
                let _ = thread::Builder::new().name("lume-node-bridge-request".into()).spawn(move || {
                    let _ = serve(stream, &app, &state, &secret);
                });
            }
        })
        .map_err(io)?;
    Ok(())
}

fn serve(mut stream: TcpStream, app: &AppHandle, state: &AppState, secret: &str) -> Result<(), String> {
    stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(io)?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(io)?;
    let mut line = String::new();
    BufReader::new((&stream).take(MAX_REQUEST_BYTES)).read_line(&mut line).map_err(io)?;
    let answer = match handle(&line, app, state, secret) {
        Ok(value) => json!({ "ok": true, "result": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    stream.write_all(format!("{answer}\n").as_bytes()).map_err(io)
}

fn handle(line: &str, app: &AppHandle, state: &AppState, secret: &str) -> Result<Value, String> {
    let request = authenticated(line, secret)?;
    let command = match request["cmd"].as_str() {
        Some("sessions") => return live_sessions(state),
        Some("session") => return live_conversation(state, request["sessionId"].as_str().unwrap_or_default()),
        Some("permission") => return pending_permission(state, request["sessionId"].as_str().unwrap_or_default()),
        Some("approve") => approve_command(&request)?,
        Some("interrupt") => interrupt_command(&request)?,
        _ => prompt_command(&request)?,
    };
    let response = control::execute_hub_command(
        app,
        state,
        app.state::<CodexBridge>().inner(),
        app.state::<crate::browser_server::BrowserControl>().inner(),
        app.state::<crate::workflow_runtime::WorkflowRuntime>().inner(),
        command,
    );
    if response.ok {
        Ok(json!({ "accepted": true }))
    } else {
        Err(response.error.map(|error| error.message).unwrap_or_else(|| "The Lume app refused the request".into()))
    }
}

/// What every session of the app is doing now, as the summaries a device may see.
fn live_sessions(state: &AppState) -> Result<Value, String> {
    let mut sessions = state.map_sessions(crate::node_sessions::summarize)?;
    sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
    sessions.truncate(crate::node_sessions::MAX_SESSIONS);
    let snapshot = crate::node_sessions::SessionsSnapshot { generated_at: now_millis(), app_found: true, live: true, sessions };
    serde_json::to_value(snapshot).map_err(io)
}

fn live_conversation(state: &AppState, session_id: &str) -> Result<Value, String> {
    if session_id.is_empty() || session_id.len() > 512 {
        return Err("a session id is required".into());
    }
    let conversation = state.with_session(session_id, |session| crate::node_sessions::conversation_of(session, now_millis()))?;
    serde_json::to_value(conversation).map_err(io)
}

/// What a session waits for the person to allow, or `null` when it waits for nothing.
fn pending_permission(state: &AppState, session_id: &str) -> Result<Value, String> {
    if session_id.is_empty() || session_id.len() > 512 {
        return Err("a session id is required".into());
    }
    let permission = state.with_session(session_id, |session| session.pending_permission.clone())?;
    Ok(json!({ "permission": permission.map(|permission| json!({
        "id": permission.id,
        "kind": permission.kind,
        "summary": permission.summary,
        "risk": permission.risk,
        "requestedAt": permission.requested_at,
    })) }))
}

/// Stops the turn a session is running; the session itself stays open.
fn interrupt_command(request: &Value) -> Result<HubCommandRequest, String> {
    serde_json::from_value(json!({
        "requestId": request["requestId"],
        "type": "interrupt_prompt",
        "sessionId": request["sessionId"],
    }))
    .map_err(|error: serde_json::Error| error.to_string())
}

/// An approval travels in the signed text slot as `permission id`, a newline and the answer.
fn approve_command(request: &Value) -> Result<HubCommandRequest, String> {
    let body = request["prompt"].as_str().unwrap_or_default();
    let (permission_id, action) = body.split_once('\n').ok_or("an approval needs a permission and an answer")?;
    if !matches!(action, "allow_once" | "deny") {
        return Err("the answer must be allow_once or deny".into());
    }
    serde_json::from_value(json!({
        "requestId": request["requestId"],
        "type": "resolve_permission",
        "sessionId": request["sessionId"],
        "permissionId": permission_id,
        "action": action,
    }))
    .map_err(|error: serde_json::Error| error.to_string())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_millis() as i64).unwrap_or(0)
}

/// The request, once its signature checks out. A wrong or missing signature never reveals why.
fn authenticated(line: &str, secret: &str) -> Result<Value, String> {
    let request: Value = serde_json::from_str(line.trim()).map_err(|_| "unreadable request".to_string())?;
    let field = |name: &str| request[name].as_str().unwrap_or_default();
    let text = signed_text(field("cmd"), field("sessionId"), field("requestId"), field("prompt"));
    if !verify(secret, &text, field("mac")) {
        return Err("not authorized".into());
    }
    Ok(request)
}

fn prompt_command(request: &Value) -> Result<HubCommandRequest, String> {
    if request["cmd"] != "prompt" {
        return Err("unsupported command".into());
    }
    let prompt = request["prompt"].as_str().unwrap_or_default();
    if prompt.trim().is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err("the prompt is empty or too long".into());
    }
    serde_json::from_value(json!({
        "requestId": request["requestId"],
        "type": "submit_prompt",
        "sessionId": request["sessionId"],
        "prompt": prompt,
    }))
    .map_err(|error: serde_json::Error| error.to_string())
}

// ── Node side ────────────────────────────────────────────────────────────────

const APP_CLOSED: &str = "the Lume app is not open on that computer";

/// True for the error a Node gets when no app answers, as opposed to an app that answered with a refusal.
pub fn is_app_closed(error: &str) -> bool {
    error == APP_CLOSED
}

fn random_id() -> String {
    let mut bytes = [0u8; 12];
    let _ = SystemRandom::new().fill(&mut bytes);
    hex(&bytes)
}

/// What the running app says its sessions are doing now. `Err` when the app is not open on this computer.
pub fn live_sessions_snapshot(directory: &Path) -> Result<Value, String> {
    call(directory, "sessions", "", &random_id(), "")
}

/// What was said in one session of the running app.
pub fn live_session_conversation(directory: &Path, session_id: &str) -> Result<Value, String> {
    call(directory, "session", session_id, &random_id(), "")
}

/// What a session of the running app waits for the person to allow.
pub fn live_permission(directory: &Path, session_id: &str) -> Result<Value, String> {
    call(directory, "permission", session_id, &random_id(), "")
}

/// Allows once or denies a session's pending permission in the running app.
pub fn answer_permission(directory: &Path, session_id: &str, request_id: &str, permission_id: &str, action: &str) -> Result<Value, String> {
    call(directory, "approve", session_id, request_id, &format!("{permission_id}\n{action}"))
}

/// Stops the turn a session of the running app is running.
pub fn interrupt_session(directory: &Path, session_id: &str, request_id: &str) -> Result<Value, String> {
    call(directory, "interrupt", session_id, request_id, "")
}

/// Sends a prompt to the running Lume app on this computer. `Err` when the app is not open.
pub fn send_prompt(directory: &Path, session_id: &str, request_id: &str, prompt: &str) -> Result<Value, String> {
    call(directory, "prompt", session_id, request_id, prompt)
}

fn call(directory: &Path, command: &str, session_id: &str, request_id: &str, prompt: &str) -> Result<Value, String> {
    let stored: Stored = serde_json::from_slice(&fs::read(path(directory)).map_err(|_| APP_CLOSED.to_string())?)
        .map_err(|_| APP_CLOSED.to_string())?;
    let mut stream = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, stored.port)), IO_TIMEOUT)
        .map_err(|_| APP_CLOSED.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).map_err(io)?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(io)?;
    let mac = sign(&stored.secret, &signed_text(command, session_id, request_id, prompt));
    let request = json!({ "cmd": command, "sessionId": session_id, "requestId": request_id, "prompt": prompt, "mac": mac });
    stream.write_all(format!("{request}\n").as_bytes()).map_err(io)?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).map_err(|_| "the Lume app did not answer".to_string())?;
    let answer: Value = serde_json::from_str(line.trim()).map_err(|_| "the Lume app sent an unreadable answer".to_string())?;
    if answer["ok"] == true {
        Ok(answer["result"].clone())
    } else {
        Err(answer["error"].as_str().unwrap_or("the Lume app refused the prompt").to_string())
    }
}

/// Lets the Node's tests point at a stand-in for the app.
#[cfg(test)]
pub(crate) fn publish_for_tests(directory: &Path, port: u16, secret: &str) {
    fs::create_dir_all(directory).unwrap();
    write_private(&path(directory), &serde_json::to_vec(&Stored { port, secret: secret.into() }).unwrap()).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::HubCommand;

    #[test]
    fn a_signature_covers_the_command_the_session_the_request_and_the_text() {
        let secret = "s3cret";
        let signature = sign(secret, &signed_text("prompt", "s1", "r1", "hello"));
        assert!(verify(secret, &signed_text("prompt", "s1", "r1", "hello"), &signature));
        for tampered in [("prompt", "s2", "r1", "hello"), ("prompt", "s1", "r2", "hello"), ("prompt", "s1", "r1", "hello!"), ("other", "s1", "r1", "hello")] {
            assert!(!verify(secret, &signed_text(tampered.0, tampered.1, tampered.2, tampered.3), &signature), "{tampered:?}");
        }
        assert!(!verify("another", &signed_text("prompt", "s1", "r1", "hello"), &signature));
        assert!(!verify(secret, "x", "not hex"));
        assert!(!verify(secret, "x", ""));
    }

    #[test]
    fn only_a_signed_prompt_becomes_a_command() {
        let secret = "s3cret";
        let body = |mac: &str| json!({ "cmd": "prompt", "sessionId": "s1", "requestId": "r1", "prompt": "hello", "mac": mac }).to_string();
        let good = sign(secret, &signed_text("prompt", "s1", "r1", "hello"));
        let request = authenticated(&body(&good), secret).unwrap();
        let command = prompt_command(&request).unwrap();
        assert_eq!(command.request_id, "r1");
        assert!(matches!(command.command, HubCommand::SubmitPrompt { ref session_id, ref prompt, .. } if session_id == "s1" && prompt == "hello"));
        assert!(authenticated(&body("00"), secret).is_err());
        assert!(authenticated("not json", secret).is_err());
        let empty = json!({ "cmd": "prompt", "sessionId": "s1", "requestId": "r1", "prompt": "  " });
        assert!(prompt_command(&empty).is_err());
        let long = json!({ "cmd": "prompt", "sessionId": "s1", "requestId": "r1", "prompt": "x".repeat(MAX_PROMPT_CHARS + 1) });
        assert!(prompt_command(&long).is_err());
    }

    /// The Node's half against a stand-in for the app: the request arrives signed, and the app's answer comes back.
    #[test]
    fn the_node_reaches_the_app_with_a_request_the_app_can_verify() {
        let directory = std::env::temp_dir().join(format!("lume-bridge-present-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let secret = "app-secret";
        write_private(&path(&directory), &serde_json::to_vec(&Stored { port: listener.local_addr().unwrap().port(), secret: secret.into() }).unwrap()).unwrap();
        let app = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let verdict = authenticated(&line, secret).and_then(|request| prompt_command(&request)).map(|command| command.request_id);
            let answer = match verdict {
                Ok(request_id) => json!({ "ok": true, "result": { "accepted": true, "requestId": request_id } }),
                Err(error) => json!({ "ok": false, "error": error }),
            };
            stream.write_all(format!("{answer}\n").as_bytes()).unwrap();
        });
        let result = send_prompt(&directory, "s1", "request-0001", "hello").unwrap();
        app.join().unwrap();
        assert_eq!(result["requestId"], "request-0001");
        // A Node holding another secret is refused.
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        write_private(&path(&directory), &serde_json::to_vec(&Stored { port: listener.local_addr().unwrap().port(), secret: "other".into() }).unwrap()).unwrap();
        let app = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let answer = match authenticated(&line, secret) {
                Ok(_) => json!({ "ok": true, "result": {} }),
                Err(error) => json!({ "ok": false, "error": error }),
            };
            stream.write_all(format!("{answer}\n").as_bytes()).unwrap();
        });
        assert_eq!(send_prompt(&directory, "s1", "request-0002", "hello").unwrap_err(), "not authorized");
        app.join().unwrap();
        let _ = fs::remove_dir_all(&directory);
    }

    /// The running app knows what its sessions are doing and saying; the copy in its database does not.
    #[test]
    fn the_live_view_has_the_status_the_permission_flag_and_the_conversation() {
        let state = AppState::new(Path::new(":memory:")).expect("state");
        let event = |kind: &str, session: &str, extra: Value| -> crate::domain::HookEvent {
            let mut value = json!({
                "event": kind, "sessionId": session, "agent": "claude_code", "agentLabel": "Claude Code", "sessionName": "Cart",
                "project": "/home/ana/secret/shop", "source": "cli", "statusLabel": "Working", "startedAt": null, "processId": null,
                "nativeSessionId": null, "workingDirectory": "/home/ana/secret/shop", "permissionProfile": null, "permission": null,
                "activities": [], "waitForDecision": false
            });
            for (key, item) in extra.as_object().unwrap() {
                value[key] = item.clone();
            }
            serde_json::from_value(value).expect("hook event")
        };
        let activity = |id: &str, kind: &str, detail: &str, at: i64| json!({ "id": id, "kind": kind, "title": "t", "detail": detail, "status": "completed", "createdAt": at, "files": [] });
        state
            .ingest(event("session_started", "live-1", json!({ "activities": [activity("p1", "prompt", "Fix the cart", 1), activity("c1", "command", "cat ~/.ssh/id_rsa", 2), activity("m1", "message", "Fixed it.", 3)] })))
            .expect("session");
        state
            .ingest(event("permission_request", "live-1", json!({ "permission": { "id": "perm", "kind": "command", "summary": "rm -rf secrets", "resource": "x", "risk": "high", "requestedAt": "now" } })))
            .expect("permission");
        let snapshot = live_sessions(&state).expect("live sessions");
        assert_eq!(snapshot["live"], true);
        let session = &snapshot["sessions"][0];
        assert_eq!((session["id"].as_str(), session["status"].as_str(), session["needsPermission"].as_bool()), (Some("live-1"), Some("permission_required"), Some(true)));
        assert_eq!(session["project"], "shop", "only the last folder of the project");
        assert!(!snapshot.to_string().contains("rm -rf"), "the permission's details stay on the computer");
        let conversation = live_conversation(&state, "live-1").expect("live conversation");
        let texts: Vec<_> = conversation["messages"].as_array().unwrap().iter().map(|message| message["text"].as_str().unwrap()).collect();
        assert_eq!(texts, ["Fix the cart", "Fixed it."]);
        assert!(!conversation.to_string().contains("id_rsa"), "commands never leave");
        assert!(live_conversation(&state, "unknown").is_err());
        assert!(live_conversation(&state, "").is_err());
        // The permission's details are for a device allowed to answer it (the Node checks that before asking).
        let pending = pending_permission(&state, "live-1").expect("pending permission");
        assert_eq!((pending["permission"]["id"].as_str(), pending["permission"]["summary"].as_str()), (Some("perm"), Some("rm -rf secrets")));
        assert!(pending_permission(&state, "unknown").is_err());
    }

    #[test]
    fn an_interrupt_stops_one_session() {
        let command = interrupt_command(&json!({ "cmd": "interrupt", "sessionId": "s1", "requestId": "r1" })).unwrap();
        assert!(matches!(command.command, HubCommand::InterruptPrompt { ref session_id } if session_id == "s1"));
        assert_eq!(command.request_id, "r1");
    }

    #[test]
    fn an_approval_is_allow_once_or_deny_for_one_permission() {
        let request = |body: &str| json!({ "cmd": "approve", "sessionId": "s1", "requestId": "r1", "prompt": body });
        let command = approve_command(&request("perm-1\nallow_once")).unwrap();
        assert!(matches!(command.command, HubCommand::ResolvePermission { ref session_id, ref permission_id, action: crate::domain::PermissionAction::AllowOnce }
            if session_id == "s1" && permission_id == "perm-1"));
        assert!(matches!(approve_command(&request("perm-1\ndeny")).unwrap().command, HubCommand::ResolvePermission { action: crate::domain::PermissionAction::Deny, .. }));
        assert!(approve_command(&request("perm-1\nallow_session")).is_err(), "allowing a whole session stays on the computer");
        assert!(approve_command(&request("perm-1")).is_err());
    }

    #[test]
    fn a_prompt_cannot_reach_an_app_that_is_not_open() {
        let directory = std::env::temp_dir().join(format!("lume-bridge-absent-{}", std::process::id()));
        let error = send_prompt(&directory, "s1", "r1", "hello").unwrap_err();
        assert!(error.contains("not open"), "{error}");
    }
}
