use std::{
    collections::HashMap,
    io::{BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    acp_bridge::read_bounded_line,
    codex_bridge::{CodexModelOption, CodexReasoningEffortOption, CodexThreadModelSettings},
    domain::{
        AccessMode, AgentKind, AgentRateLimit, HookEvent, HookEventKind, PermissionProfile,
        SessionActivity, SessionControlOrigin, SessionModelOverride, SessionSource,
    },
    event_server,
    state::{now_millis, AppState},
};

pub const PERMISSION_DEFAULT: &str = "agy_default";
pub const PERMISSION_ACCEPT_EDITS: &str = "agy_accept_edits";
pub const PERMISSION_PLAN: &str = "agy_plan";
pub const PERMISSION_ALLOW_ALL: &str = "agy_allow_all";
pub const PROBE_ENV: &str = "LUME_ANTIGRAVITY_PROBE";

const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(10);
const MODEL_LIST_CACHE_TTL: Duration = Duration::from_secs(300);
const USAGE_TIMEOUT: Duration = Duration::from_secs(15);
const CLI_EFFORT_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const VARIANT_EFFORTS: [(&str, &[&str]); 6] = [
    ("extra-low", &["extra low", "extra-low"]),
    ("low", &["low"]),
    ("medium", &["medium"]),
    ("high", &["high"]),
    ("xhigh", &["xhigh", "extra high", "extra-high"]),
    ("max", &["max"]),
];

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
    pub(crate) fn environment_roots(&self) -> Vec<(String, u32)> {
        self.sessions
            .lock()
            .map(|sessions| {
                sessions
                    .iter()
                    .map(|(id, runtime)| (id.clone(), runtime.child.id()))
                    .collect()
            })
            .unwrap_or_default()
    }

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
        let (cli_model, cli_effort) = self.resolve_cli_selection(model, effort);
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
        if let Some(model) = cli_model.as_deref() {
            command.args(["--model", model]);
        }
        if let Some(effort) = cli_effort.as_deref() {
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
                model: cli_model,
                effort: cli_effort,
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
        if resume_id.is_some() {
            let already_loaded = self
                .state
                .connected_session(&format!("antigravity-stream:{id}"))
                .is_ok_and(|session| {
                    session
                        .activities
                        .iter()
                        .any(|activity| matches!(activity.kind.as_str(), "prompt" | "message"))
                });
            if !already_loaded {
                let (activities, last_response) =
                    crate::integrations::antigravity_resume_activities(&id);
                event.activities = activities;
                event.last_response = last_response;
            }
        }
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
        let resolved_override = (model_override.is_some() || effort_override.is_some())
            .then(|| self.resolve_cli_selection(model_override.as_deref(), effort_override.as_deref()));
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
                    let (requested_model, requested_effort) = resolved_override
                        .unwrap_or_else(|| (session.model.clone(), session.effort.clone()));
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
                None => {
                    let (requested_model, requested_effort) =
                        resolved_override.unwrap_or((None, None));
                    Some((
                        requested_model,
                        requested_effort,
                        permission_override.unwrap_or_else(|| PERMISSION_DEFAULT.into()),
                    ))
                }
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
        let catalog = self.model_catalog()?;
        let (current_model, current_effort) = self
            .sessions
            .lock()
            .map_err(|_| "Antigravity session lock failed")?
            .get(id)
            .map(|session| (session.model.clone(), session.effort.clone()))
            .unwrap_or_default();
        Ok(catalog.settings_for(current_model.as_deref(), current_effort.as_deref()))
    }

    fn resolve_cli_selection(
        &self,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        let model = model.map(str::trim).filter(|model| !model.is_empty());
        let effort = effort.map(str::trim).filter(|effort| !effort.is_empty());
        let Some(model) = model else {
            return (None, effort.filter(|effort| is_effort(effort)).map(str::to_string));
        };
        if let Ok(catalog) = self.model_catalog() {
            return catalog.resolve_cli_arguments(Some(model), effort);
        }
        resolve_cli_arguments_without_catalog(model, effort)
    }

    fn model_catalog(&self) -> Result<ModelCatalog, String> {
        let listing = cached_model_listing()?;
        Ok(parse_model_catalog_data(
            &listing,
            default_model_label().as_deref(),
        ))
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

fn cached_model_listing() -> Result<String, String> {
    static CACHE: OnceLock<Mutex<Option<(Instant, String)>>> = OnceLock::new();
    static PROBES: Mutex<()> = Mutex::new(());
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let cached = || {
        cache
            .lock()
            .ok()
            .and_then(|entry| entry.clone())
            .filter(|(created, _)| created.elapsed() < MODEL_LIST_CACHE_TTL)
            .map(|(_, listing)| listing)
    };
    if let Some(listing) = cached() {
        return Ok(listing);
    }
    let _probe = PROBES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(listing) = cached() {
        return Ok(listing);
    }
    let listing = query_model_listing()?;
    if let Ok(mut entry) = cache.lock() {
        *entry = Some((Instant::now(), listing.clone()));
    }
    Ok(listing)
}

fn query_model_listing() -> Result<String, String> {
    let mut command = crate::executables::command("agy")?;
    command
        .args(["models"])
        .env(PROBE_ENV, "1")
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
    let listing = String::from_utf8_lossy(&output.stdout).into_owned();
    if parse_model_lines(&listing).is_empty() {
        return Err("A CLI do Antigravity não retornou modelos disponíveis".into());
    }
    Ok(listing)
}

/// Quota windows of the `agy` CLI, read from `/usage` (zero tokens).
pub fn fetch_rate_limits() -> Result<Vec<AgentRateLimit>, String> {
    let mut command = crate::executables::command("agy")?;
    command
        .args(["-p", "/usage", "--output-format", "stream-json"])
        .env(PROBE_ENV, "1")
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

#[derive(Clone, Debug)]
struct ModelCatalog {
    options: Vec<CodexModelOption>,
    /// `(family_model_id, effort) -> exact CLI --model slug` from `agy models`.
    variant_slugs: HashMap<(String, String), String>,
    /// `raw_cli_slug -> (family_model_id, effort)` so a running CLI slug maps back to its family.
    raw_to_family: HashMap<String, (String, Option<String>)>,
}

struct FamilyBuilder {
    id: String,
    display_name: String,
    standalone_slug: Option<String>,
    first_effort: Option<String>,
    variants: Vec<(&'static str, String)>,
}

impl ModelCatalog {
    fn settings_for(
        &self,
        current_model: Option<&str>,
        current_effort: Option<&str>,
    ) -> CodexThreadModelSettings {
        let current_model = current_model.map(str::trim).filter(|model| !model.is_empty());
        let current_effort = current_effort
            .map(str::trim)
            .filter(|effort| !effort.is_empty())
            .map(str::to_string);
        let (model, reasoning_effort) = match current_model {
            Some(raw) => {
                if let Some((family, variant_effort)) = self.raw_to_family.get(raw) {
                    (
                        family.clone(),
                        current_effort.or_else(|| variant_effort.clone()),
                    )
                } else if let Some(option) = self.options.iter().find(|item| item.model == raw) {
                    let effort = current_effort.or_else(|| {
                        (!option.default_reasoning_effort.is_empty())
                            .then(|| option.default_reasoning_effort.clone())
                    });
                    (option.model.clone(), effort)
                } else {
                    (raw.to_string(), current_effort)
                }
            }
            None => {
                let default_option = self.options.iter().find(|option| option.is_default);
                let model = default_option
                    .map(|option| option.model.clone())
                    .unwrap_or_default();
                let effort = current_effort.or_else(|| {
                    default_option.and_then(|option| {
                        (!option.default_reasoning_effort.is_empty())
                            .then(|| option.default_reasoning_effort.clone())
                    })
                });
                (model, effort)
            }
        };
        CodexThreadModelSettings {
            model,
            reasoning_effort,
            service_tier: None,
            models: self.options.clone(),
            session_modes: None,
        }
    }

    fn resolve_cli_arguments(
        &self,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        let model = model.map(str::trim).filter(|model| !model.is_empty());
        let effort = effort.map(str::trim).filter(|effort| !effort.is_empty());
        let Some(model) = model else {
            return (
                None,
                effort.filter(|effort| is_effort(effort)).map(str::to_string),
            );
        };
        if self.raw_to_family.contains_key(model) {
            return (Some(model.to_string()), None);
        }
        if let Some(option) = self.options.iter().find(|option| option.model == model) {
            if option.supported_reasoning_efforts.is_empty() {
                return (Some(model.to_string()), None);
            }
            let effective_effort = effort
                .filter(|candidate| {
                    option
                        .supported_reasoning_efforts
                        .iter()
                        .any(|supported| supported.value == *candidate)
                })
                .or_else(|| {
                    (!option.default_reasoning_effort.is_empty())
                        .then_some(option.default_reasoning_effort.as_str())
                });
            if let Some(selected_effort) = effective_effort {
                if let Some(slug) = self
                    .variant_slugs
                    .get(&(model.to_string(), selected_effort.to_string()))
                {
                    return (Some(slug.clone()), None);
                }
            }
        }
        resolve_cli_arguments_without_catalog(model, effort)
    }
}

fn resolve_cli_arguments_without_catalog(
    model: &str,
    effort: Option<&str>,
) -> (Option<String>, Option<String>) {
    if split_slug_effort(model).is_some() {
        return (Some(model.to_string()), None);
    }
    (
        Some(model.to_string()),
        effort.filter(|effort| is_effort(effort)).map(str::to_string),
    )
}

fn split_slug_effort(slug: &str) -> Option<(&str, &'static str)> {
    for (key, _) in VARIANT_EFFORTS {
        if slug.len() > key.len() + 1 {
            let split = slug.len() - key.len() - 1;
            if slug.as_bytes().get(split) == Some(&b'-')
                && slug[split + 1..].eq_ignore_ascii_case(key)
            {
                return Some((&slug[..split], key));
            }
        }
    }
    None
}

fn split_label_effort(label: &str) -> (&str, Option<&'static str>) {
    let trimmed = label.trim();
    if let Some((prefix, rest)) = trimmed.rsplit_once('(') {
        if let Some(tag) = rest.strip_suffix(')').map(str::trim) {
            let base = prefix.trim_end();
            if !base.is_empty() {
                for (key, aliases) in VARIANT_EFFORTS {
                    if aliases.iter().any(|alias| tag.eq_ignore_ascii_case(alias)) {
                        return (base, Some(key));
                    }
                }
            }
        }
    }
    (trimmed, None)
}

/// Splits `<base>-<effort>` + `<Base> (<Effort>)` rows from `agy models` into their model family
/// and effort tier. Models whose slug or label has no effort tier (such as `(Thinking)`) stay whole.
fn split_effort_variant(slug: &str, display_name: &str) -> Option<(String, String, &'static str)> {
    let (base_slug, slug_effort) = split_slug_effort(slug)?;
    let (base_label, label_effort) = split_label_effort(display_name);
    (label_effort == Some(slug_effort) && !base_slug.is_empty() && !base_label.is_empty()).then(
        || (base_slug.to_string(), base_label.to_string(), slug_effort),
    )
}

fn effort_rank(effort: &str) -> usize {
    VARIANT_EFFORTS
        .iter()
        .position(|(key, _)| *key == effort)
        .unwrap_or(VARIANT_EFFORTS.len())
}

fn parse_model_catalog_data(output: &str, default_label: Option<&str>) -> ModelCatalog {
    let default_label = default_label
        .map(str::trim)
        .filter(|label| !label.is_empty());
    let mut families: Vec<FamilyBuilder> = Vec::new();
    let mut variant_slugs = HashMap::new();
    let mut raw_to_family = HashMap::new();

    for (slug, display_name) in parse_model_lines(output) {
        if let Some((base_slug, base_label, effort)) = split_effort_variant(&slug, &display_name) {
            variant_slugs.insert((base_slug.clone(), effort.to_string()), slug.clone());
            raw_to_family.insert(slug.clone(), (base_slug.clone(), Some(effort.to_string())));
            if let Some(existing) = families.iter_mut().find(|family| family.id == base_slug) {
                if existing.first_effort.is_none() {
                    existing.first_effort = Some(effort.to_string());
                }
                if !existing.variants.iter().any(|(item, _)| *item == effort) {
                    existing.variants.push((effort, slug));
                }
            } else {
                families.push(FamilyBuilder {
                    id: base_slug,
                    display_name: base_label,
                    standalone_slug: None,
                    first_effort: Some(effort.to_string()),
                    variants: vec![(effort, slug)],
                });
            }
        } else {
            raw_to_family.insert(slug.clone(), (slug.clone(), None));
            if let Some(existing) = families.iter_mut().find(|family| family.id == slug) {
                existing.standalone_slug = Some(slug);
            } else {
                families.push(FamilyBuilder {
                    id: slug.clone(),
                    display_name,
                    standalone_slug: Some(slug),
                    first_effort: None,
                    variants: Vec::new(),
                });
            }
        }
    }

    let parsed_default = default_label.map(split_label_effort);
    let mut matched_default = false;
    let mut options = Vec::with_capacity(families.len() + 1);

    for mut family in families {
        family
            .variants
            .sort_by_key(|(effort, _)| effort_rank(effort));
        let supported_reasoning_efforts = family
            .variants
            .iter()
            .map(|(effort, _)| CodexReasoningEffortOption {
                value: (*effort).into(),
                description: format!("--effort {effort}"),
            })
            .collect::<Vec<_>>();

        let is_default_family = !matched_default
            && match (default_label, parsed_default) {
                (Some(raw), _) if family.variants.is_empty() => {
                    family.display_name.eq_ignore_ascii_case(raw)
                }
                (Some(raw), Some((base_label, Some(default_effort)))) => {
                    (family.display_name.eq_ignore_ascii_case(base_label)
                        || family.display_name.eq_ignore_ascii_case(raw))
                        && family
                            .variants
                            .iter()
                            .any(|(effort, _)| *effort == default_effort)
                }
                (Some(raw), Some((base_label, None))) => {
                    family.display_name.eq_ignore_ascii_case(raw)
                        || (family.standalone_slug.is_some()
                            && family.display_name.eq_ignore_ascii_case(base_label))
                }
                _ => false,
            };

        if is_default_family {
            matched_default = true;
        }

        let default_reasoning_effort = if is_default_family {
            parsed_default
                .and_then(|(_, effort)| effort)
                .filter(|target| family.variants.iter().any(|(effort, _)| effort == target))
                .map(str::to_string)
                .or_else(|| family.first_effort.clone())
                .unwrap_or_default()
        } else {
            family.first_effort.unwrap_or_default()
        };

        options.push(CodexModelOption {
            model: family.id,
            display_name: family.display_name,
            description: if is_default_family {
                "Padrão da CLI do Antigravity".into()
            } else {
                "Disponível na CLI do Antigravity".into()
            },
            is_default: is_default_family,
            default_reasoning_effort,
            supported_reasoning_efforts,
        });
    }

    if !matched_default {
        options.insert(
            0,
            CodexModelOption {
                model: String::new(),
                display_name: default_label.unwrap_or("Padrão do Antigravity").into(),
                description: if default_label.is_some() {
                    "Padrão da CLI do Antigravity".into()
                } else {
                    "Usa o modelo padrão configurado na CLI do Antigravity".into()
                },
                is_default: true,
                default_reasoning_effort: String::new(),
                supported_reasoning_efforts: cli_effort_options(),
            },
        );
    }

    options.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });

    ModelCatalog {
        options,
        variant_slugs,
        raw_to_family,
    }
}

#[cfg(test)]
fn parse_model_catalog(output: &str, default_label: Option<&str>) -> Vec<CodexModelOption> {
    parse_model_catalog_data(output, default_label).options
}

pub fn apply_model_override(
    settings: &mut CodexThreadModelSettings,
    selected: &SessionModelOverride,
) {
    if selected.model.is_none() && selected.reasoning_effort.is_none() {
        return;
    }
    let mut fallback_effort: Option<&str> = None;
    if let Some(raw_model) = selected.model.as_deref().map(str::trim) {
        if raw_model.is_empty() {
            if settings.models.iter().any(|option| option.model.is_empty()) {
                settings.model = String::new();
            } else if let Some(default_option) = settings.models.iter().find(|option| option.is_default)
            {
                settings.model = default_option.model.clone();
            }
        } else if settings.models.iter().any(|option| option.model == raw_model) {
            settings.model = raw_model.to_string();
        } else if let Some((base_slug, variant_effort)) = split_slug_effort(raw_model) {
            if settings.models.iter().any(|option| option.model == base_slug) {
                settings.model = base_slug.to_string();
                fallback_effort = Some(variant_effort);
            } else {
                settings.model = raw_model.to_string();
            }
        } else {
            settings.model = raw_model.to_string();
        }
    }

    let Some(option) = settings
        .models
        .iter()
        .find(|option| option.model == settings.model)
    else {
        if let Some(effort) = selected
            .reasoning_effort
            .as_deref()
            .map(str::trim)
            .or(fallback_effort)
        {
            settings.reasoning_effort = is_effort(effort).then(|| effort.to_string());
        }
        return;
    };

    if option.supported_reasoning_efforts.is_empty() {
        settings.reasoning_effort = None;
        return;
    }

    let requested = selected
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|effort| !effort.is_empty() || option.model.is_empty())
        .or(fallback_effort);
    if let Some(effort) = requested.filter(|candidate| {
        option
            .supported_reasoning_efforts
            .iter()
            .any(|supported| supported.value == *candidate)
    }) {
        settings.reasoning_effort = (!effort.is_empty()).then(|| effort.to_string());
    } else {
        settings.reasoning_effort = (!option.default_reasoning_effort.is_empty())
            .then(|| option.default_reasoning_effort.clone());
    }
}

pub fn validate_model_selection(
    settings: &mut CodexThreadModelSettings,
    model: &str,
    effort: &str,
) -> Result<SessionModelOverride, String> {
    let model = model.trim();
    let effort = effort.trim();
    let option = settings
        .models
        .iter()
        .find(|option| option.model == model)
        .ok_or_else(|| "Esse modelo não está disponível na CLI do Antigravity".to_string())?
        .clone();

    if option.supported_reasoning_efforts.is_empty() {
        if !effort.is_empty() {
            return Err("Esse modelo não aceita ajuste de esforço na CLI do Antigravity".into());
        }
        settings.model = option.model.clone();
        settings.reasoning_effort = None;
        return Ok(SessionModelOverride {
            model: Some(option.model),
            reasoning_effort: Some(String::new()),
        });
    }

    let effective_effort = if effort.is_empty() && !option.model.is_empty() {
        option.default_reasoning_effort.as_str()
    } else {
        effort
    };
    if !option
        .supported_reasoning_efforts
        .iter()
        .any(|supported| supported.value == effective_effort)
    {
        return Err("Esse esforço não é aceito pela CLI do Antigravity".into());
    }
    settings.model = option.model.clone();
    settings.reasoning_effort = (!effective_effort.is_empty()).then(|| effective_effort.to_string());
    Ok(SessionModelOverride {
        model: Some(option.model),
        reasoning_effort: Some(effective_effort.to_string()),
    })
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

/// The CLI default comes first (empty value) so the slider can return to it when the active
/// model comes from CLI settings outside the parsed catalog.
fn cli_effort_options() -> Vec<CodexReasoningEffortOption> {
    std::iter::once(CodexReasoningEffortOption {
        value: String::new(),
        description: "Usa o esforço padrão configurado na CLI do Antigravity".into(),
    })
    .chain(
        CLI_EFFORT_LEVELS
            .iter()
            .map(|effort| CodexReasoningEffortOption {
                value: (*effort).into(),
                description: format!("--effort {effort}"),
            }),
    )
    .collect()
}

pub fn is_effort(effort: &str) -> bool {
    CLI_EFFORT_LEVELS.contains(&effort)
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
    let state_name = step["state"].as_str().unwrap_or_default();
    let step_error = step["error_message"]
        .as_str()
        .or_else(|| tool_info["error"].as_str())
        .filter(|error| !error.trim().is_empty());
    let failed = matches!(state_name, "ERROR" | "FAILED" | "STATE_ERROR") || step_error.is_some();
    let done = matches!(state_name, "DONE" | "STATE_DONE");
    let status = if failed {
        "failed"
    } else if done {
        "completed"
    } else {
        "running"
    };
    let output_value = tool_info.get("output").filter(|value| !value.is_null());
    let output_text = output_value.and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        _ => serde_json::to_string_pretty(value).ok(),
    });
    let presentation = crate::adapters::antigravity_tool_presentation(
        tool_name,
        params.is_object().then_some(params),
        Some(step),
        output_text.as_deref(),
        output_value,
        step_error,
        done || failed,
    );
    let step_index = step["step_index"].as_u64().unwrap_or(0);
    if let Some(child_id) = step
        .pointer("/subagent_info/conversation_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        record_subagent_conversation(id, step_index, child_id);
    }
    let activity_id = if presentation.kind == "subagent" {
        format!("antigravity:{id}:subagent:{step_index}")
    } else {
        format!("agy-tool:{id}:{step_index}")
    };
    SessionActivity {
        id: activity_id,
        kind: presentation.kind.into(),
        title: presentation.title.chars().take(180).collect(),
        detail: presentation
            .detail
            .map(|text| text.chars().take(presentation.detail_limit).collect()),
        status: status.into(),
        created_at: now_millis(),
        files: presentation.files,
        attachments: Vec::new(),
        append_detail: false,
    }
}

fn is_safe_stream_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn subagent_registry() -> &'static Mutex<HashMap<(String, u64), String>> {
    static REGISTRY: OnceLock<Mutex<HashMap<(String, u64), String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn record_subagent_conversation(parent_id: &str, step_index: u64, child_id: &str) {
    if !is_safe_stream_id(parent_id) || !is_safe_stream_id(child_id) {
        return;
    }
    if let Ok(mut map) = subagent_registry().lock() {
        if map.len() >= 128 {
            map.clear();
        }
        map.insert((parent_id.to_string(), step_index), child_id.to_string());
    }
}

pub(crate) fn subagent_conversation_id(parent_id: &str, step_index: u64) -> Option<String> {
    subagent_registry()
        .lock()
        .ok()
        .and_then(|map| map.get(&(parent_id.to_string(), step_index)).cloned())
}

/// Copies attachments outside the project folder into Antigravity's own conversation scratch
/// directory when available, so `read_file` can inspect pasted screenshots and external files
/// without outside-workspace permission prompts.
pub fn stage_prompt_attachments(native_id: &str, cwd: &str, paths: &[String]) -> Vec<String> {
    if paths.is_empty() || !is_safe_stream_id(native_id) {
        return paths.to_vec();
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    let brain_root = home
        .as_deref()
        .map(|home| Path::new(home).join(".gemini/antigravity-cli/brain").join(native_id));
    stage_prompt_attachments_with_brain(paths, Path::new(cwd), brain_root.as_deref())
}

fn stage_prompt_attachments_with_brain(
    paths: &[String],
    cwd: &Path,
    brain_conversation_dir: Option<&Path>,
) -> Vec<String> {
    let normalize = |path: &Path| {
        let text = path
            .to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches("//?/")
            .trim_end_matches('/')
            .to_string();
        if cfg!(windows) {
            text.to_ascii_lowercase()
        } else {
            text
        }
    };
    let normalized_cwd = normalize(cwd);
    let scratch_dir = brain_conversation_dir
        .filter(|dir| dir.is_dir())
        .map(|dir| dir.join("scratch/lume-attachments"));
    paths
        .iter()
        .enumerate()
        .map(|(index, raw_path)| {
            let source = Path::new(raw_path);
            let normalized_source = normalize(source);
            let in_workspace = !normalized_cwd.is_empty()
                && (normalized_source == normalized_cwd
                    || normalized_source.starts_with(&format!("{normalized_cwd}/")));
            if in_workspace {
                return raw_path.clone();
            }
            let Some(scratch_dir) = scratch_dir.as_deref() else {
                return raw_path.clone();
            };
            if std::fs::create_dir_all(scratch_dir).is_err() {
                return raw_path.clone();
            }
            let file_name = source
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("attachment")
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            let dest = scratch_dir.join(format!("{}-{index}-{file_name}", now_millis()));
            match std::fs::copy(source, &dest) {
                Ok(_) => dest.to_string_lossy().to_string(),
                Err(_) => raw_path.clone(),
            }
        })
        .collect()
}

fn step_update_activity(
    id: &str,
    turn_number: u64,
    step: &Value,
    active_file_diffs: &mut HashMap<String, String>,
) -> Option<SessionActivity> {
    let step_index = step["step_index"].as_u64().unwrap_or(0);
    match step["step_type"].as_str() {
        Some("agent_response") => {
            let done = matches!(step["state"].as_str(), Some("DONE" | "STATE_DONE"));
            if let Some(delta) = step["text_delta"].as_str().filter(|text| !text.is_empty()) {
                return Some(SessionActivity {
                    id: format!("agy-response:{id}:{turn_number}:{step_index}"),
                    kind: "message".into(),
                    title: "Antigravity".into(),
                    detail: Some(delta.chars().take(32 * 1024).collect()),
                    status: if done { "completed" } else { "running" }.into(),
                    created_at: now_millis(),
                    files: Vec::new(),
                    attachments: Vec::new(),
                    append_detail: true,
                });
            }
            if let Some(full_text) = step["text"].as_str().filter(|text| !text.is_empty()) {
                return Some(SessionActivity {
                    id: format!("agy-response:{id}:{turn_number}:{step_index}"),
                    kind: "message".into(),
                    title: "Antigravity".into(),
                    detail: Some(full_text.chars().take(32 * 1024).collect()),
                    status: if done { "completed" } else { "running" }.into(),
                    created_at: now_millis(),
                    files: Vec::new(),
                    attachments: Vec::new(),
                    append_detail: false,
                });
            }
            None
        }
        Some("compaction") => {
            if !matches!(step["state"].as_str(), Some("DONE" | "STATE_DONE")) {
                return None;
            }
            Some(SessionActivity {
                id: format!("agy-compact:{id}:{step_index}"),
                kind: "compact".into(),
                title: "Context compacted".into(),
                detail: None,
                status: "completed".into(),
                created_at: now_millis(),
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            })
        }
        Some("tool") => {
            let tool_name = step["tool_name"].as_str().unwrap_or("Ferramenta");
            let params = &step["tool_info"]["parameters"];
            if crate::adapters::is_antigravity_internal_tool(
                tool_name,
                params.is_object().then_some(params),
            ) {
                return None;
            }
            let mut activity = tool_activity(id, step);
            if activity.kind == "file" {
                let has_diff = activity
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.starts_with("diff --git "));
                if has_diff && activity.status == "running" {
                    if active_file_diffs.len() >= 64 {
                        active_file_diffs.clear();
                    }
                    active_file_diffs
                        .insert(activity.id.clone(), activity.detail.clone().unwrap_or_default());
                } else if activity.status != "running" {
                    let previous_diff = active_file_diffs.remove(&activity.id);
                    if !has_diff {
                        if let Some(diff) = previous_diff {
                            activity.detail = Some(diff);
                        }
                    }
                }
            }
            Some(activity)
        }
        _ => None,
    }
}

fn result_token_usage_activity(
    id: &str,
    turn_number: u64,
    message: &Value,
) -> Option<SessionActivity> {
    let result = &message["result"];
    let usage = result.get("usage").or_else(|| message.get("usage"))?;
    let u64_field = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| usage.get(*key).and_then(Value::as_u64))
    };
    let input_tokens = u64_field(&["input_tokens", "inputTokens"]).unwrap_or(0);
    let output_tokens = u64_field(&["output_tokens", "outputTokens"]).unwrap_or(0);
    let cache_read = u64_field(&["cache_read_tokens", "cacheReadTokens", "cached_tokens"]);
    let total_tokens = u64_field(&["total_tokens", "totalTokens"])
        .filter(|total| *total > 0)
        .unwrap_or(input_tokens + output_tokens);
    if total_tokens == 0 && input_tokens + output_tokens == 0 {
        return None;
    }
    let context_tokens = u64_field(&["context_tokens", "contextTokens"])
        .unwrap_or_else(|| input_tokens + cache_read.unwrap_or(0) + output_tokens);
    let context_window = u64_field(&["context_window", "contextWindow"]);
    let model = result
        .get("model")
        .or_else(|| usage.get("model"))
        .or_else(|| message.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty());
    let created_at = now_millis();
    let mut detail = json!({
        "turnId": format!("agy-stream-turn-{turn_number}"),
        "totalTokens": input_tokens + output_tokens,
        "inputTokens": input_tokens,
        "outputTokens": output_tokens,
        "contextTokens": context_tokens,
        "createdAt": created_at,
    });
    if detail["totalTokens"].as_u64() == Some(0) {
        detail["totalTokens"] = json!(total_tokens);
    }
    if let Some(cache_read) = cache_read {
        detail["cacheReadTokens"] = json!(cache_read);
    }
    if let Some(window) = context_window.filter(|window| *window > 0) {
        detail["contextWindow"] = json!(window);
    }
    if let Some(model) = model {
        detail["model"] = json!(model);
    }
    Some(SessionActivity {
        id: format!("agy-token:{id}:stream:{turn_number}"),
        kind: "token_usage".into(),
        title: "Uso de tokens".into(),
        detail: Some(detail.to_string()),
        status: "completed".into(),
        created_at,
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    })
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
    let mut active_file_diffs: HashMap<String, String> = HashMap::new();
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
                let Some(activity) = step_update_activity(
                    id,
                    turn.load(Ordering::SeqCst),
                    step,
                    &mut active_file_diffs,
                ) else {
                    continue;
                };
                let mut event = base_event(id, HookEventKind::Activity);
                event.activity = Some(activity);
                let _ = event_server::publish_event(&state, &app, event);
            }
            Some("result") => {
                let Some(id) = id.as_deref() else { continue };
                active.store(false, Ordering::SeqCst);
                active_file_diffs.clear();
                let result = &message["result"];
                let status = result["status"].as_str().unwrap_or("ERROR");
                let event_kind = match status {
                    "SUCCESS" => HookEventKind::Completed,
                    "CANCELED" | "INTERRUPTED" | "WAITING" => HookEventKind::WaitingForInput,
                    _ => HookEventKind::Failed,
                };
                let mut event = base_event(id, event_kind);
                event.last_response = result["response"].as_str().map(str::to_string);
                if let Some(usage_activity) =
                    result_token_usage_activity(id, turn.load(Ordering::SeqCst), &message)
                {
                    event.activities.push(usage_activity);
                }
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

    const AGY_GEMINI_4_MODELS: &str = "Available models:\n\
        gemini-4-carbon-high\tGemini 4 Carbon (High)\n\
        gemini-4-carbon-medium\tGemini 4 Carbon (Medium)\n\
        gemini-4-carbon-low\tGemini 4 Carbon (Low)\n\
        gemini-4-hydrogen-xhigh\tGemini 4 Hydrogen (XHigh)\n\
        gemini-4-hydrogen-high\tGemini 4 Hydrogen (High)\n\
        gemini-4-hydrogen-medium\tGemini 4 Hydrogen (Medium)\n\
        gemini-4-hydrogen-low\tGemini 4 Hydrogen (Low)\n\
        claude-sonnet-5-5-max\tClaude Sonnet 5.5 (Max)\n\
        claude-sonnet-5-5-low\tClaude Sonnet 5.5 (Low)\n\
        claude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)\n";

    fn shown(models: &[CodexModelOption]) -> Vec<(&str, &str)> {
        models
            .iter()
            .map(|entry| (entry.model.as_str(), entry.display_name.as_str()))
            .collect()
    }

    fn efforts(option: &CodexModelOption) -> Vec<&str> {
        option
            .supported_reasoning_efforts
            .iter()
            .map(|effort| effort.value.as_str())
            .collect()
    }

    #[test]
    fn effort_variants_collapse_into_one_model_family_and_mark_the_cli_default() {
        let catalog = parse_model_catalog_data(AGY_MODELS, Some("Gemini 3.1 Pro (High)"));
        assert_eq!(
            shown(&catalog.options),
            [
                ("gemini-3.1-pro", "Gemini 3.1 Pro"),
                ("claude-sonnet-4-6", "Claude Sonnet 4.6 (Thinking)"),
            ],
            "Low and High become efforts of one Gemini 3.1 Pro model"
        );
        assert_eq!(catalog.options[0].description, "Padrão da CLI do Antigravity");
        assert!(catalog.options[0].is_default);
        assert_eq!(catalog.options[0].default_reasoning_effort, "high");
        assert_eq!(efforts(&catalog.options[0]), ["low", "high"]);
        assert!(
            efforts(&catalog.options[1]).is_empty(),
            "models without effort tiers do not advertise --effort"
        );
        let settings = catalog.settings_for(None, None);
        assert_eq!(settings.model, "gemini-3.1-pro");
        assert_eq!(settings.reasoning_effort.as_deref(), Some("high"));

        // Lookup ignores case and outer whitespace.
        let loose = parse_model_catalog(AGY_MODELS, Some("  gemini 3.1 PRO (low) "));
        assert_eq!(loose.len(), 2);
        assert_eq!(loose[0].model, "gemini-3.1-pro");
        assert_eq!(loose[0].default_reasoning_effort, "low");
    }

    #[test]
    fn gemini_4_and_claude_variants_order_efforts_low_to_max_and_resolve_to_cli_slugs() {
        let catalog = parse_model_catalog_data(AGY_GEMINI_4_MODELS, Some("Gemini 4 Carbon (Medium)"));
        assert_eq!(
            shown(&catalog.options),
            [
                ("gemini-4-carbon", "Gemini 4 Carbon"),
                ("claude-sonnet-4-6", "Claude Sonnet 4.6 (Thinking)"),
                ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
                ("gemini-4-hydrogen", "Gemini 4 Hydrogen"),
            ]
        );
        assert_eq!(catalog.options[0].default_reasoning_effort, "medium");
        assert_eq!(efforts(&catalog.options[0]), ["low", "medium", "high"]);
        assert_eq!(efforts(&catalog.options[1]), Vec::<&str>::new());
        assert_eq!(efforts(&catalog.options[2]), ["low", "max"]);
        assert_eq!(efforts(&catalog.options[3]), ["low", "medium", "high", "xhigh"]);

        // Launching resolves family + effort to the exact `agy models` slug without passing `--effort`.
        assert_eq!(
            catalog.resolve_cli_arguments(Some("gemini-4-carbon"), Some("low")),
            (Some("gemini-4-carbon-low".into()), None)
        );
        assert_eq!(
            catalog.resolve_cli_arguments(Some("gemini-4-hydrogen"), Some("xhigh")),
            (Some("gemini-4-hydrogen-xhigh".into()), None)
        );
        assert_eq!(
            catalog.resolve_cli_arguments(Some("claude-sonnet-4-6"), Some("high")),
            (Some("claude-sonnet-4-6".into()), None)
        );
        // Running sessions storing a raw variant slug map cleanly back to family + effort.
        let running = catalog.settings_for(Some("gemini-4-hydrogen-xhigh"), None);
        assert_eq!(running.model, "gemini-4-hydrogen");
        assert_eq!(running.reasoning_effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn without_a_known_default_the_generic_entry_stays_above_grouped_families() {
        for label in [None, Some(""), Some("   ")] {
            let models = parse_model_catalog(AGY_MODELS, label);
            assert_eq!(
                shown(&models),
                [
                    ("", "Padrão do Antigravity"),
                    ("claude-sonnet-4-6", "Claude Sonnet 4.6 (Thinking)"),
                    ("gemini-3.1-pro", "Gemini 3.1 Pro"),
                ]
            );
            assert_eq!(
                models[0].description,
                "Usa o modelo padrão configurado na CLI do Antigravity"
            );
            assert_eq!(
                efforts(&models[0]),
                ["", "low", "medium", "high", "xhigh", "max"]
            );
        }
    }

    #[test]
    fn a_default_the_list_does_not_have_is_still_named() {
        let models = parse_model_catalog(AGY_MODELS, Some("Gemini 9 Ultra"));
        assert_eq!(models[0].model, "");
        assert_eq!(models[0].display_name, "Gemini 9 Ultra");
        assert_eq!(models.len(), 3, "the grouped families remain below it");
    }

    #[test]
    fn a_catalog_of_one_model_is_not_an_empty_catalog() {
        let only = "gemini-3.1-pro-high\tGemini 3.1 Pro (High)\n";
        let models = parse_model_catalog(only, Some("Gemini 3.1 Pro (High)"));
        assert_eq!(models.len(), 1, "only the named default family remains");
        assert_eq!(models[0].model, "gemini-3.1-pro");
        assert_eq!(models[0].default_reasoning_effort, "high");
        assert_eq!(
            parse_model_lines(only).len(),
            1,
            "so the CLI did list a model"
        );
        assert!(parse_model_lines("Available models:\n").is_empty());
    }

    #[test]
    fn overrides_and_selection_validation_enforce_each_models_efforts() {
        let catalog = parse_model_catalog_data(AGY_GEMINI_4_MODELS, Some("Gemini 4 Carbon (High)"));
        let mut settings = catalog.settings_for(None, None);
        let saved = validate_model_selection(&mut settings, "gemini-4-hydrogen", "xhigh").unwrap();
        assert_eq!(saved.model.as_deref(), Some("gemini-4-hydrogen"));
        assert_eq!(saved.reasoning_effort.as_deref(), Some("xhigh"));
        assert!(validate_model_selection(&mut settings, "gemini-4-carbon", "xhigh").is_err());
        assert!(validate_model_selection(&mut settings, "claude-sonnet-4-6", "low").is_err());
        let plain = validate_model_selection(&mut settings, "claude-sonnet-4-6", "").unwrap();
        assert_eq!(plain.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(settings.reasoning_effort, None);

        // Legacy overrides storing a raw variant slug still restore family + effort.
        let mut legacy = catalog.settings_for(None, None);
        apply_model_override(
            &mut legacy,
            &SessionModelOverride {
                model: Some("gemini-4-carbon-low".into()),
                reasoning_effort: None,
            },
        );
        assert_eq!(legacy.model, "gemini-4-carbon");
        assert_eq!(legacy.reasoning_effort.as_deref(), Some("low"));
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
    fn cli_effort_levels_match_current_antigravity_cli_flags() {
        for valid in ["low", "medium", "high", "xhigh", "max"] {
            assert!(is_effort(valid), "{valid}");
        }
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
        assert_eq!(activity.kind, "test");
        assert_eq!(activity.title, "cargo test");
        assert_eq!(
            activity.detail.as_deref(),
            Some("test result: ok. 12 passed")
        );
        assert_eq!(activity.status, "completed");

        let shell = tool_activity(
            "conversation-1",
            &json!({
                "step_type": "tool",
                "step_index": 5,
                "state": "DONE",
                "tool_name": "run_command",
                "tool_info": {
                    "parameters": { "CommandLine": "git status -s", "toolSummary": "Git status" },
                    "output": " M src/main.rs",
                },
            }),
        );
        assert_eq!(shell.kind, "command");
        assert_eq!(shell.title, "git status -s");
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

        let edit = tool_activity(
            "conversation-1",
            &json!({
                "step_index": 7,
                "state": "DONE",
                "tool_name": "edit_file",
                "tool_info": {
                    "parameters": {
                        "Path": "G:\\repo\\src\\app.ts",
                        "Target": "const oldVal = 1;",
                        "Replacement": "const newVal = 2;\nconst extra = true;",
                    },
                    "output": "Made 1 replacement in G:\\repo\\src\\app.ts (+2/-1 lines).",
                },
            }),
        );
        assert_eq!(edit.kind, "file");
        assert_eq!(edit.title, "G:\\repo\\src\\app.ts");
        assert_eq!(edit.files, ["G:\\repo\\src\\app.ts"]);
        let diff = edit.detail.expect("unified diff synthesized from edit_file");
        assert!(diff.starts_with("diff --git a/G:/repo/src/app.ts b/G:/repo/src/app.ts\n"), "{diff}");
        assert!(diff.contains("-const oldVal = 1;\n+const newVal = 2;\n+const extra = true;\n"), "{diff}");

        let created = tool_activity(
            "conversation-1",
            &json!({
                "step_index": 8,
                "state": "DONE",
                "tool_name": "write_file",
                "tool_info": {
                    "parameters": {
                        "Path": "/repo/src/new_module.ts",
                        "Content": "export const ready = true;\n",
                    },
                    "output": "Created /repo/src/new_module.ts",
                },
            }),
        );
        assert_eq!(created.kind, "file");
        assert_eq!(created.files, ["/repo/src/new_module.ts"]);
        let created_diff = created.detail.expect("new file unified diff");
        assert!(created_diff.contains("new file mode 100644"), "{created_diff}");
        assert!(created_diff.contains("+export const ready = true;"), "{created_diff}");

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
        assert_eq!(activity.title, "Read · /repo/README.md");
        assert!(activity.files.is_empty());

        let read_file = tool_activity(
            "conversation-1",
            &json!({
                "state": "DONE",
                "tool_name": "read_file",
                "tool_info": {
                    "parameters": { "Path": "G:\\repo\\src\\main.rs", "StartLine": 1, "EndLine": 40 },
                    "output": "1: fn main() {}",
                },
            }),
        );
        assert_eq!(read_file.kind, "tool");
        assert_eq!(read_file.title, "Read · G:\\repo\\src\\main.rs");
        assert_eq!(read_file.detail.as_deref(), Some("1: fn main() {}"));
        assert!(read_file.files.is_empty());
    }

    #[test]
    fn subagents_mcp_internal_tools_and_stream_steps_match_claude_presentation() {
        let mut active_diffs = HashMap::new();
        let sub = step_update_activity(
            "conversation-1",
            1,
            &json!({
                "step_type": "tool",
                "step_index": 11,
                "state": "ACTIVE",
                "tool_name": "invoke_subagent",
                "subagent_info": { "conversation_id": "sub-conv-99" },
                "tool_info": {
                    "parameters": {
                        "TypeName": "research",
                        "Role": "Codebase Researcher",
                        "Prompt": "Inspect adapters",
                    }
                }
            }),
            &mut active_diffs,
        )
        .expect("subagent activity");
        assert_eq!(sub.id, "antigravity:conversation-1:subagent:11");
        assert_eq!(sub.kind, "subagent");
        assert_eq!(sub.title, "Codebase Researcher");
        assert_eq!(
            subagent_conversation_id("conversation-1", 11).as_deref(),
            Some("sub-conv-99")
        );

        let mcp = step_update_activity(
            "conversation-1",
            1,
            &json!({
                "step_type": "tool",
                "step_index": 12,
                "state": "DONE",
                "tool_name": "mcp__github__list_issues",
                "tool_info": {
                    "parameters": { "repo": "Lume-agents/Lume" },
                    "output": { "total": 2 }
                }
            }),
            &mut active_diffs,
        )
        .expect("mcp activity");
        assert_eq!(mcp.kind, "tool");
        assert_eq!(mcp.title, "mcp__github__list_issues");
        let parsed: Value = serde_json::from_str(mcp.detail.as_deref().unwrap()).unwrap();
        assert_eq!(parsed["input"]["repo"], "Lume-agents/Lume");
        assert_eq!(parsed["result"]["total"], 2);

        for internal in ["send_message", "schedule", "define_subagent", "manage_subagents"] {
            assert!(
                step_update_activity(
                    "conversation-1",
                    1,
                    &json!({
                        "step_type": "tool",
                        "step_index": 13,
                        "state": "DONE",
                        "tool_name": internal,
                        "tool_info": { "parameters": {} }
                    }),
                    &mut active_diffs,
                )
                .is_none(),
                "{internal} should stay out of project trace"
            );
        }

        // Separate agent_response steps in the same turn keep distinct step-indexed IDs.
        let first_msg = step_update_activity(
            "conversation-1",
            2,
            &json!({
                "step_type": "agent_response",
                "step_index": 1,
                "state": "ACTIVE",
                "text_delta": "Vou inspecionar o código."
            }),
            &mut active_diffs,
        )
        .unwrap();
        let second_msg = step_update_activity(
            "conversation-1",
            2,
            &json!({
                "step_type": "agent_response",
                "step_index": 4,
                "state": "DONE",
                "text_delta": "Correção aplicada."
            }),
            &mut active_diffs,
        )
        .unwrap();
        assert_ne!(first_msg.id, second_msg.id);
        assert_eq!(first_msg.id, "agy-response:conversation-1:2:1");
        assert_eq!(second_msg.id, "agy-response:conversation-1:2:4");
        assert_eq!(second_msg.status, "completed");

        let compact = step_update_activity(
            "conversation-1",
            2,
            &json!({
                "step_type": "compaction",
                "step_index": 5,
                "state": "DONE"
            }),
            &mut active_diffs,
        )
        .unwrap();
        assert_eq!(compact.kind, "compact");

        let usage = result_token_usage_activity(
            "conversation-1",
            2,
            &json!({
                "event": "result",
                "result": {
                    "status": "SUCCESS",
                    "model": "gemini-4-carbon-high",
                    "usage": {
                        "input_tokens": 1200,
                        "output_tokens": 350,
                        "cache_read_tokens": 18000
                    }
                }
            }),
        )
        .expect("token usage activity");
        assert_eq!(usage.kind, "token_usage");
        let usage_json: Value = serde_json::from_str(usage.detail.as_deref().unwrap()).unwrap();
        assert_eq!(usage_json["inputTokens"], 1200);
        assert_eq!(usage_json["outputTokens"], 350);
        assert_eq!(usage_json["totalTokens"], 1550);
        assert_eq!(usage_json["cacheReadTokens"], 18000);
        assert_eq!(usage_json["model"], "gemini-4-carbon-high");
    }

    #[test]
    fn external_attachments_stage_into_conversation_scratch_while_workspace_files_stay_put() {
        let temp = std::env::temp_dir().join(format!("lume-agy-attach-{}", now_millis()));
        let cwd = temp.join("project");
        let brain = temp.join("brain/conv-1");
        let outside = temp.join("clipboard");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&brain).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let inside_file = cwd.join("diagram.png");
        let outside_file = outside.join("paste.png");
        std::fs::write(&inside_file, b"inside").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();

        let staged = stage_prompt_attachments_with_brain(
            &[
                inside_file.to_string_lossy().to_string(),
                outside_file.to_string_lossy().to_string(),
            ],
            &cwd,
            Some(&brain),
        );
        assert_eq!(staged[0], inside_file.to_string_lossy());
        assert!(
            staged[1].contains("scratch") && staged[1].ends_with("paste.png"),
            "{}",
            staged[1]
        );
        assert_eq!(std::fs::read(&staged[1]).unwrap(), b"outside");
        let _ = std::fs::remove_dir_all(&temp);
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
