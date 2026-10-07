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
    codex_bridge::{CodexModelOption, CodexReasoningEffortOption, CodexThreadModelSettings},
    domain::{
        AccessMode, AgentKind, AgentRateLimit, HookEvent, HookEventKind, PermissionProfile,
        SessionActivity, SessionControlOrigin, SessionSource,
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
const USAGE_TIMEOUT: Duration = Duration::from_secs(15);
const EFFORT_LEVELS: [&str; 3] = ["low", "medium", "high"];

struct ManagedSession {
    child: Child,
    writer: Arc<Mutex<ChildStdin>>,
    active: Arc<AtomicBool>,
    turn: Arc<AtomicU64>,
    model: Option<String>,
    effort: Option<String>,
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
        effort: Option<&str>,
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
        if let Some(effort) = effort.filter(|effort| is_effort(effort)) {
            command.args(["--effort", effort]);
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
                effort: effort
                    .filter(|effort| is_effort(effort))
                    .map(str::to_string),
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
            .session_model_override_for_native_id(AgentKind::Antigravity, id)?;
        let effort_override = model_override.reasoning_effort;
        let model_override = model_override.model;
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
                    let requested_effort = effort_override
                        .as_deref()
                        .map(|effort| is_effort(effort).then(|| effort.to_string()))
                        .unwrap_or_else(|| session.effort.clone());
                    let requested_permission = permission_override
                        .clone()
                        .unwrap_or_else(|| session.permission_mode.clone());
                    let configuration_changed = requested_model != session.model
                        || requested_effort != session.effort
                        || requested_permission != session.permission_mode;
                    if configuration_changed && session.active.load(Ordering::SeqCst) {
                        return Err(
                            "Finalize o prompt atual para aplicar os ajustes do Antigravity".into(),
                        );
                    }
                    if stale || configuration_changed {
                        sessions.remove(id);
                        Some((requested_model, requested_effort, requested_permission))
                    } else {
                        None
                    }
                }
                None => Some((
                    model_override
                        .as_deref()
                        .and_then(|model| (!model.trim().is_empty()).then(|| model.to_string())),
                    effort_override.filter(|effort| is_effort(effort)),
                    permission_override.unwrap_or_else(|| PERMISSION_DEFAULT.into()),
                )),
            }
        };
        if let Some((model, effort, permission_mode)) = restart_settings {
            self.launch(
                cwd,
                Some(id),
                model.as_deref(),
                effort.as_deref(),
                &permission_mode,
            )?;
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
        let (current_model, current_effort) = self
            .sessions
            .lock()
            .map_err(|_| "Antigravity session lock failed")?
            .get(id)
            .map(|session| (session.model.clone(), session.effort.clone()))
            .unwrap_or_default();
        Ok(CodexThreadModelSettings {
            model: current_model.unwrap_or_default(),
            reasoning_effort: current_effort,
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
        let listing = String::from_utf8_lossy(&output.stdout);
        if parse_model_lines(&listing).is_empty() {
            return Err("A CLI do Antigravity não retornou modelos disponíveis".into());
        }
        let mut models = parse_model_catalog(&listing, default_model_label().as_deref());
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

/// Quota windows of the `agy` CLI, read from `/usage` (zero tokens).
pub fn fetch_rate_limits() -> Result<Vec<AgentRateLimit>, String> {
    let mut command = crate::executables::command("agy")?;
    command
        .args(["-p", "/usage", "--output-format", "stream-json"])
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    // Drain stdout while waiting so a large payload cannot fill the pipe and stall the child.
    let mut stdout = child
        .stdout
        .take()
        .ok_or("Antigravity stdout unavailable")?;
    let reader = thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut buffer);
        buffer
    });
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            break;
        }
        if started.elapsed() >= USAGE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("O consumo do Antigravity demorou demais para responder".into());
        }
        thread::sleep(Duration::from_millis(40));
    }
    let output = reader.join().unwrap_or_default();
    let listing = String::from_utf8_lossy(&output);
    let limits = listing
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|message| {
            message["event"] == "command_result" && message["command"]["name"] == "usage"
        })
        .map(|message| parse_usage_rate_limits(&message))
        .unwrap_or_default();
    if limits.is_empty() {
        return Err("A CLI do Antigravity não retornou o consumo de quota".into());
    }
    Ok(limits)
}

/// Turns the `command_result` of `/usage` into one limit per group and window.
pub fn parse_usage_rate_limits(output: &Value) -> Vec<AgentRateLimit> {
    let mut limits = Vec::new();
    let Some(groups) = output
        .pointer("/command/data/groups")
        .and_then(Value::as_array)
    else {
        return limits;
    };
    for group in groups {
        let name = group.get("name").and_then(Value::as_str).unwrap_or("");
        let group_label = if name.to_lowercase().contains("gemini") {
            "Gemini"
        } else {
            "3P Models"
        };
        let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
            continue;
        };
        for bucket in buckets {
            let Some(id) = bucket.get("id").and_then(Value::as_str) else {
                continue;
            };
            let window = bucket.get("window").and_then(Value::as_str).unwrap_or("");
            let remaining = bucket
                .get("remaining_fraction")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            let (window_minutes, window_label) = match window {
                "5h" => (Some(300), "5h"),
                "weekly" => (Some(10080), "Semanal"),
                other => (None, other),
            };
            limits.push(AgentRateLimit {
                id: format!("antigravity:{id}"),
                label: format!("{group_label} · {window_label}"),
                used_percent: ((1.0 - remaining).clamp(0.0, 1.0) * 100.0).round() as u8,
                resets_at: bucket
                    .get("reset_time")
                    .and_then(Value::as_str)
                    .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                    .map(|stamp| stamp.timestamp_millis()),
                window_minutes,
            });
        }
    }
    limits
}

/// The `<id>\t<label>` lines of `agy models`, without headings or anything else.
fn parse_model_lines(output: &str) -> Vec<(String, String)> {
    let mut lines: Vec<(String, String)> = Vec::new();
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let Some(model) = parts.next() else { continue };
        let display_name = parts.collect::<Vec<_>>().join(" ");
        let valid_slug = !matches!(model, "Available" | "Models" | "Model" | "Usage")
            && !model.ends_with(':')
            && model
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.:/".contains(character));
        if !valid_slug || display_name.is_empty() || lines.iter().any(|(id, _)| id == model) {
            continue;
        }
        lines.push((model.to_string(), display_name));
    }
    lines
}

/// The first entry has an empty model, so no `--model` is passed and the session keeps
/// following the CLI. It is named after the model the CLI uses (`default_label`), and that
/// model is not listed again below it.
fn parse_model_catalog(output: &str, default_label: Option<&str>) -> Vec<CodexModelOption> {
    let default_label = default_label
        .map(str::trim)
        .filter(|label| !label.is_empty());
    let mut models = vec![CodexModelOption {
        model: String::new(),
        display_name: default_label.unwrap_or("Padrão do Antigravity").into(),
        description: if default_label.is_some() {
            "Padrão da CLI do Antigravity".into()
        } else {
            "Usa o modelo padrão configurado na CLI do Antigravity".into()
        },
        is_default: true,
        default_reasoning_effort: String::new(),
        supported_reasoning_efforts: effort_options(),
    }];
    for (model, display_name) in parse_model_lines(output) {
        let is_the_default =
            default_label.is_some_and(|label| label.eq_ignore_ascii_case(display_name.trim()));
        if is_the_default {
            continue;
        }
        models.push(CodexModelOption {
            model,
            display_name,
            description: "Disponível na CLI do Antigravity".into(),
            is_default: false,
            default_reasoning_effort: String::new(),
            supported_reasoning_efforts: effort_options(),
        });
    }
    models
}

const MAX_DEFAULT_LABEL_CHARS: usize = 120;

/// The model the `agy` CLI uses when none is chosen. Its settings hold the label
/// (for example "Gemini 3.1 Pro (High)"), not the id.
fn default_model_label() -> Option<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    read_default_model_label(
        &std::path::Path::new(&home).join(".gemini/antigravity-cli/settings.json"),
    )
}

fn read_default_model_label(path: &std::path::Path) -> Option<String> {
    let settings = serde_json::from_str::<Value>(&std::fs::read_to_string(path).ok()?).ok()?;
    let label = settings.get("model")?.as_str()?.trim();
    (!label.is_empty()).then(|| label.chars().take(MAX_DEFAULT_LABEL_CHARS).collect())
}

/// The CLI default comes first (empty value) so the slider can return to it.
fn effort_options() -> Vec<CodexReasoningEffortOption> {
    std::iter::once(CodexReasoningEffortOption {
        value: String::new(),
        description: "Usa o esforço padrão configurado na CLI do Antigravity".into(),
    })
    .chain(
        EFFORT_LEVELS
            .iter()
            .map(|effort| CodexReasoningEffortOption {
                value: (*effort).into(),
                description: format!("--effort {effort}"),
            }),
    )
    .collect()
}

pub fn is_effort(effort: &str) -> bool {
    EFFORT_LEVELS.contains(&effort)
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

/// Reads stay `tool` on purpose: the UI treats `file` activities as edits
/// and lists their paths among the turn's changed files.
fn tool_activity(id: &str, step: &Value) -> SessionActivity {
    let tool_name = step["tool_name"].as_str().unwrap_or("Ferramenta");
    let tool_info = &step["tool_info"];
    let params = &tool_info["parameters"];
    let first_param = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| params[*key].as_str().filter(|value| !value.is_empty()))
    };
    let path = first_param(&["TargetFile", "AbsolutePath"]);
    let edited_path = match tool_name {
        "write_to_file" | "replace_file_content" => path,
        _ => None,
    };
    let (kind, title) = match tool_name {
        "run_command" => ("command", first_param(&["CommandLine", "toolSummary"])),
        "manage_task" => ("tool", first_param(&["Action", "toolSummary"])),
        _ if edited_path.is_some() => ("file", edited_path),
        // Reads, and edits without a path: a summary must not pose as a file.
        "view_file" | "write_to_file" | "replace_file_content" => {
            ("tool", path.or_else(|| first_param(&["toolSummary"])))
        }
        _ => ("tool", first_param(&["toolSummary", "toolAction"])),
    };
    let title = title.unwrap_or(tool_name);
    let done = step["state"].as_str() == Some("DONE");
    let detail = tool_info["output"]
        .as_str()
        .filter(|output| done && !output.is_empty())
        .map(str::to_string)
        .or_else(|| {
            params
                .as_object()
                .filter(|params| !params.is_empty())
                .and_then(|params| serde_json::to_string_pretty(params).ok())
        });
    SessionActivity {
        id: format!("agy-tool:{id}:{}", step["step_index"].as_u64().unwrap_or(0)),
        kind: kind.into(),
        title: title.chars().take(180).collect(),
        detail: detail.map(|text| text.chars().take(32 * 1024).collect()),
        status: if done { "completed" } else { "running" }.into(),
        created_at: now_millis(),
        files: edited_path.map(str::to_string).into_iter().collect(),
        attachments: Vec::new(),
        append_detail: false,
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
                    Some("tool") => tool_activity(id, step),
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

    const AGY_MODELS: &str = "Available models:\n\
        gemini-3.1-pro-high\tGemini 3.1 Pro (High)\n\
        gemini-3.1-pro-low\tGemini 3.1 Pro (Low)\n\
        claude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)\n";

    fn shown(models: &[CodexModelOption]) -> Vec<(&str, &str)> {
        models
            .iter()
            .map(|entry| (entry.model.as_str(), entry.display_name.as_str()))
            .collect()
    }

    #[test]
    fn the_default_entry_is_named_after_the_model_the_cli_uses() {
        let models = parse_model_catalog(AGY_MODELS, Some("Gemini 3.1 Pro (High)"));
        assert_eq!(
            shown(&models),
            [
                ("", "Gemini 3.1 Pro (High)"),
                ("gemini-3.1-pro-low", "Gemini 3.1 Pro (Low)"),
                ("claude-sonnet-4-6", "Claude Sonnet 4.6 (Thinking)"),
            ],
            "the default is not listed a second time"
        );
        assert_eq!(models[0].description, "Padrão da CLI do Antigravity");
        assert!(
            models[0].is_default && models[0].model.is_empty(),
            "it keeps following the CLI"
        );
        // Lookup ignores case and the spaces at the ends.
        let loose = parse_model_catalog(AGY_MODELS, Some("  gemini 3.1 PRO (high) "));
        assert_eq!(loose.len(), 3);
    }

    #[test]
    fn without_a_known_default_the_generic_entry_stays() {
        for label in [None, Some(""), Some("   ")] {
            let models = parse_model_catalog(AGY_MODELS, label);
            assert_eq!(models.len(), 4, "every listed model stays");
            assert_eq!(models[0].display_name, "Padrão do Antigravity");
            assert_eq!(
                models[0].description,
                "Usa o modelo padrão configurado na CLI do Antigravity"
            );
        }
    }

    #[test]
    fn a_default_the_list_does_not_have_is_still_named() {
        let models = parse_model_catalog(AGY_MODELS, Some("Gemini 9 Ultra"));
        assert_eq!(models[0].display_name, "Gemini 9 Ultra");
        assert_eq!(models.len(), 4, "the list is left as it is");
    }

    #[test]
    fn a_catalog_of_one_model_is_not_an_empty_catalog() {
        let only = "gemini-3.1-pro-high\tGemini 3.1 Pro (High)\n";
        let models = parse_model_catalog(only, Some("Gemini 3.1 Pro (High)"));
        assert_eq!(models.len(), 1, "only the named default remains");
        assert_eq!(
            parse_model_lines(only).len(),
            1,
            "so the CLI did list a model"
        );
        assert!(parse_model_lines("Available models:\n").is_empty());
    }

    #[test]
    fn the_default_label_comes_from_the_cli_settings() {
        let directory = std::env::temp_dir().join(format!("lume-agy-settings-{}", now_millis()));
        std::fs::create_dir_all(&directory).unwrap();
        let read = |content: Option<&str>| {
            let path = directory.join("settings.json");
            match content {
                Some(content) => std::fs::write(&path, content).unwrap(),
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
            read_default_model_label(&path)
        };
        assert_eq!(
            read(Some(
                r#"{"colorScheme":"dark","model":" Gemini 3.1 Pro (High) "}"#
            ))
            .as_deref(),
            Some("Gemini 3.1 Pro (High)")
        );
        assert_eq!(read(None), None, "no file");
        assert_eq!(read(Some("{not json")), None);
        assert_eq!(
            read(Some(r#"{"colorScheme":"dark"}"#)),
            None,
            "no model key"
        );
        assert_eq!(read(Some(r#"{"model":"   "}"#)), None, "only spaces");
        assert_eq!(read(Some(r#"{"model":""}"#)), None);
        assert_eq!(read(Some(r#"{"model":42}"#)), None, "not a string");
        let long = format!(r#"{{"model":"{}"}}"#, "x".repeat(500));
        assert_eq!(
            read(Some(&long)).map(|label| label.chars().count()),
            Some(120)
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn managed_event_keeps_provider_identity() {
        let event = base_event("conversation-1", HookEventKind::Running);
        assert_eq!(event.native_session_id.as_deref(), Some("conversation-1"));
        assert_eq!(event.control_origin, SessionControlOrigin::Lume);
        assert_eq!(event.source, Some(SessionSource::Desktop));
    }

    #[test]
    fn model_catalog_offers_cli_effort_levels() {
        let models = parse_model_catalog("gemini-3.1-pro-high\tGemini 3.1 Pro (High)\n", None);
        assert_eq!(models.len(), 2);
        for model in &models {
            let efforts = model
                .supported_reasoning_efforts
                .iter()
                .map(|effort| effort.value.as_str())
                .collect::<Vec<_>>();
            assert_eq!(efforts, ["", "low", "medium", "high"]);
            assert_eq!(model.default_reasoning_effort, "");
        }
        assert!(is_effort("high"));
        assert!(!is_effort("xhigh"));
        assert!(!is_effort("max"));
        assert!(!is_effort(""));
        assert!(!is_effort("ultra"));
    }

    #[test]
    fn finished_command_shows_command_line_and_output() {
        let activity = tool_activity(
            "conversation-1",
            &json!({
                "step_type": "tool",
                "step_index": 4,
                "state": "DONE",
                "tool_name": "run_command",
                "tool_info": {
                    "parameters": { "CommandLine": "cargo test", "toolSummary": "Run tests" },
                    "output": "test result: ok. 12 passed",
                },
            }),
        );
        assert_eq!(activity.id, "agy-tool:conversation-1:4");
        assert_eq!(activity.kind, "command");
        assert_eq!(activity.title, "cargo test");
        assert_eq!(
            activity.detail.as_deref(),
            Some("test result: ok. 12 passed")
        );
        assert_eq!(activity.status, "completed");
    }

    #[test]
    fn running_task_shows_action_and_parameters() {
        let activity = tool_activity(
            "conversation-1",
            &json!({
                "step_index": 2,
                "state": "ACTIVE",
                "tool_name": "manage_task",
                "tool_info": { "parameters": { "Action": "create", "TaskName": "Fix CI" } },
            }),
        );
        assert_eq!(activity.kind, "tool");
        assert_eq!(activity.title, "create");
        assert_eq!(activity.status, "running");
        let detail = activity.detail.expect("parameters are shown while running");
        assert!(detail.contains("\"TaskName\": \"Fix CI\""), "{detail}");
    }

    #[test]
    fn running_tool_ignores_partial_output() {
        let activity = tool_activity(
            "conversation-1",
            &json!({
                "state": "ACTIVE",
                "tool_name": "run_command",
                "tool_info": {
                    "parameters": { "CommandLine": "npm run build" },
                    "output": "vite v6 building...",
                },
            }),
        );
        let detail = activity.detail.expect("parameters are shown while running");
        assert!(detail.contains("npm run build"), "{detail}");
        assert!(!detail.contains("building"), "{detail}");
    }

    #[test]
    fn finished_task_shows_its_output() {
        let activity = tool_activity(
            "conversation-1",
            &json!({
                "state": "DONE",
                "tool_name": "manage_task",
                "tool_info": {
                    "parameters": { "Action": "complete" },
                    "output": "Task Fix CI completed",
                },
            }),
        );
        assert_eq!(activity.title, "complete");
        assert_eq!(activity.detail.as_deref(), Some("Task Fix CI completed"));
        assert_eq!(activity.status, "completed");
    }

    #[test]
    fn file_edits_report_their_target() {
        let activity = tool_activity(
            "conversation-1",
            &json!({
                "state": "DONE",
                "tool_name": "replace_file_content",
                "tool_info": { "parameters": { "TargetFile": "/repo/src/lib.rs" } },
            }),
        );
        assert_eq!(activity.kind, "file");
        assert_eq!(activity.title, "/repo/src/lib.rs");
        assert_eq!(activity.files, ["/repo/src/lib.rs"]);

        let pathless = tool_activity(
            "conversation-1",
            &json!({
                "tool_name": "write_to_file",
                "tool_info": { "parameters": { "toolSummary": "Write notes" } },
            }),
        );
        assert_eq!(pathless.kind, "tool");
        assert_eq!(pathless.title, "Write notes");
        assert!(pathless.files.is_empty());
    }

    #[test]
    fn file_reads_are_not_reported_as_edits() {
        let activity = tool_activity(
            "conversation-1",
            &json!({
                "state": "DONE",
                "tool_name": "view_file",
                "tool_info": { "parameters": { "AbsolutePath": "/repo/README.md" } },
            }),
        );
        assert_eq!(activity.kind, "tool");
        assert_eq!(activity.title, "/repo/README.md");
        assert!(activity.files.is_empty());
    }

    #[test]
    fn unknown_tools_fall_back_to_summary_then_name() {
        let summarized = tool_activity(
            "conversation-1",
            &json!({
                "tool_name": "grep_search",
                "tool_info": { "parameters": { "toolSummary": "Search for TODO" } },
            }),
        );
        assert_eq!(summarized.title, "Search for TODO");
        let bare = tool_activity("conversation-1", &json!({ "tool_name": "list_dir" }));
        assert_eq!(bare.title, "list_dir");
        assert_eq!(bare.detail, None);
        let unnamed = tool_activity("conversation-1", &json!({}));
        assert_eq!(unnamed.title, "Ferramenta");
    }

    #[test]
    fn tool_detail_and_title_are_bounded() {
        let activity = tool_activity(
            "conversation-1",
            &json!({
                "state": "DONE",
                "tool_name": "run_command",
                "tool_info": {
                    "parameters": { "CommandLine": "x".repeat(500) },
                    "output": "y".repeat(64 * 1024),
                },
            }),
        );
        assert_eq!(activity.title.chars().count(), 180);
        assert_eq!(
            activity.detail.map(|text| text.chars().count()),
            Some(32 * 1024)
        );
    }

    const AGY_USAGE: &str = r#"{"event":"command_result","command":{"name":"usage","data":{"groups":[
        {"name":"Gemini Models","buckets":[
            {"id":"gemini-weekly","window":"weekly","remaining_fraction":0.9723,"reset_time":"2026-10-14T02:32:39Z"},
            {"id":"gemini-5h","window":"5h","remaining_fraction":0.98,"reset_time":"2026-10-07T23:31:41Z"}]},
        {"name":"Claude and GPT models","buckets":[
            {"id":"3p-weekly","window":"weekly","remaining_fraction":0.12,"reset_time":"2026-10-11T19:08:56Z"},
            {"id":"3p-5h","window":"5h","remaining_fraction":1,"reset_time":"2026-10-08T00:59:49Z"}]}]}}}"#;

    #[test]
    fn usage_buckets_become_labelled_rate_limits() {
        let limits = parse_usage_rate_limits(&serde_json::from_str(AGY_USAGE).unwrap());
        let shown: Vec<_> = limits
            .iter()
            .map(|limit| {
                (
                    limit.id.as_str(),
                    limit.label.as_str(),
                    limit.used_percent,
                    limit.window_minutes,
                )
            })
            .collect();
        assert_eq!(
            shown,
            vec![
                (
                    "antigravity:gemini-weekly",
                    "Gemini · Semanal",
                    3,
                    Some(10080)
                ),
                ("antigravity:gemini-5h", "Gemini · 5h", 2, Some(300)),
                (
                    "antigravity:3p-weekly",
                    "3P Models · Semanal",
                    88,
                    Some(10080)
                ),
                ("antigravity:3p-5h", "3P Models · 5h", 0, Some(300)),
            ]
        );
    }

    #[test]
    fn usage_reset_time_is_unix_milliseconds() {
        let limits = parse_usage_rate_limits(&serde_json::from_str(AGY_USAGE).unwrap());
        assert_eq!(limits[1].resets_at, Some(1_791_415_901_000));
    }

    #[test]
    fn usage_tolerates_missing_fields_and_foreign_payloads() {
        assert!(parse_usage_rate_limits(&json!({ "event": "result" })).is_empty());
        let limits = parse_usage_rate_limits(&json!({
            "command": { "data": { "groups": [
                { "name": "Gemini Models", "buckets": [
                    { "id": "x", "window": "monthly", "remaining_fraction": -0.5, "reset_time": "soon" },
                    { "window": "5h" }
                ] },
                { "name": "Empty" }
            ] } }
        }));
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].used_percent, 100);
        assert_eq!(limits[0].label, "Gemini · monthly");
        assert_eq!(limits[0].resets_at, None);
        assert_eq!(limits[0].window_minutes, None);
    }
}
