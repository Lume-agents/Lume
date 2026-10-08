use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use tauri::{AppHandle, Url};

use crate::{
    codex_bridge::{
        CodexModelOption, CodexReasoningEffortOption, CodexThreadModelSettings, SessionModeOption,
        SessionModeSettings,
    },
    domain::{
        AccessMode, AgentKind, HookEvent, HookEventKind, PermissionAction, PermissionProfile,
        PermissionRequest, SessionActivity, SessionControlOrigin, SessionSource,
    },
    event_server,
    state::{now_millis, AppState},
};

const MAX_ACP_LINE: usize = 8 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const TURN_TIMEOUT: Duration = Duration::from_secs(60 * 60);
type Pending = Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Value, String>>>>>;
type InboundPermissions = Arc<Mutex<HashMap<String, Vec<(Value, Arc<AtomicBool>)>>>>;
type SessionConfigs = Arc<Mutex<HashMap<String, Value>>>;

pub struct PromptFile {
    pub path: String,
    pub mime_type: String,
    pub is_image: bool,
}

struct Process {
    child: Child,
    writer: Arc<Mutex<ChildStdin>>,
    pending: Pending,
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
struct Runtime {
    process: Option<Process>,
    initialized: bool,
    can_resume: bool,
    can_load: bool,
    can_list: bool,
    can_images: bool,
    loaded: HashSet<String>,
    active: HashSet<String>,
    cancelled: HashSet<String>,
}

#[derive(Clone)]
pub struct OpenCodeBridge {
    runtime: Arc<Mutex<Runtime>>,
    startup: Arc<Mutex<()>>,
    next_id: Arc<AtomicU64>,
    turn_text: Arc<Mutex<HashMap<String, String>>>,
    inbound_permissions: InboundPermissions,
    turn_sequence: Arc<Mutex<HashMap<String, u64>>>,
    session_configs: SessionConfigs,
    reader_generation: Arc<AtomicU64>,
    state: AppState,
    app: AppHandle,
}

impl OpenCodeBridge {
    pub(crate) fn environment_roots(&self) -> Vec<(String, u32)> {
        self.runtime.lock().map(|runtime| runtime.process.as_ref().map(|process|
            runtime.loaded.iter().map(|id| (id.clone(), process.child.id())).collect()
        ).unwrap_or_default()).unwrap_or_default()
    }

    pub fn new(state: AppState, app: AppHandle) -> Self {
        Self {
            runtime: Arc::new(Mutex::new(Runtime::default())),
            startup: Arc::new(Mutex::new(())),
            next_id: Arc::new(AtomicU64::new(1)),
            turn_text: Arc::new(Mutex::new(HashMap::new())),
            inbound_permissions: Arc::new(Mutex::new(HashMap::new())),
            turn_sequence: Arc::new(Mutex::new(HashMap::new())),
            session_configs: Arc::new(Mutex::new(HashMap::new())),
            reader_generation: Arc::new(AtomicU64::new(0)),
            state,
            app,
        }
    }

    fn ensure_started(&self) -> Result<(), String> {
        let _startup = self
            .startup
            .lock()
            .map_err(|_| "OpenCode startup lock failed")?;
        {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| "OpenCode state lock failed")?;
            if let Some(process) = runtime.process.as_mut() {
                if process
                    .child
                    .try_wait()
                    .map_err(|error| error.to_string())?
                    .is_none()
                    && runtime.initialized
                {
                    return Ok(());
                }
            }
            let generation = self.reader_generation.fetch_add(1, Ordering::SeqCst) + 1;
            runtime.process = None;
            runtime.initialized = false;
            runtime.can_resume = false;
            runtime.can_load = false;
            runtime.can_list = false;
            runtime.can_images = false;
            runtime.loaded.clear();
            runtime.active.clear();
            runtime.cancelled.clear();
            self.session_configs
                .lock()
                .map_err(|_| "OpenCode config lock failed")?
                .clear();
            self.turn_text
                .lock()
                .map_err(|_| "OpenCode stream lock failed")?
                .clear();
            if let Ok(mut permissions) = self.inbound_permissions.lock() {
                for requests in permissions.values() {
                    for (_, answered) in requests {
                        answered.store(true, Ordering::SeqCst);
                    }
                }
                permissions.clear();
            }

            let mut password = [0u8; 24];
            getrandom::getrandom(&mut password)
                .map_err(|error| format!("Could not secure OpenCode loopback: {error}"))?;
            let secret = password
                .iter()
                .map(|value| format!("{value:02x}"))
                .collect::<String>();
            let mut command = crate::executables::command("opencode")?;
            command
                .args(["acp", "--hostname", "127.0.0.1", "--port", "0"])
                .env("OPENCODE_SERVER_PASSWORD", secret)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000);
            }
            let mut child = command
                .spawn()
                .map_err(|error| format!("Could not launch OpenCode ACP: {error}"))?;
            let writer = Arc::new(Mutex::new(
                child.stdin.take().ok_or("OpenCode ACP stdin unavailable")?,
            ));
            let output = child
                .stdout
                .take()
                .ok_or("OpenCode ACP stdout unavailable")?;
            let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
            let reader_pending = pending.clone();
            let reader_writer = writer.clone();
            let reader_state = self.state.clone();
            let reader_app = self.app.clone();
            let turn_text = self.turn_text.clone();
            let inbound_permissions = self.inbound_permissions.clone();
            let turn_sequence = self.turn_sequence.clone();
            let session_configs = self.session_configs.clone();
            let reader_generation = self.reader_generation.clone();
            thread::Builder::new()
                .name("lume-opencode-acp".into())
                .spawn(move || {
                    read_messages(
                        output,
                        reader_writer,
                        reader_pending,
                        reader_state,
                        reader_app,
                        turn_text,
                        inbound_permissions,
                        turn_sequence,
                        session_configs,
                        reader_generation,
                        generation,
                    )
                })
                .map_err(|error| error.to_string())?;
            runtime.process = Some(Process {
                child,
                writer,
                pending,
            });
        }
        let initialized = self.raw_request("initialize", json!({
            "protocolVersion": 1,
            "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "Lume", "version": env!("CARGO_PKG_VERSION")}
        }), REQUEST_TIMEOUT);
        let initialized = match initialized {
            Ok(initialized) => initialized,
            Err(error) => {
                if let Ok(mut runtime) = self.runtime.lock() {
                    runtime.process = None;
                }
                return Err(error);
            }
        };
        if initialized.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
            if let Ok(mut runtime) = self.runtime.lock() {
                runtime.process = None;
            }
            return Err("OpenCode ACP protocol version is not supported".into());
        }
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?;
        runtime.can_resume = initialized["agentCapabilities"]["sessionCapabilities"]
            .get("resume")
            .is_some();
        runtime.can_load = initialized["agentCapabilities"]["loadSession"].as_bool() == Some(true);
        runtime.can_list = initialized["agentCapabilities"]["sessionCapabilities"]
            .get("list")
            .is_some();
        runtime.can_images =
            initialized["agentCapabilities"]["promptCapabilities"]["image"].as_bool() == Some(true);
        runtime.initialized = true;
        Ok(())
    }

    fn raw_request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let (writer, pending) = {
            let runtime = self
                .runtime
                .lock()
                .map_err(|_| "OpenCode state lock failed")?;
            let process = runtime
                .process
                .as_ref()
                .ok_or("OpenCode ACP is not running")?;
            (process.writer.clone(), process.pending.clone())
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        pending
            .lock()
            .map_err(|_| "OpenCode response lock failed")?
            .insert(id, sender);
        let payload = json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params});
        let write_result = writer
            .lock()
            .map_err(|_| "OpenCode write lock failed".to_string())?
            .write_all(format!("{payload}\n").as_bytes());
        if let Err(error) = write_result {
            let _ = pending.lock().map(|mut requests| requests.remove(&id));
            return Err(format!("OpenCode ACP connection closed: {error}"));
        }
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(error) => {
                let _ = pending.lock().map(|mut requests| requests.remove(&id));
                Err(format!("OpenCode ACP {method} did not complete: {error}"))
            }
        }
    }

    pub fn launch(
        &self,
        working_directory: &str,
        resume_id: Option<&str>,
    ) -> Result<String, String> {
        let cwd = std::fs::canonicalize(working_directory)
            .map_err(|_| "A pasta do projeto do OpenCode não existe".to_string())?;
        if !cwd.is_dir() {
            return Err("A pasta do projeto do OpenCode não é um diretório".into());
        }
        let cwd = cwd.to_string_lossy().to_string();
        self.ensure_started()?;
        let (method, params) = if let Some(id) = resume_id {
            let has_history = self
                .state
                .connected_session(&format!("opencode-acp:{id}"))
                .is_ok_and(|session| !session.activities.is_empty());
            let runtime = self
                .runtime
                .lock()
                .map_err(|_| "OpenCode state lock failed")?;
            // Replay only for imported chats. Reattaching a conversation already
            // stored in Lume must not duplicate all of its historical messages.
            let method = resume_method(runtime.can_load, runtime.can_resume, has_history)?;
            if method == "session/load" {
                self.turn_sequence
                    .lock()
                    .map_err(|_| "OpenCode stream lock failed")?
                    .insert(id.into(), 0);
            }
            (method, json!({"sessionId":id,"cwd":cwd,"mcpServers":[]}))
        } else {
            ("session/new", json!({"cwd":cwd,"mcpServers":[]}))
        };
        let response = self.raw_request(method, params, REQUEST_TIMEOUT)?;
        let id = resume_id
            .map(str::to_string)
            .or_else(|| {
                response
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .ok_or("OpenCode did not return a session ID")?;
        self.cache_config(&id, &response)?;
        self.runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?
            .loaded
            .insert(id.clone());
        let project = Path::new(&cwd)
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        let mut event = base_event(&id, HookEventKind::SessionStarted);
        event.project = project;
        event.working_directory = Some(cwd);
        event.permission_profile = Some(permission_profile());
        if let Err(error) = event_server::publish_event(&self.state, &self.app, event) {
            let _ = self.close(&id);
            return Err(error);
        }
        Ok(id)
    }

    pub fn resumable_sessions(&self) -> Result<Vec<crate::integrations::ResumableSession>, String> {
        self.ensure_started()?;
        if !self
            .runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?
            .can_list
        {
            return Err("This OpenCode version cannot list sessions through ACP".into());
        }
        let mut sessions = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        for _ in 0..5 {
            let params = cursor
                .as_ref()
                .map(|cursor| json!({"cursor":cursor}))
                .unwrap_or_else(|| json!({}));
            let result = self.raw_request("session/list", params, REQUEST_TIMEOUT)?;
            if let Some(items) = result["sessions"].as_array() {
                for item in items {
                    let (Some(id), Some(cwd)) = (item["sessionId"].as_str(), item["cwd"].as_str())
                    else {
                        continue;
                    };
                    if !seen.insert(id.to_string()) {
                        continue;
                    }
                    let project = Path::new(cwd)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(cwd);
                    let updated_at = item["updatedAt"]
                        .as_str()
                        .and_then(|stamp| chrono::DateTime::parse_from_rfc3339(stamp).ok())
                        .map(|stamp| stamp.timestamp_millis())
                        .unwrap_or(0);
                    sessions.push(crate::integrations::ResumableSession {
                        id: id.to_string(),
                        agent: crate::integrations::IntegrationKind::OpenCode,
                        name: item["title"]
                            .as_str()
                            .filter(|title| !title.trim().is_empty())
                            .unwrap_or("OpenCode session")
                            .to_string(),
                        project: project.to_string(),
                        working_directory: cwd.to_string(),
                        source: "OpenCode ACP".into(),
                        updated_at,
                    });
                    if sessions.len() == 250 {
                        break;
                    }
                }
            }
            if sessions.len() == 250 {
                break;
            }
            let Some(next) = result["nextCursor"]
                .as_str()
                .filter(|next| !next.is_empty())
            else {
                break;
            };
            if cursor.as_deref() == Some(next) {
                break;
            }
            cursor = Some(next.to_string());
        }
        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(sessions)
    }

    pub fn prompt(
        &self,
        session_id: &str,
        cwd: &str,
        prompt: String,
        files: Vec<PromptFile>,
    ) -> Result<(), String> {
        self.ensure_started()?;
        let loaded = self
            .runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?
            .loaded
            .contains(session_id);
        if !loaded {
            self.launch(cwd, Some(session_id))?;
        }
        let can_images = self
            .runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?
            .can_images;
        let content = prompt_content(&prompt, &files, can_images)?;
        {
            let mut runtime = self
                .runtime
                .lock()
                .map_err(|_| "OpenCode state lock failed")?;
            if !runtime.active.insert(session_id.into()) {
                return Err("Este prompt do OpenCode já está em execução".into());
            }
            runtime.cancelled.remove(session_id);
        }
        let prepared = (|| -> Result<(), String> {
            *self
                .turn_sequence
                .lock()
                .map_err(|_| "OpenCode stream lock failed")?
                .entry(session_id.into())
                .or_default() += 1;
            self.turn_text
                .lock()
                .map_err(|_| "OpenCode stream lock failed")?
                .insert(session_id.into(), String::new());
            event_server::publish_event(
                &self.state,
                &self.app,
                base_event(session_id, HookEventKind::Running),
            )
            .map(|_| ())
        })();
        if let Err(error) = prepared {
            if let Ok(mut runtime) = self.runtime.lock() {
                runtime.active.remove(session_id);
            }
            return Err(error);
        }
        let bridge = self.clone();
        let id = session_id.to_string();
        let generation = self.reader_generation.load(Ordering::SeqCst);
        thread::Builder::new()
            .name("lume-opencode-prompt".into())
            .spawn(move || {
                let result = bridge.raw_request(
                    "session/prompt",
                    json!({
                        "sessionId": id, "prompt": content
                    }),
                    TURN_TIMEOUT,
                );
                // A retired reader must not clear a new process's turn or emit
                // an old failure into a recovered conversation.
                if generation != bridge.reader_generation.load(Ordering::SeqCst) {
                    return;
                }
                if !bridge
                    .runtime
                    .lock()
                    .ok()
                    .is_some_and(|runtime| runtime.loaded.contains(&id))
                {
                    return;
                }
                let response = bridge
                    .turn_text
                    .lock()
                    .ok()
                    .and_then(|mut values| values.remove(&id));
                let cancelled = bridge
                    .runtime
                    .lock()
                    .ok()
                    .is_some_and(|runtime| runtime.cancelled.contains(&id));
                let stop_reason = result
                    .as_ref()
                    .ok()
                    .and_then(|value| value["stopReason"].as_str());
                let event_kind = if cancelled || stop_reason == Some("cancelled") {
                    HookEventKind::WaitingForInput
                } else if result.is_ok() {
                    HookEventKind::Completed
                } else {
                    HookEventKind::Failed
                };
                let mut event = base_event(&id, event_kind);
                event.last_response = response;
                if cancelled || stop_reason == Some("cancelled") {
                    event.status_label = Some("Interrompido".into());
                } else if let Err(error) = result {
                    event.status_label = Some(format!(
                        "OpenCode: {}",
                        error.chars().take(160).collect::<String>()
                    ));
                }
                let _ = event_server::publish_event(&bridge.state, &bridge.app, event);
                if let Ok(mut runtime) = bridge.runtime.lock() {
                    runtime.active.remove(&id);
                    runtime.cancelled.remove(&id);
                }
            })
            .map_err(|error| {
                if let Ok(mut runtime) = self.runtime.lock() {
                    runtime.active.remove(session_id);
                }
                error.to_string()
            })?;
        Ok(())
    }

    fn cache_config(&self, session_id: &str, response: &Value) -> Result<(), String> {
        if response.get("configOptions").is_some() {
            self.session_configs
                .lock()
                .map_err(|_| "OpenCode config lock failed")?
                .insert(session_id.into(), response["configOptions"].clone());
        }
        Ok(())
    }

    fn config_options(&self, session_id: &str, cwd: &str) -> Result<Value, String> {
        self.ensure_started()?;
        if !self
            .runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?
            .loaded
            .contains(session_id)
        {
            self.launch(cwd, Some(session_id))?;
        }
        self.session_configs
            .lock()
            .map_err(|_| "OpenCode config lock failed")?
            .get(session_id)
            .cloned()
            .ok_or_else(|| "Esta versão do OpenCode não informou as opções da sessão".into())
    }

    pub fn slash_commands(
        &self,
        session_id: &str,
        cwd: &str,
    ) -> Result<Vec<crate::agent_commands::AgentSlashCommand>, String> {
        // Loading the session makes OpenCode announce its commands over ACP.
        let _ = self.config_options(session_id, cwd);
        for _ in 0..40 {
            if let Some(commands) = crate::agent_commands::opencode_commands(session_id) {
                return Ok(commands);
            }
            thread::sleep(Duration::from_millis(25));
        }
        Ok(Vec::new())
    }

    pub fn model_settings(
        &self,
        session_id: &str,
        cwd: &str,
    ) -> Result<CodexThreadModelSettings, String> {
        model_settings_from_config(&self.config_options(session_id, cwd)?)
    }

    fn set_config_option(
        &self,
        session_id: &str,
        option: &Value,
        value: &str,
    ) -> Result<Value, String> {
        validate_config_value(option, value)?;
        let config_id = option["id"]
            .as_str()
            .ok_or("OpenCode retornou uma opção sem identificador")?;
        let response = self.raw_request(
            "session/set_config_option",
            json!({"sessionId":session_id,"configId":config_id,"value":value}),
            REQUEST_TIMEOUT,
        )?;
        let returned = &response["configOptions"];
        if config_value(returned, config_id) == Some(value) {
            self.cache_config(session_id, &response)?;
            return Ok(returned.clone());
        }
        // Some OpenCode versions report the new selection in a separate
        // config_option_update notification. Do not replace that newer cache
        // with a stale set_config_option response.
        for _ in 0..20 {
            let cached = self
                .session_configs
                .lock()
                .map_err(|_| "OpenCode config lock failed")?
                .get(session_id)
                .cloned();
            if let Some(config) = cached {
                if config_value(&config, config_id) == Some(value) {
                    return Ok(config);
                }
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err("OpenCode não confirmou a troca de modelo. A seleção anterior foi mantida.".into())
    }

    pub fn set_model_settings(
        &self,
        session_id: &str,
        cwd: &str,
        model: &str,
        effort: &str,
    ) -> Result<CodexThreadModelSettings, String> {
        let mut config = self.config_options(session_id, cwd)?;
        let option =
            config_option(&config, "model").ok_or("OpenCode não oferece seleção de modelo")?;
        validate_config_value(option, model)?;
        let changing_model = option["currentValue"].as_str() != Some(model);
        // A model change supplies its own effort catalog and default. Do not apply
        // an effort borrowed from the previous model.
        if !changing_model && !effort.is_empty() {
            let option = config_option(&config, "thought_level")
                .ok_or("Este modelo não oferece níveis de effort")?;
            validate_config_value(option, effort)?;
        }
        if changing_model {
            config = self.set_config_option(session_id, option, model)?;
        }
        if !effort.is_empty() {
            let option = config_option(&config, "thought_level")
                .ok_or("Este modelo não oferece níveis de effort")?;
            if option["currentValue"].as_str() != Some(effort) {
                config = self.set_config_option(session_id, option, effort)?;
            }
        }
        model_settings_from_config(&config)
    }

    pub fn set_mode(
        &self,
        session_id: &str,
        cwd: &str,
        mode: &str,
    ) -> Result<CodexThreadModelSettings, String> {
        let config = self.config_options(session_id, cwd)?;
        let option =
            config_option(&config, "mode").ok_or("OpenCode não oferece seleção de modo")?;
        model_settings_from_config(&self.set_config_option(session_id, option, mode)?)
    }

    pub fn cancel(&self, session_id: &str) -> Result<(), String> {
        self.runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?
            .cancelled
            .insert(session_id.into());
        let writer = self
            .runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?
            .process
            .as_ref()
            .ok_or("OpenCode ACP is not running")?
            .writer
            .clone();
        if let Ok(mut pending) = self.inbound_permissions.lock() {
            for (request_id, answered) in pending.remove(session_id).unwrap_or_default() {
                if !answered.swap(true, Ordering::SeqCst) {
                    let _ = send_json(
                        &writer,
                        json!({"jsonrpc":"2.0","id":request_id,"result":{"outcome":{"outcome":"cancelled"}}}),
                    );
                }
            }
        }
        send_json(
            &writer,
            json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session_id}}),
        )
    }

    pub fn close(&self, session_id: &str) -> Result<(), String> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?;
        if !runtime.loaded.contains(session_id) {
            return Ok(());
        }
        let alive = match runtime.process.as_mut() {
            Some(process) => process
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_none(),
            None => false,
        };
        if !alive {
            runtime.process = None;
            runtime.loaded.remove(session_id);
            runtime.active.remove(session_id);
            self.session_configs
                .lock()
                .map_err(|_| "OpenCode config lock failed")?
                .remove(session_id);
            return Ok(());
        }
        drop(runtime);
        self.cancel(session_id)?;
        self.raw_request(
            "session/close",
            json!({"sessionId":session_id}),
            REQUEST_TIMEOUT,
        )?;
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "OpenCode state lock failed")?;
        runtime.loaded.remove(session_id);
        runtime.active.remove(session_id);
        runtime.cancelled.remove(session_id);
        // Keep the shared service only while another Lume session needs it.
        if runtime.loaded.is_empty() {
            runtime.process = None;
            runtime.initialized = false;
        }
        drop(runtime);
        self.session_configs
            .lock()
            .map_err(|_| "OpenCode config lock failed")?
            .remove(session_id);
        Ok(())
    }
}

fn permission_profile() -> PermissionProfile {
    PermissionProfile {
        mode: AccessMode::WorkspaceWrite,
        label: "OpenCode ACP".into(),
        approval_policy: "user".into(),
        approvals_reviewer: None,
        can_respond_from_lume: true,
        available_actions: vec![
            PermissionAction::AllowOnce,
            PermissionAction::AllowSession,
            PermissionAction::Deny,
        ],
    }
}

fn base_event(id: &str, event: HookEventKind) -> HookEvent {
    HookEvent {
        event,
        session_id: format!("opencode-acp:{id}"),
        agent: AgentKind::OpenCode,
        agent_label: Some("OpenCode".into()),
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

fn send_json(writer: &Arc<Mutex<ChildStdin>>, value: Value) -> Result<(), String> {
    writer
        .lock()
        .map_err(|_| "OpenCode write lock failed".to_string())?
        .write_all(format!("{value}\n").as_bytes())
        .map_err(|error| error.to_string())
}

fn read_messages(
    output: impl Read,
    writer: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    state: AppState,
    app: AppHandle,
    turn_text: Arc<Mutex<HashMap<String, String>>>,
    inbound_permissions: InboundPermissions,
    turn_sequence: Arc<Mutex<HashMap<String, u64>>>,
    session_configs: SessionConfigs,
    reader_generation: Arc<AtomicU64>,
    generation: u64,
) {
    let mut reader = BufReader::new(output);
    while let Ok(Some(line)) = read_bounded_line(&mut reader) {
        if generation != reader_generation.load(Ordering::SeqCst) {
            break;
        }
        if line.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if let Some(id_value) = message.get("id") {
            if message.get("method").is_some() {
                answer_agent_request(
                    id_value.clone(),
                    &message,
                    &writer,
                    &state,
                    &app,
                    &inbound_permissions,
                );
            } else if let Some(id) = id_value.as_u64() {
                if let Ok(mut requests) = pending.lock() {
                    if let Some(sender) = requests.remove(&id) {
                        let answer = message
                            .get("error")
                            .map(|error| {
                                Err(error
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("OpenCode ACP error")
                                    .to_string())
                            })
                            .unwrap_or_else(|| {
                                Ok(message.get("result").cloned().unwrap_or(Value::Null))
                            });
                        let _ = sender.send(answer);
                    }
                }
            }
        } else if message.get("method").and_then(Value::as_str) == Some("session/update") {
            handle_update(
                &message,
                &state,
                &app,
                &turn_text,
                &turn_sequence,
                &session_configs,
            );
        }
    }
    if let Ok(mut requests) = pending.lock() {
        for (_, sender) in requests.drain() {
            let _ = sender.send(Err("OpenCode ACP process disconnected".into()));
        }
    }
    if generation != reader_generation.load(Ordering::SeqCst) {
        return;
    }
    if let Ok(mut permissions) = inbound_permissions.lock() {
        for requests in permissions.values() {
            for (_, answered) in requests {
                answered.store(true, Ordering::SeqCst);
            }
        }
        permissions.clear();
    }
    if let Ok(mut turns) = turn_text.lock() {
        turns.clear();
    }
}

pub(crate) fn read_bounded_line(reader: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok((!line.is_empty()).then_some(line));
        }
        let consumed = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(buffer.len());
        let ended = buffer.get(consumed - 1) == Some(&b'\n');
        if !oversized && line.len() + consumed <= MAX_ACP_LINE {
            line.extend_from_slice(&buffer[..consumed]);
        } else {
            line.clear();
            oversized = true;
        }
        reader.consume(consumed);
        if ended {
            return Ok(Some(line));
        }
    }
}

fn handle_update(
    message: &Value,
    state: &AppState,
    app: &AppHandle,
    turn_text: &Arc<Mutex<HashMap<String, String>>>,
    turn_sequence: &Arc<Mutex<HashMap<String, u64>>>,
    session_configs: &SessionConfigs,
) {
    let params = &message["params"];
    let Some(id) = params["sessionId"].as_str() else {
        return;
    };
    let update = &params["update"];
    let kind = update["sessionUpdate"].as_str().unwrap_or("");
    if kind == "available_commands_update" {
        crate::agent_commands::record_opencode_commands(id, &update["availableCommands"]);
        return;
    }
    if kind == "config_option_update" {
        if update["configOptions"].is_array() {
            if let Ok(mut configs) = session_configs.lock() {
                configs.insert(id.into(), update["configOptions"].clone());
            }
        }
        return;
    }
    if kind == "session_info_update" {
        if let Some(title) = update["title"]
            .as_str()
            .filter(|title| !title.trim().is_empty())
        {
            let mut event = base_event(id, HookEventKind::Activity);
            event.session_name = Some(title.chars().take(140).collect());
            let _ = event_server::publish_event(state, app, event);
        }
        return;
    }
    let activity = match kind {
        "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk" => {
            let Some(text) = update["content"]["text"].as_str() else {
                return;
            };
            if text.is_empty() {
                return;
            }
            let live = turn_text
                .lock()
                .ok()
                .is_some_and(|turns| turns.contains_key(id));
            // Lume records outgoing prompts itself. ACP replays must retain users'
            // messages, but live echoes must not duplicate those messages.
            if live && kind == "user_message_chunk" {
                return;
            }
            if !live {
                if let Ok(mut turns) = turn_sequence.lock() {
                    *turns.entry(id.into()).or_default() += 1;
                }
            }
            if live && kind == "agent_message_chunk" {
                if let Ok(mut turns) = turn_text.lock() {
                    let full = turns.entry(id.into()).or_default();
                    if full.len() < 128 * 1024 {
                        full.push_str(text);
                    }
                }
            }
            SessionActivity {
                id: format!(
                    "opencode-{kind}:{id}:{}:{}",
                    turn_sequence
                        .lock()
                        .ok()
                        .and_then(|turns| turns.get(id).copied())
                        .unwrap_or(0),
                    update["messageId"].as_str().unwrap_or("current")
                ),
                kind: if kind == "user_message_chunk" {
                    "prompt"
                } else if kind == "agent_message_chunk" {
                    "message"
                } else {
                    "analysis"
                }
                .into(),
                title: if kind == "user_message_chunk" {
                    "Você"
                } else if kind == "agent_message_chunk" {
                    "OpenCode"
                } else {
                    "Pensando"
                }
                .into(),
                detail: Some(text.chars().take(32 * 1024).collect()),
                status: if live { "running" } else { "completed" }.into(),
                created_at: now_millis(),
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: true,
            }
        }
        "tool_call" | "tool_call_update" => tool_activity(id, update),
        "plan" => SessionActivity {
            id: format!("opencode-plan:{id}"),
            kind: "plan".into(),
            title: "Plano atualizado".into(),
            detail: Some(plan_detail(update)),
            status: "running".into(),
            created_at: now_millis(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        },
        _ => return,
    };
    let mut event = base_event(id, HookEventKind::Activity);
    event.activity = Some(activity);
    let _ = event_server::publish_event(state, app, event);
}

fn plan_detail(update: &Value) -> String {
    update["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .take(200)
        .filter_map(|entry| {
            let content = entry["content"].as_str()?;
            let marker = match entry["status"].as_str() {
                Some("completed") => "✓",
                Some("in_progress") => "●",
                _ => "○",
            };
            Some(format!(
                "{marker} {}",
                content.chars().take(512).collect::<String>()
            ))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_activity(session_id: &str, update: &Value) -> SessionActivity {
    let tool_id = update["toolCallId"].as_str().unwrap_or("tool");
    let title = update["title"].as_str().unwrap_or("Ferramenta em execução");
    let tool_kind = update["kind"].as_str().unwrap_or("other");
    let title = match tool_kind {
        "read" => format!("Read · {title}"),
        "search" | "fetch" => format!("Search · {title}"),
        _ => title.to_string(),
    };
    let mut files = Vec::new();
    for path in update["locations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|location| location["path"].as_str())
        .chain(
            update["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|content| content["path"].as_str()),
        )
        .take(64)
    {
        let path = path.chars().take(4096).collect::<String>();
        if !files.contains(&path) {
            files.push(path);
        }
    }
    let detail = update["rawInput"]["command"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            let lines = update["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|content| content["content"]["text"].as_str())
                .take(16)
                .collect::<Vec<_>>();
            (!lines.is_empty()).then(|| lines.join("\n"))
        })
        .map(|text| text.chars().take(8 * 1024).collect());
    SessionActivity {
        id: format!("opencode-tool:{session_id}:{tool_id}"),
        kind: match tool_kind {
            "edit" | "delete" | "move" => "file",
            "read" | "search" | "fetch" => "tool",
            "execute" => "command",
            _ => "tool",
        }
        .into(),
        title: title.chars().take(180).collect(),
        detail,
        status: match update["status"].as_str() {
            Some("completed") => "completed",
            Some("failed") => "failed",
            _ => "running",
        }
        .into(),
        created_at: now_millis(),
        files,
        attachments: Vec::new(),
        // ACP updates can contain only a status. Retain the original tool's
        // title, kind and file locations when merging a partial update.
        append_detail: update["sessionUpdate"] == "tool_call_update",
    }
}

fn resume_method(
    can_load: bool,
    can_resume: bool,
    has_history: bool,
) -> Result<&'static str, String> {
    if can_resume && has_history {
        Ok("session/resume")
    } else if can_load {
        Ok("session/load")
    } else if can_resume {
        Ok("session/resume")
    } else {
        Err("This OpenCode version cannot resume sessions through ACP".into())
    }
}

fn select_values(option: &Value) -> Vec<&Value> {
    option["options"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|item| match item.get("options").and_then(Value::as_array) {
            Some(group) => group.iter().collect::<Vec<_>>(),
            None => vec![item],
        })
        .filter(|item| item["value"].is_string())
        .collect()
}

fn config_option<'a>(config: &'a Value, category: &str) -> Option<&'a Value> {
    let options = config.as_array()?;
    options
        .iter()
        .find(|option| option["type"] == "select" && option["category"] == category)
        .or_else(|| {
            options.iter().find(|option| {
                option["type"] == "select"
                    && (option["id"] == category
                        || (category == "thought_level" && option["id"] == "effort"))
            })
        })
}

fn config_value<'a>(config: &'a Value, id: &str) -> Option<&'a str> {
    config
        .as_array()?
        .iter()
        .find(|option| option["id"] == id)?["currentValue"]
        .as_str()
}

fn validate_config_value(option: &Value, value: &str) -> Result<(), String> {
    if !select_values(option)
        .iter()
        .any(|item| item["value"].as_str() == Some(value))
    {
        return Err("A opção selecionada não está disponível nesta sessão OpenCode".into());
    }
    Ok(())
}

fn model_settings_from_config(config: &Value) -> Result<CodexThreadModelSettings, String> {
    let option =
        config_option(config, "model").ok_or("OpenCode não informou os modelos da sessão")?;
    let model = option["currentValue"]
        .as_str()
        .ok_or("OpenCode não informou o modelo atual")?
        .to_string();
    let effort = config_option(config, "thought_level");
    let reasoning_effort = effort
        .and_then(|option| option["currentValue"].as_str())
        .map(str::to_string);
    let efforts = effort
        .map(select_values)
        .unwrap_or_default()
        .into_iter()
        .map(|item| CodexReasoningEffortOption {
            value: item["value"].as_str().unwrap_or_default().into(),
            description: item["description"]
                .as_str()
                .or_else(|| item["name"].as_str())
                .unwrap_or_default()
                .into(),
        })
        .collect::<Vec<_>>();
    let models = select_values(option)
        .into_iter()
        .map(|item| {
            let id = item["value"].as_str().unwrap_or_default();
            CodexModelOption {
                model: id.into(),
                display_name: item["name"].as_str().unwrap_or(id).into(),
                description: item["description"].as_str().unwrap_or_default().into(),
                is_default: false,
                default_reasoning_effort: if id == model {
                    reasoning_effort.clone().unwrap_or_default()
                } else {
                    String::new()
                },
                supported_reasoning_efforts: if id == model {
                    efforts.clone()
                } else {
                    Vec::new()
                },
            }
        })
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Err("OpenCode não retornou modelos disponíveis".into());
    }
    let session_modes = config_option(config, "mode").and_then(|option| {
        Some(SessionModeSettings {
            current_mode: option["currentValue"].as_str()?.into(),
            options: select_values(option)
                .into_iter()
                .map(|item| SessionModeOption {
                    value: item["value"].as_str().unwrap_or_default().into(),
                    label: item["name"].as_str().unwrap_or_default().into(),
                    description: item["description"].as_str().unwrap_or_default().into(),
                })
                .collect(),
        })
    });
    Ok(CodexThreadModelSettings {
        model,
        reasoning_effort,
        service_tier: None,
        models,
        session_modes,
    })
}

fn prompt_content(
    prompt: &str,
    files: &[PromptFile],
    can_images: bool,
) -> Result<Vec<Value>, String> {
    if files.len() > 4 {
        return Err("Envie no máximo 4 arquivos por prompt".into());
    }
    let mut content = Vec::with_capacity(files.len() + 1);
    if !prompt.is_empty() {
        content.push(json!({"type":"text","text":prompt}));
    }
    for file in files {
        let uri = Url::from_file_path(&file.path).map_err(|_| "Caminho de anexo inválido")?;
        if file.is_image {
            if !can_images {
                return Err("Esta versão do OpenCode não aceita imagens pelo ACP".into());
            }
            let mut bytes = Vec::new();
            std::fs::File::open(&file.path)
                .map_err(|error| error.to_string())?
                .take(MAX_IMAGE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.len() as u64 > MAX_IMAGE_BYTES {
                return Err("A imagem excede o limite de 5 MB".into());
            }
            content.push(json!({"type":"image","data":STANDARD.encode(&bytes),"mimeType":file.mime_type,"uri":uri.as_str()}));
        } else {
            content.push(json!({"type":"resource_link","uri":uri.as_str(),"name":Path::new(&file.path).file_name().unwrap_or_default().to_string_lossy(),"mimeType":file.mime_type}));
        }
    }
    Ok(content)
}

fn answer_agent_request(
    id: Value,
    message: &Value,
    writer: &Arc<Mutex<ChildStdin>>,
    state: &AppState,
    app: &AppHandle,
    inbound: &InboundPermissions,
) {
    if message["method"].as_str() != Some("session/request_permission") {
        let _ = send_json(
            writer,
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported client method"}}),
        );
        return;
    }
    let Some(session_id) = message["params"]["sessionId"].as_str() else {
        let _ = send_json(
            writer,
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Missing sessionId"}}),
        );
        return;
    };
    let options = message["params"]["options"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let tool = &message["params"]["toolCall"];
    let tool_id = tool["toolCallId"].as_str().unwrap_or("tool");
    let permission_id = format!("opencode:{session_id}:{id}:{tool_id}");
    let mut event = base_event(session_id, HookEventKind::PermissionRequest);
    let mut profile = permission_profile();
    profile.available_actions = vec![PermissionAction::Deny];
    if options
        .iter()
        .any(|option| option["kind"].as_str() == Some("allow_once"))
    {
        profile.available_actions.push(PermissionAction::AllowOnce);
    }
    if options
        .iter()
        .any(|option| option["kind"].as_str() == Some("allow_always"))
    {
        profile
            .available_actions
            .push(PermissionAction::AllowSession);
    }
    event.permission_profile = Some(profile);
    event.permission = Some(PermissionRequest {
        id: permission_id.clone(),
        kind: "tool".into(),
        summary: tool["title"]
            .as_str()
            .unwrap_or("OpenCode requests permission")
            .chars()
            .take(180)
            .collect(),
        resource: tool["kind"].as_str().unwrap_or("tool").into(),
        risk: "medium".into(),
        requested_at: chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
            .to_rfc3339(),
    });
    let answered = Arc::new(AtomicBool::new(false));
    if let Ok(mut requests) = inbound.lock() {
        requests
            .entry(session_id.into())
            .or_default()
            .push((id.clone(), answered.clone()));
    }
    let published = event_server::publish_event(state, app, event).is_ok();
    let writer = writer.clone();
    let state = state.clone();
    let inbound = inbound.clone();
    let native_id = session_id.to_string();
    thread::spawn(move || {
        let mut decision = None;
        if published {
            for _ in 0..300 {
                if answered.load(Ordering::SeqCst) {
                    return;
                }
                decision = state
                    .wait_for_decision(&permission_id, Duration::from_secs(1))
                    .ok()
                    .flatten();
                if decision.is_some() {
                    break;
                }
            }
        }
        if answered.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut requests) = inbound.lock() {
            if let Some(items) = requests.get_mut(&native_id) {
                items.retain(|(request_id, _)| *request_id != id);
                if items.is_empty() {
                    requests.remove(&native_id);
                }
            }
        }
        let desired = match decision {
            Some(PermissionAction::AllowOnce) => "allow_once",
            Some(PermissionAction::AllowSession) => "allow_always",
            _ => "reject_once",
        };
        let selected = options
            .iter()
            .find(|option| option["kind"].as_str() == Some(desired))
            .or_else(|| {
                (desired == "reject_once")
                    .then(|| {
                        options
                            .iter()
                            .find(|option| option["kind"].as_str() == Some("reject_always"))
                    })
                    .flatten()
            })
            .and_then(|option| option["optionId"].as_str());
        let outcome = selected
            .map(|option_id| json!({"outcome":"selected","optionId":option_id}))
            .unwrap_or_else(|| json!({"outcome":"cancelled"}));
        let _ = send_json(
            &writer,
            json!({"jsonrpc":"2.0","id":id,"result":{"outcome":outcome}}),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_reader_rejects_oversized_line_without_leaking_into_next() {
        let input = format!("{}\n{{\"ok\":true}}\n", "x".repeat(MAX_ACP_LINE + 1));
        let mut reader = BufReader::new(input.as_bytes());
        assert_eq!(read_bounded_line(&mut reader).unwrap(), Some(Vec::new()));
        assert_eq!(
            read_bounded_line(&mut reader).unwrap().unwrap(),
            b"{\"ok\":true}\n"
        );
    }

    fn config(with_effort: bool) -> Value {
        let mut config = json!([
            {"id":"model","category":"model","type":"select","currentValue":"local/qwen","options":[
                {"group":"local","name":"Local models","options":[{"value":"local/qwen","name":"Qwen"}]},
                {"group":"cloud","name":"Cloud models","options":[{"value":"cloud/example","name":"Cloud"}]}
            ]},
            {"id":"mode","type":"select","currentValue":"build","options":[
                {"value":"build","name":"Build"},{"value":"plan","name":"Plan"},{"value":"review","name":"Review"}
            ]}
        ]);
        if with_effort {
            config.as_array_mut().unwrap().push(
                json!({"id":"effort","type":"select","currentValue":"high","options":[
                    {"value":"low","name":"Low"},{"value":"high","name":"High"}
                ]}),
            );
        }
        config
    }

    #[test]
    fn config_uses_native_models_modes_and_grouped_options() {
        let settings = model_settings_from_config(&config(false)).unwrap();
        assert_eq!(settings.model, "local/qwen");
        assert_eq!(settings.models.len(), 2);
        assert_eq!(settings.models[1].display_name, "Cloud");
        assert!(settings.reasoning_effort.is_none());
        assert!(settings
            .models
            .iter()
            .all(|model| model.supported_reasoning_efforts.is_empty()));
        let modes = settings.session_modes.unwrap();
        assert_eq!(modes.current_mode, "build");
        assert_eq!(modes.options[2].value, "review");
    }

    #[test]
    fn effort_catalog_belongs_only_to_current_model() {
        let settings = model_settings_from_config(&config(true)).unwrap();
        assert_eq!(settings.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(settings.models[0].supported_reasoning_efforts.len(), 2);
        assert!(settings.models[1].supported_reasoning_efforts.is_empty());
        assert!(settings.models[1].default_reasoning_effort.is_empty());
        let config = config(true);
        assert!(
            validate_config_value(config_option(&config, "thought_level").unwrap(), "ultra")
                .is_err()
        );
        assert!(
            validate_config_value(config_option(&config, "model").unwrap(), "missing/model")
                .is_err()
        );
    }

    #[test]
    fn category_is_optional_and_not_used_as_a_config_identifier() {
        let config = json!([{"id":"provider/model-choice","category":"model","type":"select","currentValue":"one","options":[{"value":"one","name":"One"}]}]);
        assert_eq!(
            config_option(&config, "model").unwrap()["id"],
            "provider/model-choice"
        );
        assert!(model_settings_from_config(&json!([])).is_err());
    }

    #[test]
    fn history_import_loads_but_recovery_does_not_replay_stored_messages() {
        assert_eq!(resume_method(true, true, false).unwrap(), "session/load");
        assert_eq!(resume_method(true, true, true).unwrap(), "session/resume");
        assert_eq!(resume_method(true, false, true).unwrap(), "session/load");
        assert!(resume_method(false, false, false).is_err());
    }

    #[test]
    fn prompt_preserves_text_and_file_links() {
        let path = std::env::temp_dir()
            .join("lume test.md")
            .to_string_lossy()
            .to_string();
        let content = prompt_content(
            "Inspect this",
            &[PromptFile {
                path,
                mime_type: "text/markdown".into(),
                is_image: false,
            }],
            false,
        )
        .unwrap();
        assert_eq!(content[0], json!({"type":"text","text":"Inspect this"}));
        assert_eq!(content[1]["type"], "resource_link");
        assert_eq!(content[1]["name"], "lume test.md");
        assert!(content[1]["uri"]
            .as_str()
            .unwrap()
            .ends_with("lume%20test.md"));
    }

    #[test]
    fn image_requests_require_native_capability() {
        let path = std::env::temp_dir()
            .join("lume-image.png")
            .to_string_lossy()
            .to_string();
        let error = prompt_content(
            "",
            &[PromptFile {
                path,
                mime_type: "image/png".into(),
                is_image: true,
            }],
            false,
        )
        .unwrap_err();
        assert!(error.contains("não aceita imagens"));
    }

    struct TestFile(std::path::PathBuf);
    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn image_file(size: u64) -> TestFile {
        let mut nonce = [0u8; 8];
        getrandom::getrandom(&mut nonce).unwrap();
        let path =
            std::env::temp_dir().join(format!("lume-acp-test-{}.png", u64::from_le_bytes(nonce)));
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.set_len(size).unwrap();
        TestFile(path)
    }

    #[test]
    fn image_encoding_is_bounded_and_supports_attachment_only_prompts() {
        let small = image_file(4);
        let attachment = |path: &Path| PromptFile {
            path: path.to_string_lossy().into(),
            mime_type: "image/png".into(),
            is_image: true,
        };
        let content = prompt_content("", &[attachment(&small.0)], true).unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["mimeType"], "image/png");
        assert_eq!(content[0]["data"], "AAAAAA==");
        let large = image_file(MAX_IMAGE_BYTES + 1);
        assert!(prompt_content("", &[attachment(&large.0)], true)
            .unwrap_err()
            .contains("5 MB"));
    }

    #[test]
    fn plan_tracks_completion_and_current_step() {
        let detail = plan_detail(
            &json!({"entries":[{"content":"Inspect","status":"completed"},{"content":"Fix","status":"in_progress"},{"content":"Verify","status":"pending"}]}),
        );
        assert_eq!(detail, "✓ Inspect\n● Fix\n○ Verify");
    }

    #[test]
    fn partial_tool_updates_keep_original_tool_metadata_when_merged() {
        let original = tool_activity(
            "one",
            &json!({"sessionUpdate":"tool_call","toolCallId":"edit-1","kind":"edit","title":"Edit config","locations":[{"path":"config.ts"},{"path":"config.ts"}]}),
        );
        assert_eq!(original.kind, "file");
        assert_eq!(original.files, vec!["config.ts"]);
        let update = tool_activity(
            "one",
            &json!({"sessionUpdate":"tool_call_update","toolCallId":"edit-1","status":"completed"}),
        );
        assert_eq!(original.id, update.id);
        assert!(update.append_detail);
        assert_eq!(update.status, "completed");
    }
}
