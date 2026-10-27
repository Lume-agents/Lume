//! The link between the Node (`lume node run`, a separate process) and the Lume app on the same computer.
//!
//! The Node cannot run an agent itself: a prompt from another computer has to reach the app, which owns the sessions.
//! While the app runs it listens on a loopback port and writes that port and a random secret to `node-bridge.json`
//! in the Node's state folder (readable only by the user). The Node proves it holds the secret with an HMAC over
//! the request, so another program, or a web page that guesses the port, cannot send a prompt through it.
//!
//! The first command is `prompt`: send text to one of the app's own sessions, with the request id that makes a repeat
//! harmless. The app answers with the same result a phone gets from the gateway.

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
    protocol::{HubCommand, HubCommandRequest},
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
    let command = prompt_command(&request)?;
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
        Err(response.error.map(|error| error.message).unwrap_or_else(|| "The Lume app refused the prompt".into()))
    }
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
    .map(|request: HubCommandRequest| {
        debug_assert!(matches!(request.command, HubCommand::SubmitPrompt { .. }));
        request
    })
}

// ── Node side ────────────────────────────────────────────────────────────────

/// Sends a prompt to the running Lume app on this computer. `Err` when the app is not open.
pub fn send_prompt(directory: &Path, session_id: &str, request_id: &str, prompt: &str) -> Result<Value, String> {
    let stored: Stored = serde_json::from_slice(&fs::read(path(directory)).map_err(|_| "the Lume app is not open on that computer".to_string())?)
        .map_err(|_| "the Lume app is not open on that computer".to_string())?;
    let mut stream = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, stored.port)), IO_TIMEOUT)
        .map_err(|_| "the Lume app is not open on that computer".to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).map_err(io)?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(io)?;
    let mac = sign(&stored.secret, &signed_text("prompt", session_id, request_id, prompt));
    let request = json!({ "cmd": "prompt", "sessionId": session_id, "requestId": request_id, "prompt": prompt, "mac": mac });
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

    #[test]
    fn a_prompt_cannot_reach_an_app_that_is_not_open() {
        let directory = std::env::temp_dir().join(format!("lume-bridge-absent-{}", std::process::id()));
        let error = send_prompt(&directory, "s1", "r1", "hello").unwrap_err();
        assert!(error.contains("not open"), "{error}");
    }
}
