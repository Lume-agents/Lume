use std::{
    collections::HashMap,
    io::{BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    domain::{
        AccessMode, AgentKind, HookEvent, HookEventKind, PermissionProfile, SessionActivity,
        SessionControlOrigin, SessionSource,
    },
    event_server,
    opencode_acp::read_bounded_line,
    state::{now_millis, AppState},
};

struct ManagedSession {
    child: Child,
    writer: Arc<Mutex<ChildStdin>>,
    active: Arc<AtomicBool>,
    turn: Arc<AtomicU64>,
}

impl Drop for ManagedSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Clone)]
pub struct AntigravityStream {
    sessions: Arc<Mutex<HashMap<String, ManagedSession>>>,
    state: AppState,
    app: AppHandle,
}

impl AntigravityStream {
    pub fn new(state: AppState, app: AppHandle) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            state,
            app,
        }
    }

    pub fn launch(
        &self,
        working_directory: &str,
        resume_id: Option<&str>,
    ) -> Result<String, String> {
        let cwd = std::fs::canonicalize(working_directory)
            .map_err(|_| "A pasta do projeto do Antigravity não existe".to_string())?;
        if !cwd.is_dir() {
            return Err("A pasta do projeto do Antigravity não é um diretório".into());
        }
        let cwd_text = cwd.to_string_lossy().to_string();
        let mut command = crate::executables::command("agy")?;
        command
            .args([
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
            ])
            .current_dir(&cwd)
            .env("LUME_MANAGED_SESSION", "1")
            .env("LUME_ANTIGRAVITY_STREAM", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(id) = resume_id {
            command.args(["--conversation", id]);
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not launch Antigravity CLI: {error}"))?;
        let writer = Arc::new(Mutex::new(
            child.stdin.take().ok_or("Antigravity stdin unavailable")?,
        ));
        let output = child
            .stdout
            .take()
            .ok_or("Antigravity stdout unavailable")?;
        let active = Arc::new(AtomicBool::new(false));
        let turn = Arc::new(AtomicU64::new(0));
        let (init_sender, init_receiver) = mpsc::channel();
        let reader_state = self.state.clone();
        let reader_app = self.app.clone();
        let reader_active = active.clone();
        let reader_turn = turn.clone();
        thread::Builder::new()
            .name("lume-antigravity-stream".into())
            .spawn(move || {
                read_stream(
                    output,
                    init_sender,
                    reader_active,
                    reader_turn,
                    reader_state,
                    reader_app,
                )
            })
            .map_err(|error| error.to_string())?;
        let id = match init_receiver.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(id)) => id,
            Ok(Err(error)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Antigravity did not provide a conversation ID during startup".into());
            }
        };
        if resume_id.is_some_and(|requested| requested != id) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Antigravity resumed a different conversation than requested".into());
        }
        let mut sessions = match self.sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Antigravity session lock failed".into());
            }
        };
        sessions.insert(
            id.clone(),
            ManagedSession {
                child,
                writer,
                active,
                turn,
            },
        );
        drop(sessions);
        let mut event = base_event(&id, HookEventKind::SessionStarted);
        event.project = Path::new(&cwd_text)
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        event.working_directory = Some(cwd_text);
        event.permission_profile = Some(PermissionProfile {
            mode: AccessMode::WorkspaceWrite,
            label: "Antigravity headless".into(),
            approval_policy: "native".into(),
            approvals_reviewer: None,
            can_respond_from_lume: false,
            available_actions: Vec::new(),
        });
        if let Err(error) = event_server::publish_event(&self.state, &self.app, event) {
            let _ = self.stop(&id);
            return Err(error);
        }
        Ok(id)
    }

    pub fn prompt(&self, id: &str, cwd: &str, prompt: &str) -> Result<(), String> {
        let restart = {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| "Antigravity session lock failed")?;
            let stale = match sessions.get_mut(id) {
                Some(session) => session
                    .child
                    .try_wait()
                    .map_err(|error| error.to_string())?
                    .is_some(),
                None => true,
            };
            if stale {
                sessions.remove(id);
            }
            stale
        };
        if restart {
            self.launch(cwd, Some(id))?;
        }
        let (writer, active) = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| "Antigravity session lock failed")?;
            let session = sessions
                .get(id)
                .ok_or("Antigravity session is not connected")?;
            if session.active.swap(true, Ordering::SeqCst) {
                return Err("Este prompt do Antigravity já está em execução".into());
            }
            session.turn.fetch_add(1, Ordering::SeqCst);
            (session.writer.clone(), session.active.clone())
        };
        if let Err(error) = event_server::publish_event(
            &self.state,
            &self.app,
            base_event(id, HookEventKind::Running),
        ) {
            active.store(false, Ordering::SeqCst);
            return Err(error);
        }
        let payload = json!({"event":"user","message":{"content":prompt}});
        let written = writer
            .lock()
            .map_err(|_| "Antigravity writer lock failed".to_string())
            .and_then(|mut writer| {
                writer
                    .write_all(format!("{payload}\n").as_bytes())
                    .map_err(|error| error.to_string())
            });
        if let Err(error) = written {
            active.store(false, Ordering::SeqCst);
            return Err(format!("Antigravity stream disconnected: {error}"));
        }
        Ok(())
    }

    pub fn stop(&self, id: &str) -> Result<(), String> {
        if let Some(session) = self
            .sessions
            .lock()
            .map_err(|_| "Antigravity session lock failed")?
            .remove(id)
        {
            session.active.store(false, Ordering::SeqCst);
            drop(session);
        }
        Ok(())
    }
}

fn base_event(id: &str, event: HookEventKind) -> HookEvent {
    HookEvent {
        event,
        session_id: format!("antigravity-stream:{id}"),
        agent: AgentKind::Antigravity,
        agent_label: Some("Antigravity CLI".into()),
        session_name: None,
        project: None,
        source: Some(SessionSource::Desktop),
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status_label: None,
        started_at: None,
        process_id: None,
        native_session_id: Some(id.into()),
        working_directory: None,
        permission_profile: None,
        permission: None,
        question: None,
        last_response: None,
        activity: None,
        activities: Vec::new(),
        wait_for_decision: false,
    }
}

fn read_stream(
    output: impl std::io::Read,
    init: mpsc::Sender<Result<String, String>>,
    active: Arc<AtomicBool>,
    turn: Arc<AtomicU64>,
    state: AppState,
    app: AppHandle,
) {
    let mut reader = BufReader::new(output);
    let mut id: Option<String> = None;
    while let Ok(Some(line)) = read_bounded_line(&mut reader) {
        if line.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        match message["event"].as_str() {
            Some("init") => {
                if let Some(conversation) = message["conversation_id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                {
                    id = Some(conversation.into());
                    let _ = init.send(Ok(conversation.into()));
                } else {
                    let _ = init.send(Err("Antigravity init omitted the conversation ID".into()));
                }
            }
            Some("step_update") => {
                let Some(id) = id.as_deref() else { continue };
                let step = &message["step_update"];
                let activity = match step["step_type"].as_str() {
                    Some("agent_response") => {
                        let Some(text) =
                            step["text_delta"].as_str().filter(|text| !text.is_empty())
                        else {
                            continue;
                        };
                        SessionActivity {
                            id: format!("agy-response:{id}:{}", turn.load(Ordering::SeqCst)),
                            kind: "message".into(),
                            title: "Antigravity".into(),
                            detail: Some(text.chars().take(32 * 1024).collect()),
                            status: "running".into(),
                            created_at: now_millis(),
                            files: Vec::new(),
                            attachments: Vec::new(),
                            append_detail: true,
                        }
                    }
                    Some("tool") => SessionActivity {
                        id: format!("agy-tool:{id}:{}", step["step_index"].as_u64().unwrap_or(0)),
                        kind: "tool".into(),
                        title: step["tool_name"]
                            .as_str()
                            .unwrap_or("Ferramenta")
                            .chars()
                            .take(120)
                            .collect(),
                        detail: None,
                        status: if step["state"].as_str() == Some("DONE") {
                            "completed"
                        } else {
                            "running"
                        }
                        .into(),
                        created_at: now_millis(),
                        files: Vec::new(),
                        attachments: Vec::new(),
                        append_detail: false,
                    },
                    _ => continue,
                };
                let mut event = base_event(id, HookEventKind::Activity);
                event.activity = Some(activity);
                let _ = event_server::publish_event(&state, &app, event);
            }
            Some("result") => {
                let Some(id) = id.as_deref() else { continue };
                active.store(false, Ordering::SeqCst);
                let result = &message["result"];
                let status = result["status"].as_str().unwrap_or("ERROR");
                let event_kind = match status {
                    "SUCCESS" => HookEventKind::Completed,
                    "CANCELED" | "INTERRUPTED" | "WAITING" => HookEventKind::WaitingForInput,
                    _ => HookEventKind::Failed,
                };
                let mut event = base_event(id, event_kind);
                event.last_response = result["response"].as_str().map(str::to_string);
                if status != "SUCCESS" {
                    event.status_label = Some(
                        result["error"]
                            .as_str()
                            .map(|error| error.chars().take(150).collect())
                            .unwrap_or_else(|| format!("Antigravity: {status}")),
                    );
                }
                let _ = event_server::publish_event(&state, &app, event);
            }
            _ => {}
        }
    }
    if let Some(id) = id.as_deref().filter(|_| active.load(Ordering::SeqCst)) {
        let mut event = base_event(id, HookEventKind::Failed);
        event.status_label = Some("Conexão do Antigravity encerrada".into());
        let _ = event_server::publish_event(&state, &app, event);
    } else if id.is_none() {
        let _ = init.send(Err("Antigravity CLI closed before initialization".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_event_keeps_provider_identity() {
        let event = base_event("conversation-1", HookEventKind::Running);
        assert_eq!(event.native_session_id.as_deref(), Some("conversation-1"));
        assert_eq!(event.control_origin, SessionControlOrigin::Lume);
        assert_eq!(event.source, Some(SessionSource::Desktop));
    }
}
