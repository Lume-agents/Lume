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
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    codex_bridge::{CodexModelOption, CodexThreadModelSettings},
    domain::{
        AccessMode, AgentKind, HookEvent, HookEventKind, PermissionProfile, SessionActivity,
        SessionControlOrigin, SessionSource,
    },
    event_server,
    opencode_acp::read_bounded_line,
    state::{now_millis, AppState},
};

pub const PERMISSION_DEFAULT: &str = "agy_default";
pub const PERMISSION_ACCEPT_EDITS: &str = "agy_accept_edits";
pub const PERMISSION_PLAN: &str = "agy_plan";
pub const PERMISSION_ALLOW_ALL: &str = "agy_allow_all";

const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(10);

struct ManagedSession {
    child: Child,
    writer: Arc<Mutex<ChildStdin>>,
    active: Arc<AtomicBool>,
    turn: Arc<AtomicU64>,
    model: Option<String>,
    permission_mode: String,
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
        model: Option<&str>,
        permission_mode: &str,
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
        if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
            command.args(["--model", model]);
        }
        match permission_mode {
            PERMISSION_ACCEPT_EDITS => {
                command.args(["--mode", "accept-edits"]);
            }
            PERMISSION_PLAN => {
                command.args(["--mode", "plan"]);
            }
            PERMISSION_ALLOW_ALL => {
                command.arg("--dangerously-skip-permissions");
            }
            _ => {}
        }
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
                model: model.map(str::to_string).filter(|model| !model.is_empty()),
                permission_mode: permission_mode.to_string(),
            },
        );
        drop(sessions);
        let mut event = base_event(&id, HookEventKind::SessionStarted);
        event.project = Path::new(&cwd_text)
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        event.working_directory = Some(cwd_text);
        event.permission_profile = Some(permission_profile_for_mode(permission_mode));
        if let Err(error) = event_server::publish_event(&self.state, &self.app, event) {
            let _ = self.stop(&id);
            return Err(error);
        }
        Ok(id)
    }

    pub fn prompt(&self, id: &str, cwd: &str, prompt: &str) -> Result<(), String> {
        let model_override = self
            .state
            .session_model_override_for_native_id(AgentKind::Antigravity, id)?
            .model;
        let permission_override = self
            .state
            .permission_mode_override_for_native_id(AgentKind::Antigravity, id)?;
        let restart_settings = {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| "Antigravity session lock failed")?;
            match sessions.get_mut(id) {
                Some(session) => {
                    let stale = session
                        .child
                        .try_wait()
                        .map_err(|error| error.to_string())?
                        .is_some();
                    let requested_model = model_override
                        .as_deref()
                        .map(|model| (!model.trim().is_empty()).then(|| model.to_string()))
                        .unwrap_or_else(|| session.model.clone());
                    let requested_permission = permission_override
                        .clone()
                        .unwrap_or_else(|| session.permission_mode.clone());
                    let configuration_changed = requested_model != session.model
                        || requested_permission != session.permission_mode;
                    if configuration_changed && session.active.load(Ordering::SeqCst) {
                        return Err(
                            "Finalize o prompt atual para aplicar os ajustes do Antigravity".into(),
                        );
                    }
                    if stale || configuration_changed {
                        sessions.remove(id);
                        Some((requested_model, requested_permission))
                    } else {
                        None
                    }
                }
                None => Some((
                    model_override
                        .as_deref()
                        .and_then(|model| (!model.trim().is_empty()).then(|| model.to_string())),
                    permission_override.unwrap_or_else(|| PERMISSION_DEFAULT.into()),
                )),
            }
        };
        if let Some((model, permission_mode)) = restart_settings {
            self.launch(cwd, Some(id), model.as_deref(), &permission_mode)?;
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

    pub fn model_settings(&self, id: &str) -> Result<CodexThreadModelSettings, String> {
        let models = self.available_models()?;
        let current_model = self
            .sessions
            .lock()
            .map_err(|_| "Antigravity session lock failed")?
            .get(id)
            .and_then(|session| session.model.clone())
            .unwrap_or_default();
        Ok(CodexThreadModelSettings {
            model: current_model,
            reasoning_effort: None,
            service_tier: None,
            models,
            session_modes: None,
        })
    }

    fn available_models(&self) -> Result<Vec<CodexModelOption>, String> {
        let mut command = crate::executables::command("agy")?;
        command
            .args(["models"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        let started = Instant::now();
        loop {
            if child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                break;
            }
            if started.elapsed() >= MODEL_LIST_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    "A lista de modelos do Antigravity demorou demais para responder".into(),
                );
            }
            thread::sleep(Duration::from_millis(40));
        }
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(240)
                .collect::<String>();
            return Err(if detail.is_empty() {
                "Não foi possível carregar os modelos do Antigravity".into()
            } else {
                detail
            });
        }
        let mut models = parse_model_catalog(&String::from_utf8_lossy(&output.stdout));
        if models.len() == 1 {
            return Err("A CLI do Antigravity não retornou modelos disponíveis".into());
        }
        models.sort_by(|left, right| {
            right
                .is_default
                .cmp(&left.is_default)
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        Ok(models)
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

fn parse_model_catalog(output: &str) -> Vec<CodexModelOption> {
    let mut models = vec![CodexModelOption {
        model: String::new(),
        display_name: "Padrão do Antigravity".into(),
        description: "Usa o modelo padrão configurado na CLI do Antigravity".into(),
        is_default: true,
        default_reasoning_effort: String::new(),
        supported_reasoning_efforts: Vec::new(),
    }];
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let Some(model) = parts.next() else { continue };
        let display_name = parts.collect::<Vec<_>>().join(" ");
        let valid_slug = !matches!(model, "Available" | "Models" | "Model" | "Usage")
            && !model.ends_with(':')
            && model
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.:/".contains(character));
        if !valid_slug || display_name.is_empty() || models.iter().any(|entry| entry.model == model)
        {
            continue;
        }
        models.push(CodexModelOption {
            model: model.to_string(),
            display_name,
            description: "Disponível na CLI do Antigravity".into(),
            is_default: false,
            default_reasoning_effort: String::new(),
            supported_reasoning_efforts: Vec::new(),
        });
    }
    models
}

pub fn is_permission_mode(mode: &str) -> bool {
    matches!(
        mode,
        PERMISSION_DEFAULT | PERMISSION_ACCEPT_EDITS | PERMISSION_PLAN | PERMISSION_ALLOW_ALL
    )
}

pub fn permission_profile_for_mode(mode: &str) -> PermissionProfile {
    PermissionProfile {
        mode: access_mode_for_permission(mode),
        label: permission_label_for_mode(mode).into(),
        approval_policy: approval_policy_for_mode(mode).into(),
        approvals_reviewer: None,
        can_respond_from_lume: false,
        available_actions: Vec::new(),
    }
}

fn access_mode_for_permission(mode: &str) -> AccessMode {
    match mode {
        PERMISSION_ACCEPT_EDITS => AccessMode::WorkspaceWrite,
        PERMISSION_PLAN => AccessMode::Plan,
        PERMISSION_ALLOW_ALL => AccessMode::FullAccess,
        _ => AccessMode::Custom,
    }
}

fn permission_label_for_mode(mode: &str) -> &'static str {
    match mode {
        PERMISSION_ACCEPT_EDITS => "Antigravity · Accept edits",
        PERMISSION_PLAN => "Antigravity · Plan",
        PERMISSION_ALLOW_ALL => "Antigravity · Allow all tools",
        _ => "Antigravity · Native defaults",
    }
}

fn approval_policy_for_mode(mode: &str) -> &'static str {
    match mode {
        PERMISSION_ACCEPT_EDITS => PERMISSION_ACCEPT_EDITS,
        PERMISSION_PLAN => PERMISSION_PLAN,
        PERMISSION_ALLOW_ALL => PERMISSION_ALLOW_ALL,
        _ => "on-request",
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
