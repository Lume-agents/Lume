use std::{
    collections::{HashMap, VecDeque},
    io::ErrorKind,
    net::{TcpListener, TcpStream},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Serialize;
use serde_json::{json, Value};
#[cfg(target_os = "macos")]
use std::io::Write;
use sysinfo::{get_current_pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind};
use tauri::AppHandle;
use tungstenite::{
    accept_hdr, client::connect_with_config, http::StatusCode, protocol::WebSocketConfig,
    stream::MaybeTlsStream, Message, WebSocket,
};

use crate::{
    codex_sessions::nested_work_tracking_tool,
    domain::{
        AccessMode, AgentKind, AgentRateLimit, HookEvent, HookEventKind, InteractiveQuestion,
        PendingQuestion, PermissionAction, PermissionProfile, PermissionRequest, QuestionOption,
        SessionActivity, SessionControlOrigin, SessionModelOverride, SessionSource, SessionStatus,
    },
    event_server,
    state::{now_millis, AppState},
};

const LEGACY_SERVER_URL: &str = "ws://127.0.0.1:43130";
const PROXY_ADDRESS: &str = "127.0.0.1:43131";
const PROXY_BASE_URL: &str = "ws://127.0.0.1:43131";
const MAX_LOCAL_CODEX_MESSAGE_BYTES: usize = 128 * 1024 * 1024;
const MAX_LOCAL_CODEX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const PROXY_SESSION_STABLE_FOR: Duration = Duration::from_millis(1_200);
const CODEX_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CODEX_MODEL_SETTINGS_TIMEOUT: Duration = Duration::from_secs(15);
const CODEX_RESUME_TIMEOUT: Duration = Duration::from_secs(90);
const CODEX_PROMPT_ACK_TIMEOUT: Duration = Duration::from_secs(30);
static NEXT_PROXY_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_PROXY_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static SERVER_URL: OnceLock<String> = OnceLock::new();

struct ManagedChild {
    child: Child,
    #[cfg(target_os = "windows")]
    _job: WindowsProcessJob,
}

impl ManagedChild {
    fn spawn(command: Command) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            return crate::macos_process_supervisor::spawn(command).map(|child| Self { child });
        }
        #[cfg(not(target_os = "macos"))]
        let mut command = command;
        #[cfg(not(target_os = "macos"))]
        let child = command.spawn().map_err(|error| error.to_string())?;
        #[cfg(target_os = "windows")]
        {
            let mut child = child;
            let job = match WindowsProcessJob::attach(&child) {
                Ok(job) => job,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            };
            return Ok(Self { child, _job: job });
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        Ok(Self { child })
    }

    #[cfg(target_os = "macos")]
    fn kill(&mut self) -> std::io::Result<()> {
        // Send an explicit stop before closing the lifetime pipe. This also
        // works if an unrelated concurrent spawn briefly inherited a writer;
        // EOF alone would then wait for that process while Lume stays alive.
        if let Some(mut lifetime_pipe) = self.child.stdin.take() {
            let _ = lifetime_pipe.write_all(&[1]);
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
impl Drop for ManagedChild {
    fn drop(&mut self) {
        // Startup can fail before the process enters the shared slot. Child
        // does not reap on drop, so close its lifetime pipe and collect the
        // supervisor here as well as during the normal bridge shutdown.
        let _ = self.kill();
        let _ = self.child.wait();
    }
}

impl std::ops::Deref for ManagedChild {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl std::ops::DerefMut for ManagedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

#[cfg(target_os = "windows")]
struct WindowsProcessJob(isize);

#[cfg(target_os = "windows")]
impl WindowsProcessJob {
    fn attach(child: &Child) -> Result<Self, String> {
        use std::{mem::size_of, os::windows::io::AsRawHandle, ptr};
        use windows_sys::Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
        };

        unsafe {
            let job = CreateJobObjectW(ptr::null(), ptr::null());
            if job.is_null() {
                return Err(format!(
                    "Could not create the Codex process job: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!(
                    "Could not configure the Codex process job: {error}"
                ));
            }
            let process = child.as_raw_handle() as HANDLE;
            if AssignProcessToJobObject(job, process) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!(
                    "Could not attach Codex to the process job: {error}"
                ));
            }
            Ok(Self(job as isize))
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsProcessJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        unsafe {
            CloseHandle(self.0 as HANDLE);
        }
    }
}

#[derive(Clone)]
struct QueuedPrompt {
    session_id: String,
    activity_id: String,
    prompt: String,
    attachment_paths: Vec<String>,
    profile: PermissionProfile,
}

const MAX_QUEUED_PROMPTS_PER_THREAD: usize = 20;
const MAX_TOTAL_QUEUED_PROMPTS: usize = 100;

struct ProxyPrompt {
    request_id: String,
    thread_id: String,
    prompt: String,
    attachment_paths: Vec<String>,
    profile: PermissionProfile,
    response: mpsc::Sender<Result<(), String>>,
}

#[derive(Clone)]
struct ActiveProxyConnection {
    connection_id: u64,
    sender: mpsc::Sender<ProxyPrompt>,
    ready: bool,
}

type ActiveProxyThreads = Arc<Mutex<HashMap<String, ActiveProxyConnection>>>;

#[derive(Clone, Debug, Default)]
struct CachedCodexThreadSettings {
    model: Option<String>,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
}

fn update_cached_thread_settings(
    cache: &Arc<Mutex<HashMap<String, CachedCodexThreadSettings>>>,
    thread_id: &str,
    update: CachedCodexThreadSettings,
) -> Result<(), String> {
    let mut settings = cache
        .lock()
        .map_err(|_| "Could not save the Codex thread model settings".to_string())?;
    let cached = settings.entry(thread_id.to_string()).or_default();
    if update.model.is_some() {
        cached.model = update.model;
    }
    if update.reasoning_effort.is_some() {
        cached.reasoning_effort = update.reasoning_effort;
    }
    if update.service_tier.is_some() {
        cached.service_tier = update.service_tier;
    }
    Ok(())
}

fn cached_thread_settings_from_response(response: &Value) -> Option<CachedCodexThreadSettings> {
    let result = response.get("result")?;
    let settings = CachedCodexThreadSettings {
        model: result
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        reasoning_effort: result
            .get("reasoningEffort")
            .map(|value| value.as_str().unwrap_or_default().to_string()),
        service_tier: result
            .get("serviceTier")
            .map(|value| value.as_str().unwrap_or("default").to_string()),
    };
    (settings.model.is_some()
        || settings.reasoning_effort.is_some()
        || settings.service_tier.is_some())
    .then_some(settings)
}

#[derive(Clone, Debug)]
pub struct PreparedThread {
    pub thread_id: String,
    pub thread_name: Option<String>,
    pub permission_profile: PermissionProfile,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexReasoningEffortOption {
    pub value: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModelOption {
    pub model: String,
    pub display_name: String,
    pub description: String,
    pub is_default: bool,
    pub default_reasoning_effort: String,
    pub supported_reasoning_efforts: Vec<CodexReasoningEffortOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexThreadModelSettings {
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub models: Vec<CodexModelOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_modes: Option<SessionModeSettings>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModeSettings {
    pub current_mode: String,
    pub options: Vec<SessionModeOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SessionModeOption {
    pub value: String,
    pub label: String,
    pub description: String,
}

pub struct CodexBridge {
    process: Arc<Mutex<Option<ManagedChild>>>,
    queued_prompts: Arc<Mutex<HashMap<String, VecDeque<QueuedPrompt>>>>,
    collaboration_modes: Arc<Mutex<HashMap<String, String>>>,
    active_proxy_threads: ActiveProxyThreads,
    thread_settings: Arc<Mutex<HashMap<String, CachedCodexThreadSettings>>>,
    proxy_url: Arc<str>,
    owns_process: bool,
}

impl Clone for CodexBridge {
    fn clone(&self) -> Self {
        Self {
            process: self.process.clone(),
            queued_prompts: self.queued_prompts.clone(),
            collaboration_modes: self.collaboration_modes.clone(),
            active_proxy_threads: self.active_proxy_threads.clone(),
            thread_settings: self.thread_settings.clone(),
            proxy_url: self.proxy_url.clone(),
            owns_process: false,
        }
    }
}

impl CodexBridge {
    pub fn start(state: AppState, app: AppHandle) -> Result<Self, String> {
        cleanup_orphaned_server()?;
        let _ = server_url();
        let proxy_token = random_secret(32)?;
        let proxy_url: Arc<str> = format!("{PROXY_BASE_URL}?token={proxy_token}").into();
        let listener = TcpListener::bind(PROXY_ADDRESS)
            .map_err(|error| format!("Could not start the Codex bridge: {error}"))?;
        let process = Arc::new(Mutex::new(None));
        let active_proxy_threads = Arc::new(Mutex::new(HashMap::new()));
        let thread_settings = Arc::new(Mutex::new(HashMap::new()));
        let proxy_state = state.clone();
        let proxy_app = app.clone();
        let proxy_threads = active_proxy_threads.clone();
        let proxy_process = process.clone();
        let proxy_thread_settings = thread_settings.clone();
        let listener_token = proxy_token.clone();
        thread::Builder::new()
            .name("lume-codex-proxy".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let state = proxy_state.clone();
                    let app = proxy_app.clone();
                    let active_threads = proxy_threads.clone();
                    let process = proxy_process.clone();
                    let thread_settings = proxy_thread_settings.clone();
                    let token = listener_token.clone();
                    let _ = thread::Builder::new()
                        .name("lume-codex-client".into())
                        .spawn(move || {
                            if let Err(error) = proxy_connection(
                                stream,
                                state,
                                app,
                                active_threads,
                                process,
                                thread_settings,
                                &token,
                            ) {
                                eprintln!("Ponte do Codex encerrada: {error}");
                            }
                        });
                }
            })
            .map_err(|error| error.to_string())?;
        let queued_prompts = Arc::new(Mutex::new(HashMap::new()));
        let collaboration_modes = Arc::new(Mutex::new(HashMap::new()));
        start_codex_cli_queue_observer(process.clone(), state.clone(), app.clone())?;
        start_queue_dispatcher(process.clone(), queued_prompts.clone(), state, app)?;
        Ok(Self {
            process,
            queued_prompts,
            collaboration_modes,
            active_proxy_threads,
            thread_settings,
            proxy_url,
            owns_process: true,
        })
    }

    pub fn proxy_url(&self) -> &str {
        &self.proxy_url
    }

    pub fn ensure_server(&self) -> Result<(), String> {
        ensure_server_process(&self.process)
    }

    fn update_thread_settings(
        &self,
        thread_id: &str,
        update: CachedCodexThreadSettings,
    ) -> Result<(), String> {
        update_cached_thread_settings(&self.thread_settings, thread_id, update)
    }

    pub fn wait_for_proxy_thread(&self, thread_id: &str, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut connected_since = None;
        while Instant::now() < deadline {
            let connected = proxy_thread_ready(&self.active_proxy_threads, thread_id)?;
            if connected {
                let since = connected_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= PROXY_SESSION_STABLE_FOR {
                    return Ok(());
                }
            } else {
                connected_since = None;
            }
            thread::sleep(Duration::from_millis(80));
        }
        Err("The Codex CLI could not keep its WebSocket connection to Lume. The session was not marked as controlled; you can try Resume again.".into())
    }

    pub fn prepare_thread(
        &self,
        working_directory: &str,
        resume_id: Option<&str>,
        permission_mode: Option<&AccessMode>,
        approval_policy: Option<&str>,
    ) -> Result<PreparedThread, String> {
        self.ensure_server()?;
        let prepared = prepare_thread_connection(
            working_directory,
            resume_id,
            permission_mode,
            approval_policy,
        )?;
        self.update_thread_settings(
            &prepared.thread_id,
            CachedCodexThreadSettings {
                model: prepared.model.clone(),
                reasoning_effort: prepared.reasoning_effort.clone(),
                service_tier: prepared.service_tier.clone(),
            },
        )?;
        Ok(prepared)
    }

    pub fn prepare_existing_thread_launch(
        &self,
        thread_id: &str,
        thread_name: Option<String>,
        permission_mode: Option<&AccessMode>,
        approval_policy: Option<&str>,
    ) -> Result<PreparedThread, String> {
        self.ensure_server()?;
        let (_, params) =
            prepare_thread_request_params(".", Some(thread_id), permission_mode, approval_policy);
        Ok(PreparedThread {
            thread_id: thread_id.to_string(),
            thread_name,
            permission_profile: profile_from_params(&params, direct_profile()),
            model: None,
            reasoning_effort: None,
            service_tier: None,
        })
    }

    pub fn collaboration_mode(&self, thread_id: &str) -> Result<String, String> {
        self.collaboration_modes
            .lock()
            .map_err(|_| "Could not read the Codex collaboration mode".to_string())
            .map(|modes| {
                modes
                    .get(thread_id)
                    .cloned()
                    .unwrap_or_else(|| "default".into())
            })
    }

    pub fn set_thread_name(&self, thread_id: &str, name: &str) -> Result<(), String> {
        self.ensure_server()?;
        set_thread_name_connection(thread_id, name)
    }

    pub fn fork_thread_at_response(
        &self,
        thread_id: &str,
        last_turn_id: Option<&str>,
        prompt: &str,
        response: &str,
    ) -> Result<String, String> {
        self.ensure_server()?;
        let mut server = connect_initialized_plain()?;
        let stored_turn_id = match last_turn_id.filter(|id| !id.trim().is_empty()) {
            Some(turn_id) => turn_id.to_string(),
            None => find_stored_turn_id(&mut server, thread_id, prompt, response)?,
        };

        send_json(
            &mut server,
            json!({
                "method": "thread/fork",
                "id": 100,
                "params": { "threadId": thread_id, "lastTurnId": stored_turn_id }
            }),
        )?;
        let fork = wait_for_plain_value_response(&mut server, 100)?;
        fork.pointer("/result/thread/id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| "Codex did not return the forked thread id".to_string())
    }

    pub fn fork_thread_at_latest_turn(&self, thread_id: &str) -> Result<String, String> {
        self.ensure_server()?;
        let mut server = connect_initialized_plain()?;
        send_json(
            &mut server,
            json!({
                "method": "thread/fork",
                "id": 100,
                "params": { "threadId": thread_id }
            }),
        )?;
        let fork = wait_for_plain_value_response(&mut server, 100)?;
        fork.pointer("/result/thread/id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| "Codex App Server did not return the forked thread id".into())
    }

    pub fn list_skills(&self, cwd: &str) -> Result<Value, String> {
        self.ensure_server()?;
        let mut server = connect_initialized_plain()?;
        send_json(
            &mut server,
            json!({
                "method": "skills/list",
                "id": 100,
                "params": { "cwds": [cwd] }
            }),
        )?;
        wait_for_plain_value_response(&mut server, 100)
    }

    pub fn thread_model_settings(
        &self,
        thread_id: &str,
    ) -> Result<CodexThreadModelSettings, String> {
        self.ensure_server()?;
        // Do not resume a live thread just to read its model. The model is cached
        // from the session's existing start/resume response instead.
        let mut settings = default_model_settings_connection()?;
        let cached = self
            .thread_settings
            .lock()
            .map_err(|_| "Could not read the Codex thread model settings".to_string())?
            .get(thread_id)
            .cloned()
            .unwrap_or_default();
        if let Some(model) = cached.model {
            if let Some(option) = settings.models.iter().find(|option| option.model == model) {
                settings.model = model;
                settings.reasoning_effort = cached
                    .reasoning_effort
                    .filter(|effort| {
                        option
                            .supported_reasoning_efforts
                            .iter()
                            .any(|supported| supported.value == *effort)
                    })
                    .or_else(|| Some(option.default_reasoning_effort.clone()));
            }
        }
        if cached.service_tier.is_some() {
            settings.service_tier = cached.service_tier;
        }
        Ok(settings)
    }

    pub fn set_thread_model_settings(
        &self,
        thread_id: &str,
        model: &str,
        effort: &str,
        models: &[CodexModelOption],
    ) -> Result<CodexThreadModelSettings, String> {
        self.ensure_server()?;
        let settings = set_thread_model_settings_connection(thread_id, model, effort, models)?;
        self.update_thread_settings(
            thread_id,
            CachedCodexThreadSettings {
                model: Some(settings.model.clone()),
                reasoning_effort: settings.reasoning_effort.clone(),
                service_tier: settings.service_tier.clone(),
            },
        )?;
        Ok(settings)
    }

    pub fn default_model_settings(&self) -> Result<CodexThreadModelSettings, String> {
        self.ensure_server()?;
        default_model_settings_connection()
    }

    pub fn set_thread_fast_mode(&self, thread_id: &str, enabled: bool) -> Result<bool, String> {
        self.ensure_server()?;
        let mut server = connect_initialized_plain()?;
        send_json(&mut server, thread_fast_mode_request(thread_id, enabled))?;
        let response = wait_for_plain_value_response(&mut server, 2)?;
        let enabled = confirmed_fast_mode(&response, enabled)?;
        self.update_thread_settings(
            thread_id,
            CachedCodexThreadSettings {
                service_tier: Some(if enabled { "fast" } else { "default" }.into()),
                ..CachedCodexThreadSettings::default()
            },
        )?;
        Ok(enabled)
    }

    pub fn set_collaboration_mode(
        &self,
        thread_id: &str,
        mode: &str,
        state: &AppState,
        app: &AppHandle,
    ) -> Result<String, String> {
        if !matches!(mode, "default" | "plan") {
            return Err("Unsupported Codex collaboration mode".into());
        }
        self.ensure_server()?;
        update_thread_collaboration_mode(thread_id, mode, state, app)?;
        self.collaboration_modes
            .lock()
            .map_err(|_| "Could not save the Codex collaboration mode".to_string())?
            .insert(thread_id.to_string(), mode.to_string());
        Ok(mode.to_string())
    }

    pub fn submit_prompt(
        &self,
        thread_id: &str,
        prompt: &str,
        attachment_paths: &[String],
        profile: PermissionProfile,
        state: AppState,
        app: AppHandle,
    ) -> Result<(), String> {
        self.ensure_server()?;
        match self.submit_through_active_proxy(thread_id, prompt, attachment_paths, profile.clone())
        {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(_error) if state.codex_active_turn(thread_id)?.is_some() => return Ok(()),
            Err(error) => return Err(error),
        }
        let monitor_profile = profile.clone();
        let mut server =
            match prompt_connection(thread_id, prompt, attachment_paths, profile, &state, &app) {
                Ok(server) => server,
                Err(_error) if state.codex_active_turn(thread_id)?.is_some() => {
                    // turn/started is stronger evidence than a lost JSON-RPC reply.
                    // Do not invite the caller to resend a turn that Codex accepted.
                    return Ok(());
                }
                Err(error) => return Err(error),
            };
        let thread_id = thread_id.to_string();
        let process = self.process.clone();
        thread::Builder::new()
            .name("lume-codex-prompt".into())
            .spawn(move || {
                if let Err(error) = monitor_prompt(
                    &mut server,
                    &thread_id,
                    monitor_profile,
                    &state,
                    &app,
                    &process,
                ) {
                    eprintln!("Prompt do Lume encerrado: {error}");
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn recover_thread_and_submit_prompt(
        &self,
        session_id: &str,
        working_directory: &str,
        prompt: &str,
        attachment_paths: &[String],
        profile: PermissionProfile,
        model_settings: SessionModelOverride,
        state: AppState,
        app: AppHandle,
    ) -> Result<(), String> {
        self.ensure_server()?;
        let mut server = connect_server()?;
        set_server_timeout(&mut server, Duration::from_secs(5))?;
        send_json(
            &mut server,
            json!({
                "method": "initialize",
                "id": 1,
                "params": {
                    "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                    "capabilities": { "experimentalApi": true }
                }
            }),
        )?;
        wait_for_plain_value_response(&mut server, 1)?;
        send_json(
            &mut server,
            json!({ "method": "initialized", "params": {} }),
        )?;

        let (_, mut params) = prepare_thread_request_params(
            working_directory,
            None,
            Some(&profile.mode),
            Some(&profile.approval_policy),
        );
        apply_model_override_to_thread_start(&mut params, &model_settings);
        send_json(
            &mut server,
            json!({ "method": "thread/start", "id": 2, "params": params }),
        )?;
        let response = wait_for_plain_value_response(&mut server, 2)?;
        let thread_id = response
            .pointer("/result/thread/id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "The Codex App Server did not return the thread id".to_string())?
            .to_string();
        state.rebind_codex_thread(session_id, thread_id.clone())?;

        let monitor_profile = profile.clone();
        let profiles = HashMap::from([(thread_id.clone(), profile)]);
        let mut turn = prompt_turn_request(&thread_id, prompt, attachment_paths);
        apply_model_override_to_turn_request(&mut turn, &model_settings);
        apply_permission_override(&state, &mut turn, &thread_id)?;
        send_json(&mut server, turn)?;
        if let Err(error) = wait_for_response_until(
            &mut server,
            3,
            &state,
            &app,
            &profiles,
            CODEX_PROMPT_ACK_TIMEOUT,
        ) {
            if state.codex_active_turn(&thread_id)?.is_none() {
                return Err(error);
            }
        }
        set_server_timeout(&mut server, Duration::from_millis(200))?;
        let process = self.process.clone();
        thread::Builder::new()
            .name("lume-codex-recovered-prompt".into())
            .spawn(move || {
                if let Err(error) = monitor_prompt(
                    &mut server,
                    &thread_id,
                    monitor_profile,
                    &state,
                    &app,
                    &process,
                ) {
                    eprintln!("Recovered Lume prompt ended: {error}");
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn submit_through_active_proxy(
        &self,
        thread_id: &str,
        prompt: &str,
        attachment_paths: &[String],
        profile: PermissionProfile,
    ) -> Result<bool, String> {
        let active = self
            .active_proxy_threads
            .lock()
            .map_err(|_| "Could not access active Codex sessions".to_string())?
            .get(thread_id)
            .filter(|connection| connection.ready)
            .cloned();
        let Some(active) = active else {
            return Ok(false);
        };
        let request_id = format!(
            "lume-prompt:{}",
            NEXT_PROXY_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
        );
        let (response, receiver) = mpsc::channel();
        if active
            .sender
            .send(ProxyPrompt {
                request_id,
                thread_id: thread_id.to_string(),
                prompt: prompt.to_string(),
                attachment_paths: attachment_paths.to_vec(),
                profile,
                response,
            })
            .is_err()
        {
            if let Ok(mut threads) = self.active_proxy_threads.lock() {
                threads.retain(|_, connection| connection.connection_id != active.connection_id);
            }
            return Ok(false);
        }
        let outcome = receiver
            .recv_timeout(Duration::from_secs(8))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    "The active Codex session did not acknowledge the prompt in time".to_string()
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    "The active Codex session closed before accepting the prompt".to_string()
                }
            });
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                if let Ok(mut threads) = self.active_proxy_threads.lock() {
                    threads
                        .retain(|_, connection| connection.connection_id != active.connection_id);
                }
                return Err(error);
            }
        };
        outcome?;
        Ok(true)
    }

    pub fn steer_prompt(
        &self,
        thread_id: &str,
        prompt: &str,
        attachment_paths: &[String],
        state: &AppState,
        app: &AppHandle,
    ) -> Result<(), String> {
        self.ensure_server()?;
        let (mut server, turn_id, profiles) = active_turn_connection(thread_id, state, app)?;
        send_json(
            &mut server,
            json!({
                "method": "turn/steer",
                "id": 3,
                "params": {
                    "threadId": thread_id,
                    "expectedTurnId": turn_id,
                    "input": prompt_input(prompt, attachment_paths)
                }
            }),
        )?;
        wait_for_response(&mut server, 3, state, app, &profiles)
    }

    pub fn queue_prompt(
        &self,
        session_id: &str,
        activity_id: &str,
        thread_id: &str,
        prompt: &str,
        attachment_paths: &[String],
        profile: PermissionProfile,
    ) -> Result<(), String> {
        let mut queues = self
            .queued_prompts
            .lock()
            .map_err(|_| "Could not access the Codex prompt queue".to_string())?;
        let total = queues.values().map(VecDeque::len).sum::<usize>();
        if total >= MAX_TOTAL_QUEUED_PROMPTS {
            return Err("The Codex prompt queue is full. Send or discard a queued prompt before adding another.".into());
        }
        let queue = queues.entry(thread_id.to_string()).or_default();
        if queue.len() >= MAX_QUEUED_PROMPTS_PER_THREAD {
            return Err("This Codex session already has too many queued prompts.".into());
        }
        queue.push_back(QueuedPrompt {
            session_id: session_id.to_string(),
            activity_id: activity_id.to_string(),
            prompt: prompt.to_string(),
            attachment_paths: attachment_paths.to_vec(),
            profile,
        });
        Ok(())
    }

    pub fn steer_queued_prompt(
        &self,
        session_id: &str,
        activity_id: &str,
        thread_id: &str,
        state: &AppState,
        app: &AppHandle,
    ) -> Result<(), String> {
        let (queued, queue_index) = {
            let mut queues = self
                .queued_prompts
                .lock()
                .map_err(|_| "Could not access the Codex prompt queue".to_string())?;
            let queue = queues
                .get_mut(thread_id)
                .ok_or_else(|| "Queued prompt not found".to_string())?;
            let queue_index = queue
                .iter()
                .position(|queued| {
                    queued.session_id == session_id && queued.activity_id == activity_id
                })
                .ok_or_else(|| "Queued prompt not found".to_string())?;
            let queued = queue
                .remove(queue_index)
                .ok_or_else(|| "Queued prompt not found".to_string())?;
            (queued, queue_index)
        };

        if let Err(error) = self.steer_prompt(
            thread_id,
            &queued.prompt,
            &queued.attachment_paths,
            state,
            app,
        ) {
            if let Ok(mut queues) = self.queued_prompts.lock() {
                let queue = queues.entry(thread_id.to_string()).or_default();
                queue.insert(queue_index.min(queue.len()), queued);
            }
            return Err(error);
        }

        state.promote_queued_prompt_activity(session_id, activity_id)?;
        state.clear_queued_prompt(activity_id)
    }

    pub fn interrupt_prompt(
        &self,
        thread_id: &str,
        state: &AppState,
        app: &AppHandle,
    ) -> Result<(), String> {
        self.ensure_server()?;
        let (mut server, turn_id, profiles) = active_turn_connection(thread_id, state, app)?;
        send_json(
            &mut server,
            json!({
                "method": "turn/interrupt",
                "id": 3,
                "params": { "threadId": thread_id, "turnId": turn_id }
            }),
        )?;
        wait_for_response(&mut server, 3, state, app, &profiles)
    }

    pub fn discard_queued_prompts(&self, thread_id: &str) -> Result<(), String> {
        self.queued_prompts
            .lock()
            .map_err(|_| "Could not access the Codex prompt queue".to_string())?
            .remove(thread_id);
        Ok(())
    }

    pub fn refresh_rate_limits(&self, state: &AppState, app: &AppHandle) -> Result<(), String> {
        self.ensure_server()?;
        let mut server = connect_server()?;
        set_server_timeout(&mut server, Duration::from_secs(5))?;
        let profiles = HashMap::new();
        send_json(
            &mut server,
            json!({
                "method": "initialize",
                "id": 1,
                "params": {
                    "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                    "capabilities": { "experimentalApi": true }
                }
            }),
        )?;
        wait_for_response(&mut server, 1, state, app, &profiles)?;
        send_json(
            &mut server,
            json!({ "method": "initialized", "params": {} }),
        )?;
        send_json(
            &mut server,
            json!({ "method": "account/rateLimits/read", "id": 2, "params": null }),
        )?;
        wait_for_rate_limits_response(&mut server, state, app)
    }
}

fn proxy_thread_ready(threads: &ActiveProxyThreads, thread_id: &str) -> Result<bool, String> {
    threads
        .lock()
        .map_err(|_| "Could not verify the Codex terminal connection".to_string())
        .map(|threads| {
            threads
                .get(thread_id)
                .is_some_and(|connection| connection.ready)
        })
}

fn ensure_server_process(process_slot: &Mutex<Option<ManagedChild>>) -> Result<(), String> {
    if server_available() {
        return Ok(());
    }
    let mut stored_process = process_slot
        .lock()
        .map_err(|_| "Não foi possível guardar o processo do Codex".to_string())?;
    if server_available() {
        return Ok(());
    }
    if let Some(process) = stored_process.as_mut() {
        match probe_existing_server(process, server_available, Duration::from_secs(2))? {
            ExistingServer::Ready => return Ok(()),
            ExistingServer::Exited(status) => {
                eprintln!("Lume Codex App Server exited: {status}");
                stored_process.take();
            }
            ExistingServer::Unresponsive => {
                return Err(
                    "The Codex App Server is still running but its port is temporarily unavailable; Lume will not interrupt the active prompt"
                        .into(),
                );
            }
        }
    }
    let mut process = ManagedChild::spawn(command_for_server()?)
        .map_err(|error| format!("Could not start `codex app-server`: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if server_available() {
            *stored_process = Some(process);
            return Ok(());
        }
        if process
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("O servidor do Codex encerrou antes de ficar disponível".into());
        }
        thread::sleep(Duration::from_millis(80));
    }
    *stored_process = Some(process);
    Err("The Codex App Server is still starting; try again in a moment".into())
}

enum ExistingServer {
    Ready,
    Exited(ExitStatus),
    Unresponsive,
}

fn probe_existing_server(
    process: &mut Child,
    available: impl Fn() -> bool,
    grace: Duration,
) -> Result<ExistingServer, String> {
    // A busy App Server can miss a short TCP probe while a turn is still
    // running. Never kill it just because the port was briefly unavailable.
    let deadline = Instant::now() + grace;
    loop {
        if available() {
            return Ok(ExistingServer::Ready);
        }
        if let Some(status) = process.try_wait().map_err(|error| error.to_string())? {
            return Ok(ExistingServer::Exited(status));
        }
        if Instant::now() >= deadline {
            return Ok(ExistingServer::Unresponsive);
        }
        thread::sleep(
            Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn cleanup_orphaned_server() -> Result<(), String> {
    let own_pid = get_current_pid().ok();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    let candidates = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            if Some(*pid) == own_pid || !is_lume_server_command(process.cmd()) {
                return None;
            }
            let owned_by_live_lume = process
                .parent()
                .and_then(|parent| system.process(parent))
                .is_some_and(|parent| {
                    let is_lume = parent
                        .name()
                        .to_string_lossy()
                        .trim_end_matches(".exe")
                        .eq_ignore_ascii_case("lume");
                    #[cfg(target_os = "macos")]
                    let is_lume = is_lume || is_process_supervisor_command(parent.cmd());
                    is_lume
                });
            (!owned_by_live_lume).then_some(*pid)
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(());
    }

    for pid in &candidates {
        if let Some(process) = system.process(*pid) {
            let _ = process.kill_with(Signal::Term);
        }
    }
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(50));
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&candidates),
            true,
            ProcessRefreshKind::nothing(),
        );
        if candidates.iter().all(|pid| system.process(*pid).is_none()) {
            return Ok(());
        }
    }
    for pid in &candidates {
        if let Some(process) = system.process(*pid) {
            let _ = process.kill();
        }
    }
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(50));
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&candidates),
            true,
            ProcessRefreshKind::nothing(),
        );
        if candidates.iter().all(|pid| system.process(*pid).is_none()) {
            return Ok(());
        }
    }
    Err("Could not release the orphaned Codex app-server from the previous Lume instance".into())
}

#[cfg(target_os = "macos")]
fn is_process_supervisor_command(command: &[std::ffi::OsString]) -> bool {
    command
        .get(1)
        .is_some_and(|argument| argument == crate::macos_process_supervisor::CLI_COMMAND)
}

fn is_lume_server_command(command: &[std::ffi::OsString]) -> bool {
    #[cfg(target_os = "macos")]
    if is_process_supervisor_command(command) {
        // The supervisor's arguments contain the app-server command too, but
        // it must stay alive long enough to kill and reap its own child.
        return false;
    }
    let command = command
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    command.contains("app-server")
        && (command.contains(server_url()) || command.contains(LEGACY_SERVER_URL))
}

impl Drop for CodexBridge {
    fn drop(&mut self) {
        if !self.owns_process {
            return;
        }
        if let Ok(mut process) = self.process.lock() {
            if let Some(process) = process.as_mut() {
                let _ = process.kill();
                let _ = process.wait();
            }
        }
    }
}

fn command_for_server() -> Result<Command, String> {
    let mut command = crate::executables::command("codex")?;
    command
        .args(["app-server", "--listen", server_url()])
        .env("LUME_MANAGED_SESSION", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;

        let lume_pid = unsafe { libc::getpid() };
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != lume_pid {
                    return Err(std::io::Error::new(
                        ErrorKind::BrokenPipe,
                        "Lume exited while starting the Codex app-server",
                    ));
                }
                Ok(())
            });
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    Ok(command)
}

fn server_available() -> bool {
    server_url()
        .trim_start_matches("ws://")
        .parse()
        .ok()
        .and_then(|address| TcpStream::connect_timeout(&address, Duration::from_millis(120)).ok())
        .is_some()
}

fn connect_server() -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    // Normal Lume operations resume without turns. Keep a bounded allowance for
    // control reads while preventing a malformed or exceptionally large local
    // response from reserving up to a gigabyte in the desktop process.
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_LOCAL_CODEX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_LOCAL_CODEX_FRAME_BYTES));
    connect_with_config(server_url(), Some(config), 3)
        .map(|(server, _)| server)
        .map_err(|error| error.to_string())
}

fn server_url() -> &'static str {
    SERVER_URL.get_or_init(|| {
        TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .map(|address| format!("ws://{address}"))
            .unwrap_or_else(|_| LEGACY_SERVER_URL.into())
    })
}

fn connect_managed_server(
    process: &Mutex<Option<ManagedChild>>,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let mut last_error = None;
    for attempt in 0..3 {
        ensure_server_process(process)?;
        match connect_server() {
            Ok(server) => return Ok(server),
            Err(error) => {
                last_error = Some(error);
                if attempt < 2 {
                    thread::sleep(Duration::from_millis(180));
                }
            }
        }
    }
    Err(format!(
        "Could not connect the Codex terminal to the local App Server: {}",
        last_error.unwrap_or_else(|| "unknown WebSocket error".into())
    ))
}

fn start_queue_dispatcher(
    process: Arc<Mutex<Option<ManagedChild>>>,
    queued_prompts: Arc<Mutex<HashMap<String, VecDeque<QueuedPrompt>>>>,
    state: AppState,
    app: AppHandle,
) -> Result<(), String> {
    thread::Builder::new()
        .name("lume-codex-queue".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(1));
            let sessions = match state.sessions() {
                Ok(sessions) => sessions,
                Err(_) => continue,
            };
            let candidates = {
                let mut queues = match queued_prompts.lock() {
                    Ok(queues) => queues,
                    Err(_) => continue,
                };
                queues.retain(|thread_id, queue| {
                    !queue.is_empty()
                        && sessions.iter().any(|session| {
                            session.native_session_id.as_deref() == Some(thread_id.as_str())
                        })
                });
                queues.keys().cloned().collect::<Vec<_>>()
            };
            if candidates.is_empty() || ensure_server_process(&process).is_err() {
                continue;
            }
            for thread_id in candidates {
                if sessions.iter().any(|session| {
                    session.native_session_id.as_deref() == Some(thread_id.as_str())
                        && matches!(
                            session.status,
                            SessionStatus::Running | SessionStatus::PermissionRequired
                        )
                }) {
                    continue;
                }
                let queued = queued_prompts
                    .lock()
                    .ok()
                    .and_then(|mut queues| queues.get_mut(&thread_id)?.pop_front());
                let Some(queued) = queued else {
                    continue;
                };
                match prompt_connection(
                    &thread_id,
                    &queued.prompt,
                    &queued.attachment_paths,
                    queued.profile.clone(),
                    &state,
                    &app,
                ) {
                    Ok(mut server) => {
                        if let Err(error) = state
                            .promote_queued_prompt_activity(&queued.session_id, &queued.activity_id)
                        {
                            eprintln!("Could not promote the queued prompt: {error}");
                        }
                        if let Err(error) = state.clear_queued_prompt(&queued.activity_id) {
                            eprintln!("Could not clear the queued prompt journal: {error}");
                        }
                        crate::protocol::emit_sessions_changed(&app);
                        let monitor_thread = thread_id.clone();
                        let monitor_state = state.clone();
                        let monitor_app = app.clone();
                        let monitor_process = process.clone();
                        let _ = thread::Builder::new()
                            .name("lume-codex-queued-prompt".into())
                            .spawn(move || {
                                if let Err(error) = monitor_prompt(
                                    &mut server,
                                    &monitor_thread,
                                    queued.profile,
                                    &monitor_state,
                                    &monitor_app,
                                    &monitor_process,
                                ) {
                                    eprintln!("Prompt enfileirado do Lume encerrado: {error}");
                                }
                            });
                    }
                    Err(error) => {
                        eprintln!("Prompt enfileirado do Lume aguardando nova tentativa: {error}");
                        if let Ok(mut queues) = queued_prompts.lock() {
                            queues.entry(thread_id).or_default().push_front(queued);
                        }
                    }
                }
            }
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

const CODEX_CLI_QUEUE_POLL_INTERVAL: Duration = Duration::from_secs(3);
const CODEX_CLI_QUEUE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CODEX_CLI_QUEUE_THREADS: usize = 128;
const MAX_CODEX_CLI_QUEUE_PAGES: usize = 5;

fn start_codex_cli_queue_observer(
    process: Arc<Mutex<Option<ManagedChild>>>,
    state: AppState,
    app: AppHandle,
) -> Result<(), String> {
    thread::Builder::new()
        .name("lume-codex-cli-queue".into())
        .spawn(move || loop {
            thread::sleep(CODEX_CLI_QUEUE_POLL_INTERVAL);
            let mut targets = match state.connected_sessions() {
                Ok(sessions) => sessions
                    .into_iter()
                    .filter(|session| session.agent == AgentKind::Codex)
                    .filter_map(|session| {
                        session
                            .native_session_id
                            .map(|thread_id| (thread_id, session.updated_at))
                    })
                    .collect::<Vec<_>>(),
                Err(_) => continue,
            };
            targets.sort_by(|left, right| right.1.cmp(&left.1));
            let mut seen_targets = HashMap::new();
            targets.retain(|(thread_id, _)| seen_targets.insert(thread_id.clone(), ()).is_none());
            targets.truncate(MAX_CODEX_CLI_QUEUE_THREADS);
            if targets.is_empty() {
                continue;
            }

            let Ok(mut server) = connect_managed_server(&process) else {
                continue;
            };
            let initialized = send_json(
                &mut server,
                json!({
                    "method": "initialize",
                    "id": 1,
                    "params": {
                        "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                        "capabilities": { "experimentalApi": true }
                    }
                }),
            )
            .and_then(|()| {
                wait_for_plain_value_response_until(
                    &mut server,
                    1,
                    CODEX_CLI_QUEUE_RESPONSE_TIMEOUT,
                )
                .map(|_| ())
            })
            .and_then(|()| {
                send_json(
                    &mut server,
                    json!({ "method": "initialized", "params": {} }),
                )
            });
            if initialized.is_err() {
                continue;
            }

            let mut request_id = 2;
            for (thread_id, _) in targets {
                let submissions = match list_codex_cli_queue(&mut server, &thread_id, &mut request_id)
                {
                    Ok(submissions) => submissions,
                    Err(error) if error.contains("did not respond within") => break,
                    Err(_) => continue,
                };
                if let Ok(Some((session_id, true))) =
                    state.sync_codex_cli_queued_prompts(&thread_id, &submissions)
                {
                    crate::protocol::emit_session_changed(
                        &app,
                        &session_id,
                        Some(&thread_id),
                    );
                }
            }
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn list_codex_cli_queue(
    server: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    thread_id: &str,
    request_id: &mut i64,
) -> Result<Vec<(String, String)>, String> {
    let mut submissions = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_CODEX_CLI_QUEUE_PAGES {
        let mut params = json!({ "threadId": thread_id, "limit": 100 });
        if let Some(cursor) = cursor.as_deref() {
            params["cursor"] = Value::String(cursor.to_string());
        }
        let current_id = *request_id;
        *request_id += 1;
        send_json(
            server,
            json!({ "method": "thread/queue/list", "id": current_id, "params": params }),
        )?;
        let response = wait_for_plain_value_response_until(
            server,
            current_id,
            CODEX_CLI_QUEUE_RESPONSE_TIMEOUT,
        )?;
        let result = response
            .get("result")
            .ok_or_else(|| "Codex returned an invalid queue response".to_string())?;
        let page = result
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| "Codex returned an invalid queue page".to_string())?;
        for submission in page {
            let Some(id) = submission.get("id").and_then(Value::as_str) else {
                continue;
            };
            let prompt = submission
                .get("input")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    (item.get("type").and_then(Value::as_str) == Some("text"))
                        .then(|| item.get("text").and_then(Value::as_str))
                        .flatten()
                })
                .map(str::to_string)
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            submissions.push((id.to_string(), prompt));
        }
        cursor = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    Ok(submissions)
}

fn proxy_connection(
    stream: TcpStream,
    state: AppState,
    app: AppHandle,
    active_proxy_threads: ActiveProxyThreads,
    process: Arc<Mutex<Option<ManagedChild>>>,
    thread_settings: Arc<Mutex<HashMap<String, CachedCodexThreadSettings>>>,
    expected_token: &str,
) -> Result<(), String> {
    let mut client = accept_hdr(
        stream,
        |request: &tungstenite::handshake::server::Request, response| {
            if proxy_request_authorized(request, expected_token) {
                Ok(response)
            } else {
                Err(tungstenite::http::Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .body(Some("Unauthorized Lume Codex bridge".into()))
                    .expect("valid unauthorized WebSocket response"))
            }
        },
    )
    .map_err(|error| error.to_string())?;
    let mut server = connect_managed_server(&process)?;
    configure_client_timeout(&mut client)?;
    configure_server_timeout(&mut server)?;
    let connection_id = NEXT_PROXY_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
    let (proxy_sender, proxy_receiver) = mpsc::channel::<ProxyPrompt>();
    let mut pending_prompts = HashMap::new();
    let mut pending_resumes = HashMap::new();
    let mut profiles = HashMap::new();
    let mut responses = HashMap::new();

    let result = (|| -> Result<(), String> {
        loop {
            while let Ok(request) = proxy_receiver.try_recv() {
                profiles.insert(request.thread_id.clone(), request.profile);
                let mut turn = prompt_turn_request_with_id(
                    &request.thread_id,
                    &request.prompt,
                    &request.attachment_paths,
                    Value::String(request.request_id.clone()),
                );
                let settings = state
                    .session_model_override_for_native_id(AgentKind::Codex, &request.thread_id)?;
                apply_model_override_to_turn_request(&mut turn, &settings);
                apply_permission_override(&state, &mut turn, &request.thread_id)?;
                server
                    .send(Message::Text(turn.to_string().into()))
                    .map_err(|error| error.to_string())?;
                pending_prompts.insert(request.request_id, request.response);
            }

            match client.read() {
                Ok(message) => {
                    let closing = matches!(message, Message::Close(_));
                    if let Some((request_id, thread_id)) = proxy_client_resume_request(&message) {
                        pending_resumes.insert(request_id, thread_id.clone());
                        active_proxy_threads
                            .lock()
                            .map_err(|_| {
                                "Could not register the resumed Codex session".to_string()
                            })?
                            .insert(
                                thread_id,
                                ActiveProxyConnection {
                                    connection_id,
                                    sender: proxy_sender.clone(),
                                    ready: false,
                                },
                            );
                    }
                    let message = apply_client_model_override(message, &state)?;
                    observe_client_message(&message, &mut profiles);
                    server.send(message).map_err(|error| error.to_string())?;
                    if closing {
                        break;
                    }
                }
                Err(tungstenite::Error::ConnectionClosed) => break,
                Err(tungstenite::Error::Io(error)) if transient(&error) => {}
                Err(error) => return Err(error.to_string()),
            }

            match server.read() {
                Ok(message) => {
                    let closing = matches!(message, Message::Close(_));
                    if let Some((request_id, accepted)) = proxy_response_outcome(&message) {
                        if let Some(thread_id) = pending_resumes.remove(&request_id) {
                            if accepted {
                                if let Some(settings) = proxy_thread_settings_response(&message) {
                                    let _ = update_cached_thread_settings(
                                        &thread_settings,
                                        &thread_id,
                                        settings,
                                    );
                                }
                            }
                            if let Ok(mut threads) = active_proxy_threads.lock() {
                                if accepted {
                                    if let Some(connection) = threads.get_mut(&thread_id) {
                                        if connection.connection_id == connection_id {
                                            connection.ready = true;
                                        }
                                    }
                                } else {
                                    threads.retain(|id, connection| {
                                        id != &thread_id
                                            || connection.connection_id != connection_id
                                    });
                                }
                            }
                        }
                    }
                    if let Some((thread_id, started)) = proxy_thread_lifecycle(&message) {
                        if started {
                            active_proxy_threads
                                .lock()
                                .map_err(|_| {
                                    "Could not register the active Codex session".to_string()
                                })?
                                .insert(
                                    thread_id,
                                    ActiveProxyConnection {
                                        connection_id,
                                        sender: proxy_sender.clone(),
                                        ready: true,
                                    },
                                );
                        } else {
                            if let Ok(mut threads) = active_proxy_threads.lock() {
                                threads.retain(|id, connection| {
                                    id != &thread_id || connection.connection_id != connection_id
                                });
                            }
                        }
                    }
                    if let Some((request_id, outcome)) = proxy_prompt_response(&message) {
                        if let Some(response) = pending_prompts.remove(&request_id) {
                            let _ = response.send(outcome);
                            continue;
                        }
                    }
                    if let Some(response) =
                        intercept_server_message(&message, &state, &app, &profiles, &mut responses)?
                    {
                        server.send(response).map_err(|error| error.to_string())?;
                    } else {
                        client.send(message).map_err(|error| error.to_string())?;
                    }
                    if closing {
                        break;
                    }
                }
                Err(tungstenite::Error::ConnectionClosed) => break,
                Err(tungstenite::Error::Io(error)) if transient(&error) => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(())
    })();

    if let Ok(mut threads) = active_proxy_threads.lock() {
        threads.retain(|_, connection| connection.connection_id != connection_id);
    }
    for response in pending_prompts.into_values() {
        let _ = response.send(Err(
            "The active Codex session closed before accepting the prompt".into(),
        ));
    }
    result
}

fn proxy_request_authorized(
    request: &tungstenite::handshake::server::Request,
    expected_token: &str,
) -> bool {
    if request.headers().contains_key("origin") {
        return false;
    }
    request
        .uri()
        .query()
        .and_then(|query| {
            query
                .split('&')
                .find_map(|part| part.strip_prefix("token="))
        })
        .is_some_and(|token| token == expected_token)
}

fn random_secret(bytes: usize) -> Result<String, String> {
    let mut value = vec![0_u8; bytes];
    getrandom::getrandom(&mut value).map_err(|error| error.to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

fn proxy_client_resume_request(message: &Message) -> Option<(String, String)> {
    let Message::Text(text) = message else {
        return None;
    };
    let value = serde_json::from_str::<Value>(text).ok()?;
    if value.get("method").and_then(Value::as_str)? != "thread/resume" {
        return None;
    }
    Some((
        proxy_request_id(value.get("id")?)?,
        text_at(value.get("params")?, "threadId")?.to_string(),
    ))
}

fn proxy_response_outcome(message: &Message) -> Option<(String, bool)> {
    let Message::Text(text) = message else {
        return None;
    };
    let value = serde_json::from_str::<Value>(text).ok()?;
    if value.get("method").is_some() {
        return None;
    }
    Some((
        proxy_request_id(value.get("id")?)?,
        value.get("error").is_none(),
    ))
}

fn proxy_thread_settings_response(message: &Message) -> Option<CachedCodexThreadSettings> {
    let Message::Text(text) = message else {
        return None;
    };
    let response = serde_json::from_str::<Value>(text).ok()?;
    cached_thread_settings_from_response(&response)
}

fn proxy_request_id(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn proxy_thread_lifecycle(message: &Message) -> Option<(String, bool)> {
    let Message::Text(text) = message else {
        return None;
    };
    let value = serde_json::from_str::<Value>(text).ok()?;
    match value.get("method").and_then(Value::as_str)? {
        "thread/started" => Some((
            text_at(value.get("params")?.get("thread")?, "id")?.to_string(),
            true,
        )),
        "thread/closed" => Some((
            text_at(value.get("params")?, "threadId")?.to_string(),
            false,
        )),
        _ => None,
    }
}

fn proxy_prompt_response(message: &Message) -> Option<(String, Result<(), String>)> {
    let Message::Text(text) = message else {
        return None;
    };
    let value = serde_json::from_str::<Value>(text).ok()?;
    let request_id = value.get("id")?.as_str()?.to_string();
    if !request_id.starts_with("lume-prompt:") {
        return None;
    }
    let outcome = value.get("error").map_or(Ok(()), |error| {
        Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex refused the prompt")
            .to_string())
    });
    Some((request_id, outcome))
}

fn configure_client_timeout(socket: &mut WebSocket<TcpStream>) -> Result<(), String> {
    socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(45)))
        .map_err(|error| error.to_string())
}

fn configure_server_timeout(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<(), String> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream
            .set_read_timeout(Some(Duration::from_millis(45)))
            .map_err(|error| error.to_string()),
        _ => Err("O Codex local deve usar uma conexão WebSocket sem TLS".into()),
    }
}

fn transient(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

fn prompt_connection(
    thread_id: &str,
    prompt: &str,
    attachment_paths: &[String],
    profile: PermissionProfile,
    state: &AppState,
    app: &AppHandle,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let mut server = connect_server()?;
    set_server_timeout(&mut server, Duration::from_secs(5))?;
    let mut profiles = HashMap::from([(thread_id.to_string(), profile)]);

    send_json(
        &mut server,
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true }
            }
        }),
    )?;
    wait_for_response(&mut server, 1, state, app, &profiles)?;
    send_json(
        &mut server,
        json!({ "method": "initialized", "params": {} }),
    )?;
    send_json(
        &mut server,
        json!({ "method": "thread/resume", "id": 2, "params": { "threadId": thread_id, "excludeTurns": true } }),
    )?;
    wait_for_response_until(&mut server, 2, state, app, &profiles, CODEX_RESUME_TIMEOUT)?;

    let mut turn = prompt_turn_request(thread_id, prompt, attachment_paths);
    let model_settings = state.session_model_override_for_native_id(AgentKind::Codex, thread_id)?;
    apply_model_override_to_turn_request(&mut turn, &model_settings);
    apply_permission_override(state, &mut turn, thread_id)?;
    observe_client_message(&Message::Text(turn.to_string().into()), &mut profiles);
    send_json(&mut server, turn)?;
    wait_for_response_until(
        &mut server,
        3,
        state,
        app,
        &profiles,
        CODEX_PROMPT_ACK_TIMEOUT,
    )?;
    set_server_timeout(&mut server, Duration::from_millis(200))?;
    Ok(server)
}

fn prepare_thread_connection(
    working_directory: &str,
    resume_id: Option<&str>,
    permission_mode: Option<&AccessMode>,
    approval_policy: Option<&str>,
) -> Result<PreparedThread, String> {
    let mut server = connect_server()?;
    set_server_timeout(&mut server, Duration::from_secs(5))?;

    send_json(
        &mut server,
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true }
            }
        }),
    )?;
    wait_for_plain_value_response(&mut server, 1)?;
    send_json(
        &mut server,
        json!({ "method": "initialized", "params": {} }),
    )?;

    let (method, params) = prepare_thread_request_params(
        working_directory,
        resume_id,
        permission_mode,
        approval_policy,
    );
    let permission_profile = profile_from_params(&params, direct_profile());
    send_json(
        &mut server,
        json!({ "method": method, "id": 2, "params": params }),
    )?;
    // Resuming a long-lived thread can take substantially longer than the
    // five-second socket timeout. A timed-out read is not a failed takeover.
    let response = if resume_id.is_some() {
        wait_for_plain_value_response_until(&mut server, 2, CODEX_RESUME_TIMEOUT)?
    } else {
        wait_for_plain_value_response(&mut server, 2)?
    };
    let thread_id = response
        .pointer("/result/thread/id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "The Codex App Server did not return the thread id".to_string())?
        .to_string();
    let thread_name = response
        .pointer("/result/thread/name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let settings = cached_thread_settings_from_response(&response).unwrap_or_default();

    Ok(PreparedThread {
        thread_id,
        thread_name,
        permission_profile,
        model: settings.model,
        reasoning_effort: settings.reasoning_effort,
        service_tier: settings.service_tier,
    })
}

fn set_thread_name_connection(thread_id: &str, name: &str) -> Result<(), String> {
    let mut server = connect_server()?;
    set_server_timeout(&mut server, Duration::from_secs(5))?;
    send_json(
        &mut server,
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true }
            }
        }),
    )?;
    wait_for_plain_value_response(&mut server, 1)?;
    send_json(
        &mut server,
        json!({ "method": "initialized", "params": {} }),
    )?;
    send_json(&mut server, thread_name_request(thread_id, name))?;
    wait_for_plain_value_response(&mut server, 2)?;
    Ok(())
}

fn prepare_thread_request_params(
    working_directory: &str,
    resume_id: Option<&str>,
    permission_mode: Option<&AccessMode>,
    approval_policy: Option<&str>,
) -> (&'static str, Value) {
    let mut params = serde_json::Map::new();
    params.insert("cwd".into(), json!(working_directory));
    let method = if let Some(thread_id) = resume_id {
        params.insert("threadId".into(), json!(thread_id));
        params.insert("excludeTurns".into(), json!(true));
        "thread/resume"
    } else {
        params.insert("serviceName".into(), json!("lume"));
        "thread/start"
    };
    if let Some(sandbox) = permission_mode.and_then(|mode| match mode {
        AccessMode::ReadOnly | AccessMode::Plan => Some("read-only"),
        AccessMode::WorkspaceWrite => Some("workspace-write"),
        AccessMode::FullAccess => Some("danger-full-access"),
        AccessMode::Custom => None,
    }) {
        params.insert("sandbox".into(), json!(sandbox));
    }
    if approval_policy.is_some_and(|policy| matches!(policy, "untrusted" | "on-request" | "never"))
    {
        params.insert("approvalPolicy".into(), json!(approval_policy));
    }
    (method, Value::Object(params))
}

fn thread_name_request(thread_id: &str, name: &str) -> Value {
    json!({
        "method": "thread/name/set",
        "id": 2,
        "params": {
            "threadId": thread_id,
            "name": name
        }
    })
}

fn prompt_turn_request(thread_id: &str, prompt: &str, attachment_paths: &[String]) -> Value {
    prompt_turn_request_with_id(thread_id, prompt, attachment_paths, json!(3))
}

fn prompt_turn_request_with_id(
    thread_id: &str,
    prompt: &str,
    attachment_paths: &[String],
    request_id: Value,
) -> Value {
    json!({
        "method": "turn/start",
        "id": request_id,
        "params": {
            "threadId": thread_id,
            "input": prompt_input(prompt, attachment_paths)
        }
    })
}

fn apply_model_override_to_turn_request(
    request: &mut Value,
    settings: &SessionModelOverride,
) -> bool {
    let Some(params) = request.get_mut("params").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    if let Some(model) = settings.model.as_deref() {
        params.insert("model".into(), json!(model));
        changed = true;
    }
    if let Some(effort) = settings.reasoning_effort.as_deref() {
        params.insert("effort".into(), json!(effort));
        changed = true;
    }
    changed
}

/// Runs the turn in the permission mode picked in Lume, replacing what the thread had.
fn apply_permission_override_to_turn_request(request: &mut Value, mode: Option<&str>) -> bool {
    let Some(params) = mode
        .and_then(crate::codex_permissions::turn_params)
        .zip(request.get_mut("params").and_then(Value::as_object_mut))
    else {
        return false;
    };
    let (permission, turn) = params;
    for (key, value) in permission {
        turn.insert(key.into(), value);
    }
    true
}

fn apply_permission_override(
    state: &AppState,
    request: &mut Value,
    thread_id: &str,
) -> Result<bool, String> {
    let mode = state.permission_mode_override_for_native_id(AgentKind::Codex, thread_id)?;
    Ok(apply_permission_override_to_turn_request(
        request,
        mode.as_deref(),
    ))
}

fn apply_client_model_override(message: Message, state: &AppState) -> Result<Message, String> {
    let Message::Text(text) = message else {
        return Ok(message);
    };
    let Ok(mut request) = serde_json::from_str::<Value>(&text) else {
        return Ok(Message::Text(text));
    };
    if request.get("method").and_then(Value::as_str) != Some("turn/start") {
        return Ok(Message::Text(text));
    }
    let Some(thread_id) = request
        .pointer("/params/threadId")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(Message::Text(text));
    };
    let settings = state.session_model_override_for_native_id(AgentKind::Codex, &thread_id)?;
    let model_changed = apply_model_override_to_turn_request(&mut request, &settings);
    let permission_changed = apply_permission_override(state, &mut request, &thread_id)?;
    if !model_changed && !permission_changed {
        return Ok(Message::Text(text));
    }
    Ok(Message::Text(request.to_string().into()))
}

fn prompt_input(prompt: &str, attachment_paths: &[String]) -> Vec<Value> {
    let mut input = Vec::new();
    if !prompt.is_empty() {
        input.push(json!({ "type": "text", "text": prompt }));
        input.extend(crate::agent_commands::codex_skill_inputs(prompt));
    }
    input.extend(
        attachment_paths
            .iter()
            .map(|path| json!({ "type": "localImage", "path": path })),
    );
    input
}

fn connect_initialized_plain() -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let timeout_message = rpc_timeout_message(CODEX_REQUEST_TIMEOUT);
    connect_initialized_plain_with_timeout(CODEX_REQUEST_TIMEOUT, &timeout_message)
}

fn connect_initialized_plain_with_timeout(
    timeout: Duration,
    timeout_message: &str,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    connect_initialized_plain_until(Instant::now() + timeout, timeout_message)
}

fn connect_initialized_plain_until(
    deadline: Instant,
    timeout_message: &str,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let mut server = connect_server()?;
    set_server_timeout(&mut server, Duration::from_secs(5))?;
    send_json(
        &mut server,
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true }
            }
        }),
    )?;
    wait_for_plain_value_response_until_deadline(&mut server, 1, deadline, timeout_message)?;
    send_json(
        &mut server,
        json!({ "method": "initialized", "params": {} }),
    )?;
    Ok(server)
}

fn set_thread_model_settings_connection(
    thread_id: &str,
    model: &str,
    effort: &str,
    models: &[CodexModelOption],
) -> Result<CodexThreadModelSettings, String> {
    let mut server = connect_initialized_plain()?;
    apply_thread_model_settings(&mut server, thread_id, model, effort, models)
}

fn apply_thread_model_settings(
    server: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    thread_id: &str,
    model: &str,
    effort: &str,
    models: &[CodexModelOption],
) -> Result<CodexThreadModelSettings, String> {
    send_json(
        server,
        thread_model_settings_update_request(thread_id, model, effort),
    )?;
    let resumed = wait_for_plain_value_response(server, 2)?;
    // The caller already loaded and validated the catalog. Older servers may
    // still report the last turn's model, so retain the acknowledged overrides
    // also saved by the caller and applied explicitly to the next turn.
    let mut settings = model_settings_from_models(&resumed, models.to_vec())?;
    settings.model = model.to_string();
    settings.reasoning_effort = Some(effort.to_string());
    Ok(settings)
}

fn default_model_settings_connection() -> Result<CodexThreadModelSettings, String> {
    let timeout_message = codex_model_settings_timeout_message();
    let deadline = Instant::now() + CODEX_MODEL_SETTINGS_TIMEOUT;
    let mut server = connect_initialized_plain_until(deadline, &timeout_message)?;
    send_json(
        &mut server,
        json!({
            "method": "model/list",
            "id": 2,
            "params": { "limit": 100, "includeHidden": false }
        }),
    )?;
    let catalog =
        wait_for_plain_value_response_until_deadline(&mut server, 2, deadline, &timeout_message)?;
    default_model_settings_from_catalog(&catalog)
}

fn thread_fast_mode_request(thread_id: &str, enabled: bool) -> Value {
    json!({
        "method": "thread/resume",
        "id": 2,
        "params": { "threadId": thread_id, "excludeTurns": true, "serviceTier": if enabled { "fast" } else { "default" } }
    })
}

fn confirmed_fast_mode(response: &Value, enabled: bool) -> Result<bool, String> {
    let actual = response
        .pointer("/result/serviceTier")
        .and_then(Value::as_str);
    match actual {
        Some("fast") if enabled => Ok(true),
        Some("default" | "standard") if !enabled => Ok(false),
        None if !enabled => Ok(false),
        _ => Err("Codex did not confirm the requested Fast mode".into()),
    }
}

fn thread_model_settings_update_request(thread_id: &str, model: &str, effort: &str) -> Value {
    json!({
        "method": "thread/resume",
        "id": 2,
        "params": {
            "threadId": thread_id,
            "excludeTurns": true,
            "model": model,
            "config": { "model_reasoning_effort": effort }
        }
    })
}

fn model_settings_from_responses(
    resumed: &Value,
    catalog: &Value,
) -> Result<CodexThreadModelSettings, String> {
    let models = catalog
        .pointer("/result/data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Codex did not return its model catalog".to_string())?
        .iter()
        .filter_map(|value| {
            let model = value.get("model")?.as_str()?.trim();
            if model.is_empty() {
                return None;
            }
            let efforts = value
                .get("supportedReasoningEfforts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|effort| {
                    Some(CodexReasoningEffortOption {
                        value: effort.get("reasoningEffort")?.as_str()?.to_string(),
                        description: effort
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    })
                })
                .collect::<Vec<_>>();
            let default_reasoning_effort = value
                .get("defaultReasoningEffort")
                .and_then(Value::as_str)
                .or_else(|| efforts.first().map(|effort| effort.value.as_str()))?
                .to_string();
            Some(CodexModelOption {
                model: model.to_string(),
                display_name: value
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(model)
                    .to_string(),
                description: value
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                is_default: value
                    .get("isDefault")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                default_reasoning_effort,
                supported_reasoning_efforts: efforts,
            })
        })
        .collect::<Vec<_>>();
    model_settings_from_models(resumed, models)
}

fn model_settings_from_models(
    resumed: &Value,
    models: Vec<CodexModelOption>,
) -> Result<CodexThreadModelSettings, String> {
    let model = resumed
        .pointer("/result/model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Codex did not report the current thread model".to_string())?
        .to_string();
    let reasoning_effort = resumed
        .pointer("/result/reasoningEffort")
        .and_then(Value::as_str)
        .map(str::to_string);
    let service_tier = resumed
        .pointer("/result/serviceTier")
        .and_then(Value::as_str)
        .map(str::to_string);
    if models.is_empty() {
        return Err("Codex did not return any available models".into());
    }
    Ok(CodexThreadModelSettings {
        model,
        reasoning_effort,
        service_tier,
        models,
        session_modes: None,
    })
}

fn default_model_settings_from_catalog(
    catalog: &Value,
) -> Result<CodexThreadModelSettings, String> {
    let available = catalog
        .pointer("/result/data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Codex did not return its model catalog".to_string())?;
    let selected = available
        .iter()
        .find(|model| model.get("isDefault").and_then(Value::as_bool) == Some(true))
        .or_else(|| available.first())
        .ok_or_else(|| "Codex did not return any available models".to_string())?;
    let model = selected
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| "Codex returned an invalid default model".to_string())?;
    let effort = selected
        .get("defaultReasoningEffort")
        .and_then(Value::as_str);
    model_settings_from_responses(
        &json!({
            "result": {
                "model": model,
                "reasoningEffort": effort,
                "serviceTier": Value::Null
            }
        }),
        catalog,
    )
}

fn apply_model_override_to_thread_start(params: &mut Value, settings: &SessionModelOverride) {
    let Some(params) = params.as_object_mut() else {
        return;
    };
    if let Some(model) = settings.model.as_deref() {
        params.insert("model".into(), json!(model));
    }
    if let Some(effort) = settings.reasoning_effort.as_deref() {
        params.insert("config".into(), json!({ "model_reasoning_effort": effort }));
    }
}

fn update_thread_collaboration_mode(
    thread_id: &str,
    mode: &str,
    state: &AppState,
    app: &AppHandle,
) -> Result<(), String> {
    let mut server = connect_server()?;
    set_server_timeout(&mut server, Duration::from_secs(5))?;
    let profiles = HashMap::from([(thread_id.to_string(), direct_profile())]);

    send_json(
        &mut server,
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true }
            }
        }),
    )?;
    wait_for_response(&mut server, 1, state, app, &profiles)?;
    send_json(
        &mut server,
        json!({ "method": "initialized", "params": {} }),
    )?;
    send_json(
        &mut server,
        json!({ "method": "thread/resume", "id": 2, "params": { "threadId": thread_id, "excludeTurns": true } }),
    )?;
    let resumed =
        wait_for_value_response_until(&mut server, 2, state, app, &profiles, CODEX_RESUME_TIMEOUT)?;
    send_json(
        &mut server,
        json!({ "method": "collaborationMode/list", "id": 3, "params": {} }),
    )?;
    let presets = wait_for_value_response(&mut server, 3, state, app, &profiles)?;
    let collaboration_mode = collaboration_mode_from_responses(mode, &resumed, &presets)?;
    send_json(
        &mut server,
        json!({
            "method": "thread/settings/update",
            "id": 4,
            "params": {
                "threadId": thread_id,
                "collaborationMode": collaboration_mode
            }
        }),
    )?;
    wait_for_response(&mut server, 4, state, app, &profiles)
}

fn collaboration_mode_from_responses(
    mode: &str,
    resumed: &Value,
    presets: &Value,
) -> Result<Value, String> {
    let preset = presets
        .pointer("/result/data")
        .and_then(Value::as_array)
        .and_then(|presets| {
            presets.iter().find(|preset| {
                text_at(preset, "mode") == Some(mode)
                    || text_at(preset, "name").is_some_and(|name| name.eq_ignore_ascii_case(mode))
            })
        });
    let model = preset
        .and_then(|preset| text_at(preset, "model"))
        .or_else(|| {
            resumed
                .pointer("/result")
                .and_then(|result| text_at(result, "model"))
        })
        .ok_or_else(|| "Codex did not provide a model for this collaboration mode".to_string())?;
    let effort = preset
        .and_then(|preset| preset.get("reasoning_effort"))
        .filter(|value| !value.is_null())
        .cloned()
        .or_else(|| {
            resumed
                .pointer("/result/effort")
                .filter(|value| !value.is_null())
                .cloned()
        });
    let mut settings = json!({
        "model": model,
        "developer_instructions": Value::Null
    });
    if let Some(effort) = effort {
        settings["reasoning_effort"] = effort;
    }
    Ok(json!({
        "mode": mode,
        "settings": settings
    }))
}

fn active_turn_connection(
    thread_id: &str,
    state: &AppState,
    app: &AppHandle,
) -> Result<
    (
        WebSocket<MaybeTlsStream<TcpStream>>,
        String,
        HashMap<String, PermissionProfile>,
    ),
    String,
> {
    let (server, active_turn, profiles) = control_connection(thread_id, state, app)?;
    let turn_id = active_turn
        .ok_or_else(|| "This agent does not have a prompt running right now".to_string())?;
    Ok((server, turn_id, profiles))
}

fn control_connection(
    thread_id: &str,
    state: &AppState,
    app: &AppHandle,
) -> Result<
    (
        WebSocket<MaybeTlsStream<TcpStream>>,
        Option<String>,
        HashMap<String, PermissionProfile>,
    ),
    String,
> {
    let mut server = connect_server()?;
    set_server_timeout(&mut server, Duration::from_secs(5))?;
    let profiles = HashMap::from([(thread_id.to_string(), direct_profile())]);
    send_json(
        &mut server,
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true }
            }
        }),
    )?;
    wait_for_response(&mut server, 1, state, app, &profiles)?;
    send_json(
        &mut server,
        json!({ "method": "initialized", "params": {} }),
    )?;
    if let Some(turn_id) = state.codex_active_turn(thread_id)? {
        return Ok((server, Some(turn_id), profiles));
    }
    let expects_active_turn = state.native_session_has_active_task(&AgentKind::Codex, thread_id)?;
    if !expects_active_turn {
        return Ok((server, None, profiles));
    }
    send_json(
        &mut server,
        json!({
            "method": "thread/turns/list",
            "id": 2,
            "params": {
                "threadId": thread_id,
                "limit": 4,
                "sortDirection": "desc",
                "itemsView": "notLoaded"
            }
        }),
    )?;
    let response = wait_for_value_response(&mut server, 2, state, app, &profiles)?;
    let active_turn = response
        .pointer("/result/data")
        .and_then(Value::as_array)
        .and_then(|turns| {
            turns.iter().rev().find(|turn| {
                matches!(
                    turn.get("status").and_then(Value::as_str),
                    Some("inProgress" | "in_progress")
                )
            })
        })
        .and_then(|turn| text_at(turn, "id"))
        .map(str::to_string);
    Ok((server, active_turn, profiles))
}

fn set_server_timeout(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    timeout: Duration,
) -> Result<(), String> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream
            .set_read_timeout(Some(timeout))
            .map_err(|error| error.to_string()),
        _ => Err("O Codex local deve usar uma conexão WebSocket sem TLS".into()),
    }
}

fn send_json(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    value: Value,
) -> Result<(), String> {
    socket
        .send(Message::Text(value.to_string().into()))
        .map_err(|error| error.to_string())
}

fn wait_for_response(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    expected_id: i64,
    state: &AppState,
    app: &AppHandle,
    profiles: &HashMap<String, PermissionProfile>,
) -> Result<(), String> {
    wait_for_response_until(
        socket,
        expected_id,
        state,
        app,
        profiles,
        CODEX_REQUEST_TIMEOUT,
    )
}

fn wait_for_response_until(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    expected_id: i64,
    state: &AppState,
    app: &AppHandle,
    profiles: &HashMap<String, PermissionProfile>,
    timeout: Duration,
) -> Result<(), String> {
    wait_for_value_response_until(socket, expected_id, state, app, profiles, timeout).map(|_| ())
}

fn rpc_timeout_message(timeout: Duration) -> String {
    format!(
        "Codex did not respond within {} seconds. If you sent a prompt, check whether it started before sending it again.",
        timeout.as_secs()
    )
}

fn codex_model_settings_timeout_message() -> String {
    format!(
        "Codex did not respond while loading model settings within {} seconds. No prompt was sent; try opening the model selector again.",
        CODEX_MODEL_SETTINGS_TIMEOUT.as_secs()
    )
}

fn read_message_until(
    deadline: Instant,
    timeout_error: &str,
    mut read: impl FnMut(Duration) -> Result<Message, tungstenite::Error>,
) -> Result<Message, String> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(timeout_error.to_string());
        }
        match read(remaining.min(Duration::from_secs(5))) {
            Ok(message) => return Ok(message),
            Err(tungstenite::Error::Io(error)) if transient(&error) => {
                thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn read_rpc_message(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    deadline: Instant,
    timeout_error: &str,
) -> Result<Message, String> {
    read_message_until(deadline, timeout_error, |remaining| {
        let MaybeTlsStream::Plain(stream) = socket.get_mut() else {
            return Err(tungstenite::Error::Io(std::io::Error::new(
                ErrorKind::InvalidInput,
                "Local Codex must use a loopback WebSocket without TLS",
            )));
        };
        stream.set_read_timeout(Some(remaining))?;
        socket.read()
    })
}

fn rpc_response_value(message: &Message, expected_id: i64) -> Result<Option<Value>, String> {
    let Message::Text(text) = message else {
        return Ok(None);
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Ok(None);
    };
    // Server-side permission requests can use the same numeric id as a client
    // request. They must be handled, never mistaken for an acknowledgement.
    if value.get("method").is_some() || value.get("id").and_then(Value::as_i64) != Some(expected_id)
    {
        return Ok(None);
    }
    if let Some(error) = value.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex refused the request")
            .to_string());
    }
    Ok(value.get("result").is_some().then_some(value))
}

fn wait_for_plain_value_response(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    expected_id: i64,
) -> Result<Value, String> {
    wait_for_plain_value_response_until(socket, expected_id, CODEX_REQUEST_TIMEOUT)
}

fn wait_for_plain_value_response_until(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    expected_id: i64,
    timeout: Duration,
) -> Result<Value, String> {
    let timeout_error = rpc_timeout_message(timeout);
    wait_for_plain_value_response_with_timeout_message(socket, expected_id, timeout, &timeout_error)
}

fn wait_for_plain_value_response_with_timeout_message(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    expected_id: i64,
    timeout: Duration,
    timeout_error: &str,
) -> Result<Value, String> {
    let deadline = Instant::now() + timeout;
    wait_for_plain_value_response_until_deadline(socket, expected_id, deadline, timeout_error)
}

fn wait_for_plain_value_response_until_deadline(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    expected_id: i64,
    deadline: Instant,
    timeout_error: &str,
) -> Result<Value, String> {
    loop {
        let message = read_rpc_message(socket, deadline, timeout_error)?;
        if let Some(value) = rpc_response_value(&message, expected_id)? {
            return Ok(value);
        }
    }
}

fn wait_for_value_response(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    expected_id: i64,
    state: &AppState,
    app: &AppHandle,
    profiles: &HashMap<String, PermissionProfile>,
) -> Result<Value, String> {
    wait_for_value_response_until(
        socket,
        expected_id,
        state,
        app,
        profiles,
        CODEX_REQUEST_TIMEOUT,
    )
}

fn wait_for_value_response_until(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    expected_id: i64,
    state: &AppState,
    app: &AppHandle,
    profiles: &HashMap<String, PermissionProfile>,
    timeout: Duration,
) -> Result<Value, String> {
    let mut responses = HashMap::new();
    let deadline = Instant::now() + timeout;
    let timeout_error = rpc_timeout_message(timeout);
    loop {
        let message = read_rpc_message(socket, deadline, &timeout_error)?;
        if let Some(value) = rpc_response_value(&message, expected_id)? {
            return Ok(value);
        }
        if let Some(response) =
            intercept_server_message(&message, state, app, profiles, &mut responses)?
        {
            socket.send(response).map_err(|error| error.to_string())?;
        }
    }
}

fn wait_for_rate_limits_response(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    state: &AppState,
    app: &AppHandle,
) -> Result<(), String> {
    loop {
        let message = socket.read().map_err(|error| error.to_string())?;
        let Message::Text(text) = message else {
            continue;
        };
        let value = serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())?;
        if value.get("id").and_then(Value::as_i64) != Some(2) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("O Codex não informou os limites da conta")
                .to_string());
        }
        let limits = rate_limits_from_message(&value);
        if state.set_agent_rate_limits(AgentKind::Codex, limits)? {
            crate::protocol::emit_sessions_changed(app);
        }
        return Ok(());
    }
}

fn monitor_prompt(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    thread_id: &str,
    profile: PermissionProfile,
    state: &AppState,
    app: &AppHandle,
    process: &Mutex<Option<ManagedChild>>,
) -> Result<(), String> {
    let profiles = HashMap::from([(thread_id.to_string(), profile.clone())]);
    let mut responses = HashMap::new();
    loop {
        match socket.read() {
            Ok(message) => {
                let completed =
                    match &message {
                        Message::Text(text) => serde_json::from_str::<Value>(text)
                            .ok()
                            .is_some_and(|value| {
                                value.get("method").and_then(Value::as_str)
                                    == Some("turn/completed")
                                    && value
                                        .get("params")
                                        .and_then(|params| text_at(params, "threadId"))
                                        == Some(thread_id)
                            }),
                        _ => false,
                    };
                if let Some(response) =
                    intercept_server_message(&message, state, app, &profiles, &mut responses)?
                {
                    socket.send(response).map_err(|error| error.to_string())?;
                }
                if completed {
                    return Ok(());
                }
            }
            Err(tungstenite::Error::ConnectionClosed) => {
                match reconnect_prompt_monitor(socket, thread_id, &profile, state, app, process) {
                    Ok(true) => continue,
                    Ok(false) => settle_disconnected_prompt(thread_id, state, app),
                    // A transport failure is not a task failure. The rollout
                    // watcher will publish the actual outcome if the turn ends.
                    Err(error) => return Err(error),
                }
                return Ok(());
            }
            Err(tungstenite::Error::Io(error)) if transient(&error) => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                match reconnect_prompt_monitor(socket, thread_id, &profile, state, app, process) {
                    Ok(true) => continue,
                    Ok(false) => settle_disconnected_prompt(thread_id, state, app),
                    Err(reconnect_error) => {
                        return Err(format!("{error}; reconnect failed: {reconnect_error}"));
                    }
                }
                return Err(error.to_string());
            }
        }
    }
}

fn reconnect_prompt_monitor(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    thread_id: &str,
    profile: &PermissionProfile,
    state: &AppState,
    app: &AppHandle,
    process: &Mutex<Option<ManagedChild>>,
) -> Result<bool, String> {
    let profiles = HashMap::from([(thread_id.to_string(), profile.clone())]);
    let mut last_error = None;
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(180 * attempt));
        }
        let result = (|| -> Result<Option<WebSocket<MaybeTlsStream<TcpStream>>>, String> {
            let mut server = connect_managed_server(process)?;
            set_server_timeout(&mut server, Duration::from_secs(5))?;
            send_json(
                &mut server,
                json!({
                    "method": "initialize",
                    "id": 1,
                    "params": {
                        "clientInfo": { "name": "lume", "title": "Lume", "version": env!("CARGO_PKG_VERSION") },
                        "capabilities": { "experimentalApi": true }
                    }
                }),
            )?;
            wait_for_response(&mut server, 1, state, app, &profiles)?;
            send_json(
                &mut server,
                json!({ "method": "initialized", "params": {} }),
            )?;
            send_json(
                &mut server,
                json!({
                    "method": "thread/read",
                    "id": 2,
                    "params": { "threadId": thread_id, "includeTurns": false }
                }),
            )?;
            let status = wait_for_value_response(&mut server, 2, state, app, &profiles)?
                .pointer("/result/thread/status/type")
                .and_then(Value::as_str)
                .unwrap_or("notLoaded")
                .to_string();
            if status != "active" {
                return Ok(None);
            }
            send_json(
                &mut server,
                json!({
                    "method": "thread/resume",
                    "id": 3,
                    "params": { "threadId": thread_id, "excludeTurns": true }
                }),
            )?;
            let resumed = wait_for_value_response_until(
                &mut server,
                3,
                state,
                app,
                &profiles,
                CODEX_RESUME_TIMEOUT,
            )?;
            if resumed
                .pointer("/result/thread/status/type")
                .and_then(Value::as_str)
                .is_some_and(|status| status != "active")
            {
                return Ok(None);
            }
            set_server_timeout(&mut server, Duration::from_millis(200))?;
            Ok(Some(server))
        })();
        match result {
            Ok(Some(server)) => {
                *socket = server;
                return Ok(true);
            }
            Ok(None) => return Ok(false),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| "Could not reconnect the Codex prompt monitor".into()))
}

fn settle_disconnected_prompt(thread_id: &str, state: &AppState, app: &AppHandle) {
    // The rollout watcher is the durable source of truth after the live
    // transport closes. Let it publish task_complete/task_failed first.
    thread::sleep(Duration::from_millis(1_200));
    let _ = state.set_codex_active_turn(thread_id, None);
    if prompt_thread_already_finished(thread_id, state) {
        return;
    }
    let _ = event_server::publish_event(state, app, prompt_monitor_settled_event(thread_id));
}

fn prompt_thread_already_finished(thread_id: &str, state: &AppState) -> bool {
    state.sessions().ok().is_some_and(|sessions| {
        sessions.iter().any(|session| {
            session.native_session_id.as_deref() == Some(thread_id)
                && matches!(
                    session.status,
                    SessionStatus::Completed | SessionStatus::Failed
                )
        })
    })
}

fn prompt_monitor_settled_event(thread_id: &str) -> HookEvent {
    HookEvent {
        event: HookEventKind::WaitingForInput,
        session_id: session_id(thread_id),
        agent: AgentKind::Codex,
        agent_label: Some("Codex".into()),
        session_name: None,
        project: None,
        source: Some(SessionSource::Cli),
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status_label: Some("Esperando ação".into()),
        started_at: None,
        process_id: None,
        native_session_id: Some(thread_id.into()),
        working_directory: None,
        permission_profile: Some(direct_profile()),
        permission: None,
        question: None,
        last_response: None,
        activity: None,
        activities: Vec::new(),
        wait_for_decision: false,
    }
}

fn intercept_server_message(
    message: &Message,
    state: &AppState,
    app: &AppHandle,
    profiles: &HashMap<String, PermissionProfile>,
    responses: &mut HashMap<String, String>,
) -> Result<Option<Message>, String> {
    let Message::Text(text) = message else {
        return Ok(None);
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Ok(None);
    };
    let method = value.get("method").and_then(Value::as_str).unwrap_or("");
    let completed_thread_id = (method == "turn/completed")
        .then(|| {
            value
                .get("params")
                .and_then(|params| text_at(params, "threadId"))
                .map(str::to_string)
        })
        .flatten();
    if method == "turn/started" {
        if let Some(params) = value.get("params") {
            if let (Some(thread_id), Some(turn_id)) = (
                text_at(params, "threadId"),
                text_at(params, "turnId")
                    .or_else(|| params.get("turn").and_then(|turn| text_at(turn, "id"))),
            ) {
                let _ = state.set_codex_active_turn(thread_id, Some(turn_id));
            }
        }
    } else if method == "turn/completed" {
        if let Some(thread_id) = completed_thread_id.as_deref() {
            let _ = state.set_codex_active_turn(thread_id, None);
        }
    }
    if method == "account/rateLimits/updated" {
        let limits = rate_limits_from_message(&value);
        if !limits.is_empty() && state.set_agent_rate_limits(AgentKind::Codex, limits)? {
            crate::protocol::emit_sessions_changed(app);
        }
    }
    if method == "thread/tokenUsage/updated" {
        if let Some((thread_id, turn_id, total_tokens, input_tokens, output_tokens)) =
            turn_token_usage_from_message(&value)
        {
            let recorded = state.record_codex_turn_token_usage(
                thread_id,
                turn_id,
                total_tokens,
                input_tokens,
                output_tokens,
            );
            let finished = !matches!(
                state.session_status(&session_id(thread_id), Some(thread_id)),
                Ok(Some(SessionStatus::Running))
            );
            if recorded.is_ok() && finished {
                crate::protocol::emit_sessions_changed(app);
            }
        }
    }
    if is_approval(method) && value.get("id").is_some() {
        return approval_response(&value, method, state, app, profiles).map(Some);
    }
    if method == "item/tool/requestUserInput" && value.get("id").is_some() {
        return user_input_response(&value, state, app, profiles).map(Some);
    }
    remember_response(&value, method, responses);
    let activity_thread_id = value.get("params").and_then(|params| {
        text_at(params, "threadId")
            .map(str::to_string)
            .or_else(|| {
                text_at(params, "turnId")
                    .and_then(|turn_id| state.codex_thread_for_turn(turn_id).ok().flatten())
            })
            .or_else(|| {
                (matches!(method, "warning" | "configWarning") && profiles.len() == 1)
                    .then(|| profiles.keys().next().cloned())
                    .flatten()
            })
    });
    if let Some(event) = activity_event(&value, method, activity_thread_id.as_deref()) {
        let _ = event_server::publish_event(state, app, event);
    }
    if let Some(event) = notification_event(&value, method, profiles, responses) {
        let _ = event_server::publish_event(state, app, event);
    }
    Ok(None)
}

fn turn_token_usage_from_message(value: &Value) -> Option<(&str, &str, u64, u64, u64)> {
    let params = value.get("params")?;
    let usage = params.get("tokenUsage")?.get("last")?;
    Some((
        params.get("threadId")?.as_str()?,
        params.get("turnId")?.as_str()?,
        usage.get("totalTokens")?.as_u64()?,
        usage
            .get("inputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        usage
            .get("outputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    ))
}

fn rate_limits_from_message(value: &Value) -> Vec<AgentRateLimit> {
    let root = value
        .get("result")
        .or_else(|| value.get("params"))
        .unwrap_or(value);
    if let Some(snapshot) = root
        .get("rateLimits")
        .filter(|snapshot| !snapshot.is_null())
    {
        let id = snapshot
            .get("limitId")
            .and_then(Value::as_str)
            .unwrap_or("codex");
        let mut limits = Vec::new();
        append_rate_limit_windows(&mut limits, id, snapshot);
        if !limits.is_empty() {
            return limits;
        }
    }
    if let Some(buckets) = root.get("rateLimitsByLimitId").and_then(Value::as_object) {
        let mut limits = Vec::new();
        for (id, snapshot) in buckets {
            append_rate_limit_windows(&mut limits, id, snapshot);
        }
        if !limits.is_empty() {
            return limits;
        }
    }
    Vec::new()
}

fn append_rate_limit_windows(limits: &mut Vec<AgentRateLimit>, id: &str, snapshot: &Value) {
    for (kind, fallback) in [("primary", "Current"), ("secondary", "Weekly")] {
        let Some(window) = snapshot.get(kind).filter(|window| !window.is_null()) else {
            continue;
        };
        let used = window
            .get("usedPercent")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        let minutes = window.get("windowDurationMins").and_then(Value::as_i64);
        let resets_at = window.get("resetsAt").and_then(Value::as_i64).map(|value| {
            if value < 10_000_000_000 {
                value.saturating_mul(1_000)
            } else {
                value
            }
        });
        limits.push(AgentRateLimit {
            id: format!("{id}:{kind}"),
            label: rate_limit_window_label(minutes, fallback),
            used_percent: used,
            resets_at,
            window_minutes: minutes,
        });
    }
}

fn rate_limit_window_label(minutes: Option<i64>, fallback: &str) -> String {
    match minutes {
        Some(minutes) if minutes > 0 && minutes % (24 * 60) == 0 => {
            format!("{}d", minutes / (24 * 60))
        }
        Some(minutes) if minutes > 0 && minutes % 60 == 0 => format!("{}h", minutes / 60),
        Some(minutes) if minutes > 0 => format!("{minutes}m"),
        _ => fallback.into(),
    }
}

fn observe_client_message(message: &Message, profiles: &mut HashMap<String, PermissionProfile>) {
    let Message::Text(text) = message else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let method = value.get("method").and_then(Value::as_str).unwrap_or("");
    if !matches!(method, "thread/resume" | "turn/start") {
        return;
    }
    let Some(params) = value.get("params") else {
        return;
    };
    let Some(thread_id) = text_at(params, "threadId") else {
        return;
    };
    let current = profiles
        .get(thread_id)
        .cloned()
        .unwrap_or_else(direct_profile);
    profiles.insert(thread_id.into(), profile_from_params(params, current));
}

fn is_approval(method: &str) -> bool {
    matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
    )
}

fn user_input_response(
    value: &Value,
    state: &AppState,
    app: &AppHandle,
    profiles: &HashMap<String, PermissionProfile>,
) -> Result<Message, String> {
    let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
    let thread_id = text_at(&params, "threadId").unwrap_or("unknown");
    let item_id = text_at(&params, "itemId").unwrap_or("question");
    let request_id = format!("codex-question:{thread_id}:{item_id}");
    let questions = codex_questions(&params);
    if questions.is_empty() {
        let response = json!({
            "id": value.get("id").cloned().unwrap_or(Value::Null),
            "result": { "answers": {} }
        });
        return Ok(Message::Text(response.to_string().into()));
    }

    let event = HookEvent {
        event: HookEventKind::QuestionRequest,
        session_id: session_id(thread_id),
        agent: AgentKind::Codex,
        agent_label: Some("Codex".into()),
        session_name: None,
        project: None,
        source: None,
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status_label: Some("Aguardando sua resposta".into()),
        started_at: None,
        process_id: None,
        native_session_id: Some(thread_id.into()),
        working_directory: None,
        permission_profile: Some(
            profiles
                .get(thread_id)
                .cloned()
                .unwrap_or_else(direct_profile),
        ),
        permission: None,
        question: Some(PendingQuestion {
            id: request_id.clone(),
            questions,
            requested_at: now_millis().to_string(),
        }),
        last_response: None,
        activity: None,
        activities: Vec::new(),
        wait_for_decision: true,
    };
    event_server::publish_event(state, app, event)?;
    let timeout = params
        .get("autoResolutionMs")
        .and_then(Value::as_u64)
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(15 * 60));
    let answers = match state.wait_for_question_answer(&request_id, timeout)? {
        Some(answers) => answers,
        None => {
            state.expire_question(&request_id)?;
            crate::protocol::emit_sessions_changed(app);
            Vec::new()
        }
    };
    let answers = answers
        .into_iter()
        .map(|answer| (answer.question_id, json!({ "answers": answer.answers })))
        .collect::<serde_json::Map<_, _>>();
    let response = json!({
        "id": value.get("id").cloned().unwrap_or(Value::Null),
        "result": { "answers": answers }
    });
    Ok(Message::Text(response.to_string().into()))
}

fn codex_questions(params: &Value) -> Vec<InteractiveQuestion> {
    params
        .get("questions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|question| {
            let id = text_at(question, "id")?.to_string();
            Some(InteractiveQuestion {
                id,
                header: text_at(question, "header")
                    .unwrap_or("Question")
                    .to_string(),
                question: text_at(question, "question")
                    .unwrap_or_default()
                    .to_string(),
                is_other: question
                    .get("isOther")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                is_secret: question
                    .get("isSecret")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                options: question
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|option| {
                        Some(QuestionOption {
                            label: text_at(option, "label")?.to_string(),
                            description: text_at(option, "description")
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect(),
            })
        })
        .collect()
}

fn approval_response(
    value: &Value,
    method: &str,
    state: &AppState,
    app: &AppHandle,
    profiles: &HashMap<String, PermissionProfile>,
) -> Result<Message, String> {
    let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
    let thread_id = text_at(&params, "threadId").unwrap_or("unknown");
    let item_id = text_at(&params, "itemId").unwrap_or("approval");
    let permission_id = format!("codex:{thread_id}:{item_id}");
    let cwd = text_at(&params, "cwd").map(str::to_string);
    let (kind, summary, resource, risk) = permission_details(method, &params, cwd.as_deref());
    let profile = profiles
        .get(thread_id)
        .cloned()
        .unwrap_or_else(direct_profile);
    if let Some(result) = automatic_approval_result(&profile, method, &params) {
        let response =
            json!({ "id": value.get("id").cloned().unwrap_or(Value::Null), "result": result });
        return Ok(Message::Text(response.to_string().into()));
    }
    let event = HookEvent {
        event: HookEventKind::PermissionRequest,
        session_id: session_id(thread_id),
        agent: AgentKind::Codex,
        agent_label: Some("Codex".into()),
        session_name: None,
        project: cwd.as_deref().and_then(project_name),
        source: None,
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status_label: Some("Aguardando sua permissão".into()),
        started_at: None,
        process_id: None,
        native_session_id: Some(thread_id.into()),
        working_directory: cwd,
        permission_profile: Some(profile),
        permission: Some(PermissionRequest {
            id: permission_id.clone(),
            kind,
            summary,
            resource,
            risk,
            requested_at: now_millis().to_string(),
        }),
        question: None,
        last_response: None,
        activity: None,
        activities: Vec::new(),
        wait_for_decision: true,
    };
    event_server::publish_event(state, app, event)?;
    let action = state
        .wait_for_decision(&permission_id, Duration::from_secs(15 * 60))?
        .unwrap_or(PermissionAction::Deny);
    let result = decision_result(method, action, &params);
    let response =
        json!({ "id": value.get("id").cloned().unwrap_or(Value::Null), "result": result });
    Ok(Message::Text(response.to_string().into()))
}

fn permission_details(
    method: &str,
    params: &Value,
    cwd: Option<&str>,
) -> (String, String, String, String) {
    let reason = text_at(params, "reason");
    match method {
        "item/commandExecution/requestApproval" => (
            "command".into(),
            reason.unwrap_or("Executar comando").into(),
            text_at(params, "command")
                .unwrap_or("Comando não informado")
                .into(),
            if params
                .get("networkApprovalContext")
                .is_some_and(|value| !value.is_null())
            {
                "high".into()
            } else {
                "medium".into()
            },
        ),
        "item/fileChange/requestApproval" => (
            "file_change".into(),
            reason.unwrap_or("Alterar arquivos").into(),
            text_at(params, "grantRoot")
                .or(cwd)
                .unwrap_or("Arquivos da sessão")
                .into(),
            "medium".into(),
        ),
        _ => (
            "permissions".into(),
            reason.unwrap_or("Ampliar permissões da sessão").into(),
            cwd.unwrap_or("Recursos adicionais").into(),
            "high".into(),
        ),
    }
}

fn decision_result(method: &str, action: PermissionAction, params: &Value) -> Value {
    if method == "item/permissions/requestApproval" {
        let permissions = if action == PermissionAction::Deny {
            json!({})
        } else {
            params
                .get("permissions")
                .cloned()
                .unwrap_or_else(|| json!({}))
        };
        return json!({
            "permissions": permissions,
            "scope": if action == PermissionAction::AllowSession { "session" } else { "turn" }
        });
    }
    json!({
        "decision": match action {
            PermissionAction::AllowOnce => "accept",
            PermissionAction::AllowSession => "acceptForSession",
            PermissionAction::Deny | PermissionAction::OpenSource => "decline",
        }
    })
}

fn automatic_approval_result(
    profile: &PermissionProfile,
    method: &str,
    params: &Value,
) -> Option<Value> {
    profile
        .automatically_approves()
        .then(|| decision_result(method, PermissionAction::AllowOnce, params))
}

fn activity_event(
    value: &Value,
    method: &str,
    fallback_thread_id: Option<&str>,
) -> Option<HookEvent> {
    let params = value.get("params")?;
    let thread_id = text_at(params, "threadId").or(fallback_thread_id)?;
    let activity = match method {
        "item/started" | "item/completed" => codex_item_activity(
            thread_id,
            params.get("item")?,
            method == "item/completed",
            text_at(params, "turnId"),
        )?,
        "item/agentMessage/delta" => {
            codex_delta_activity(thread_id, params, "message", "Resposta do agente", "delta")?
        }
        "item/commandExecution/outputDelta" => {
            codex_delta_activity(thread_id, params, "command", "Saída do comando", "delta")?
        }
        "item/fileChange/outputDelta" => {
            codex_delta_activity(thread_id, params, "file", "Alteração de arquivos", "delta")?
        }
        "item/fileChange/patchUpdated" => {
            let files = item_files(params);
            SessionActivity {
                id: codex_activity_id(thread_id, params)?,
                kind: "file".into(),
                title: if files.len() == 1 {
                    files[0].clone()
                } else {
                    format!("{} arquivos alterados", files.len())
                },
                detail: first_value_text(params, &["changes"]),
                status: "running".into(),
                created_at: now_millis(),
                files,
                attachments: Vec::new(),
                append_detail: false,
            }
        }
        "item/plan/delta" => {
            codex_delta_activity(thread_id, params, "plan", "Plano atualizado", "delta")?
        }
        "item/reasoning/summaryTextDelta" => codex_delta_activity(
            thread_id,
            params,
            "analysis",
            "Resumo do raciocínio",
            "delta",
        )?,
        "turn/diff/updated" => {
            let diff = text_at(params, "diff")?;
            SessionActivity {
                id: format!(
                    "codex:{thread_id}:diff:{}",
                    text_at(params, "turnId").unwrap_or("turn")
                ),
                kind: "file".into(),
                title: "Alterações da tarefa".into(),
                detail: Some(truncate_text(diff, 32 * 1024)),
                status: "running".into(),
                created_at: now_millis(),
                files: files_from_diff(diff),
                attachments: Vec::new(),
                append_detail: false,
            }
        }
        "turn/plan/updated" => SessionActivity {
            id: format!(
                "codex:{thread_id}:plan:{}",
                text_at(params, "turnId").unwrap_or("turn")
            ),
            kind: "plan".into(),
            title: "Plano atualizado".into(),
            detail: Some(plan_text(params)),
            status: "running".into(),
            created_at: now_millis(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        },
        "warning" => {
            let message = text_at(params, "message")?;
            warning_activity(thread_id, "Aviso do Codex", message)
        }
        "configWarning" => {
            let summary = text_at(params, "summary").unwrap_or("Aviso de configuração");
            let detail = [
                Some(summary),
                text_at(params, "details"),
                text_at(params, "path"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
            warning_activity(thread_id, "Aviso de configuração", &detail)
        }
        _ => return None,
    };
    Some(HookEvent {
        event: HookEventKind::Activity,
        session_id: session_id(thread_id),
        agent: AgentKind::Codex,
        agent_label: Some("Codex".into()),
        session_name: None,
        project: None,
        source: None,
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status_label: None,
        started_at: None,
        process_id: None,
        native_session_id: Some(thread_id.into()),
        working_directory: None,
        permission_profile: None,
        permission: None,
        question: None,
        last_response: None,
        activity: Some(activity),
        activities: Vec::new(),
        wait_for_decision: false,
    })
}

fn warning_activity(thread_id: &str, title: &str, detail: &str) -> SessionActivity {
    let hash = detail
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    SessionActivity {
        id: format!("codex:{thread_id}:warning:{hash:x}"),
        kind: "warning".into(),
        title: title.into(),
        detail: Some(truncate_text(detail, 8 * 1024)),
        status: "warning".into(),
        created_at: now_millis(),
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    }
}

fn codex_delta_activity(
    thread_id: &str,
    params: &Value,
    kind: &str,
    title: &str,
    detail_key: &str,
) -> Option<SessionActivity> {
    Some(SessionActivity {
        id: codex_activity_id(thread_id, params)?,
        kind: kind.into(),
        title: title.into(),
        detail: first_value_text(params, &[detail_key]),
        status: "running".into(),
        created_at: now_millis(),
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: true,
    })
}

fn codex_activity_id(thread_id: &str, value: &Value) -> Option<String> {
    let item_id = text_at(value, "itemId").or_else(|| text_at(value, "id"))?;
    Some(match text_at(value, "turnId") {
        Some(turn_id) => format!("codex:{thread_id}:turn:{turn_id}:item:{item_id}"),
        None => format!("codex:{thread_id}:{item_id}"),
    })
}

fn codex_item_activity(
    thread_id: &str,
    item: &Value,
    completed: bool,
    turn_id: Option<&str>,
) -> Option<SessionActivity> {
    let item_type = text_at(item, "type")?;
    let item_id = text_at(item, "id")
        .map(str::to_string)
        .unwrap_or_else(|| format!("{item_type}:{}", now_millis()));
    let status = if item_type == "subAgentActivity" {
        match text_at(item, "kind") {
            Some("started" | "interacted") => "running",
            Some("completed") => "completed",
            Some("failed" | "errored") => "failed",
            Some("interrupted" | "shutdown" | "closed") => "interrupted",
            _ => return None,
        }
    } else if item_failed(item) {
        "failed"
    } else if completed {
        "completed"
    } else {
        "running"
    };
    let (kind, title, detail, files) = match item_type {
        "commandExecution" => {
            let command =
                value_text(item.get("command")).unwrap_or_else(|| "Comando em execução".into());
            let output = first_value_text(
                item,
                &["aggregatedOutput", "output", "stdout", "stderr", "result"],
            );
            (
                if is_test_command(&command) {
                    "test"
                } else {
                    "command"
                },
                truncate_text(&command, 240),
                output,
                Vec::new(),
            )
        }
        "fileChange" => {
            let files = item_files(item);
            let title = if files.is_empty() {
                "Arquivos alterados".into()
            } else if files.len() == 1 {
                files[0].clone()
            } else {
                format!("{} arquivos alterados", files.len())
            };
            let detail = first_value_text(item, &["diff", "patch", "changes"]);
            ("file", title, detail, files)
        }
        "mcpToolCall" | "toolCall" | "dynamicToolCall" => {
            let server = text_at(item, "server").unwrap_or("");
            let original_tool = text_at(item, "tool")
                .or_else(|| text_at(item, "name"))
                .unwrap_or("Ferramenta");
            let original_detail =
                first_value_text(item, &["arguments", "result", "contentItems", "error"]);
            let nested = (normalized_codex_tool_name(original_tool) == "exec")
                .then(|| nested_tool_source(item).and_then(nested_work_tracking_tool))
                .flatten();
            let (tool, detail) = nested
                .map(|(name, arguments)| (name, Some(arguments)))
                .unwrap_or((original_tool, original_detail));
            let normalized_tool = normalized_codex_tool_name(tool);
            let title = if normalized_tool == "update_plan" {
                "Plano atualizado".into()
            } else if item_type == "mcpToolCall" && server.is_empty() {
                format!("MCP · {tool}")
            } else if item_type == "mcpToolCall" {
                format!("MCP · {server} · {tool}")
            } else if server.is_empty() {
                tool.into()
            } else {
                format!("{server} · {tool}")
            };
            (
                if normalized_tool == "update_plan" {
                    "plan"
                } else {
                    "tool"
                },
                title,
                detail,
                Vec::new(),
            )
        }
        "collabAgentToolCall" => (
            "tool",
            format!(
                "Subagente · {}",
                text_at(item, "tool").unwrap_or("colaboração")
            ),
            first_value_text(item, &["prompt", "agentsStates", "receiverThreadIds"]),
            Vec::new(),
        ),
        "subAgentActivity" => (
            "subagent",
            format!(
                "Subagente · {}",
                text_at(item, "agentPath").unwrap_or("atividade")
            ),
            first_value_text(item, &["kind", "agentThreadId"]),
            Vec::new(),
        ),
        "webSearch" => (
            "tool",
            "Pesquisa na web".into(),
            first_value_text(item, &["query", "action"]),
            Vec::new(),
        ),
        "agentMessage" => (
            "message",
            "Resposta do agente".into(),
            first_value_text(item, &["text", "content"]),
            Vec::new(),
        ),
        "userMessage" => (
            "prompt",
            "Prompt enviado".into(),
            user_message_text(item),
            Vec::new(),
        ),
        "plan" => (
            "plan",
            "Plano atualizado".into(),
            first_value_text(item, &["text", "plan"]),
            Vec::new(),
        ),
        "reasoning" => (
            "analysis",
            if completed {
                "Análise concluída".into()
            } else {
                "Analisando a solicitação".into()
            },
            first_value_text(item, &["summary"]),
            Vec::new(),
        ),
        _ => return None,
    };
    Some(SessionActivity {
        id: if item_type == "subAgentActivity" {
            format!(
                "codex:{thread_id}:subagent:{}",
                text_at(item, "agentThreadId").unwrap_or(&item_id)
            )
        } else {
            turn_id.map_or_else(
                || format!("codex:{thread_id}:{item_id}"),
                |turn_id| format!("codex:{thread_id}:turn:{turn_id}:item:{item_id}"),
            )
        },
        kind: kind.into(),
        title,
        detail: detail.map(|detail| truncate_text(&detail, 16 * 1024)),
        status: status.into(),
        created_at: now_millis(),
        files,
        attachments: Vec::new(),
        append_detail: false,
    })
}

fn normalized_codex_tool_name(name: &str) -> &str {
    name.rsplit(['.', ':', '/']).next().unwrap_or(name)
}

fn nested_tool_source(item: &Value) -> Option<&str> {
    let arguments = item.get("arguments")?;
    arguments.as_str().or_else(|| {
        ["source", "code", "input"]
            .iter()
            .find_map(|key| arguments.get(*key).and_then(Value::as_str))
    })
}

fn user_message_text(item: &Value) -> Option<String> {
    let content = item.get("content")?.as_array()?;
    let text = content
        .iter()
        .filter_map(|part| {
            text_at(part, "text")
                .or_else(|| text_at(part, "url"))
                .map(str::to_string)
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn turn_matches_response(turn: &Value, prompt: &str, response: &str, strict: bool) -> bool {
    let Some(items) = turn.get("items").and_then(Value::as_array) else {
        return false;
    };
    let prompt_match = items.iter().any(|item| {
        text_at(item, "type") == Some("userMessage")
            && user_message_text(item).is_some_and(|value| comparable_message(&value, prompt))
    });
    if !prompt_match {
        return false;
    }
    if !strict || response.trim().is_empty() {
        return true;
    }
    items.iter().any(|item| {
        text_at(item, "type") == Some("agentMessage")
            && first_value_text(item, &["text", "content"])
                .is_some_and(|value| comparable_message(&value, response))
    })
}

fn find_stored_turn_id(
    server: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    thread_id: &str,
    prompt: &str,
    response: &str,
) -> Result<String, String> {
    let mut cursor: Option<String> = None;
    let mut prompt_only_match: Option<String> = None;
    for page in 0..20_i64 {
        let request_id = 2 + page;
        let mut params = json!({
            "threadId": thread_id,
            "limit": 50,
            "sortDirection": "desc",
            "itemsView": "full"
        });
        if let Some(cursor) = cursor.as_deref() {
            params["cursor"] = json!(cursor);
        }
        send_json(
            server,
            json!({ "method": "thread/turns/list", "id": request_id, "params": params }),
        )?;
        let page = wait_for_plain_value_response(server, request_id)?;
        let turns = page
            .pointer("/result/data")
            .and_then(Value::as_array)
            .ok_or_else(|| "Codex did not return the stored conversation turns".to_string())?;
        for turn in turns {
            if turn_matches_response(turn, prompt, response, true) {
                return text_at(turn, "id")
                    .map(str::to_string)
                    .ok_or_else(|| "The matching Codex turn has no id".to_string());
            }
            if prompt_only_match.is_none() && turn_matches_response(turn, prompt, response, false) {
                prompt_only_match = text_at(turn, "id").map(str::to_string);
            }
        }
        cursor = page
            .pointer("/result/nextCursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    prompt_only_match.ok_or_else(|| "Could not match this response to a stored Codex turn".into())
}

fn comparable_message(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let left = normalize(left);
    let right = normalize(right);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    left == right || left.contains(&right) || right.contains(&left)
}

fn files_from_diff(diff: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in diff.lines() {
        let Some(raw_path) = line
            .strip_prefix("+++ b/")
            .or_else(|| line.strip_prefix("--- a/"))
            .or_else(|| line.strip_prefix("*** Update File: "))
            .or_else(|| line.strip_prefix("*** Add File: "))
            .or_else(|| line.strip_prefix("*** Delete File: "))
            .or_else(|| line.split_once("*** Update File: ").map(|(_, path)| path))
            .or_else(|| line.split_once("*** Add File: ").map(|(_, path)| path))
            .or_else(|| line.split_once("*** Delete File: ").map(|(_, path)| path))
        else {
            continue;
        };
        let path = raw_path
            .split_once(" @@")
            .map_or(raw_path, |(path, _)| path);
        let path = path
            .split_once(" *** ")
            .map_or(path, |(path, _)| path)
            .trim();
        if path != "/dev/null" && !files.iter().any(|existing| existing == path) {
            files.push(path.to_string());
        }
    }
    files.truncate(48);
    files
}

fn plan_text(params: &Value) -> String {
    let mut lines = text_at(params, "explanation")
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(plan) = params.get("plan").and_then(Value::as_array) {
        lines.extend(plan.iter().filter_map(|entry| {
            let step = text_at(entry, "step")?;
            let marker = match text_at(entry, "status") {
                Some("completed") => "✓",
                Some("inProgress") => "●",
                _ => "○",
            };
            Some(format!("{marker} {step}"))
        }));
    }
    truncate_text(&lines.join("\n"), 16 * 1024)
}

fn item_failed(item: &Value) -> bool {
    matches!(
        text_at(item, "status"),
        Some("failed" | "error" | "declined" | "cancelled")
    ) || item.get("error").is_some_and(|error| !error.is_null())
}

fn is_test_command(command: &str) -> bool {
    let command = command.to_lowercase();
    [
        "npm test",
        "pnpm test",
        "yarn test",
        "cargo test",
        "dotnet test",
        "go test",
        "flutter test",
        "pytest",
        "vitest",
        "jest",
        "mvn test",
        "gradle test",
        "gradlew test",
    ]
    .iter()
    .any(|pattern| command.contains(pattern))
}

fn item_files(item: &Value) -> Vec<String> {
    let mut files = Vec::new();
    for key in ["path", "filePath", "file_path"] {
        if let Some(path) = text_at(item, key) {
            files.push(path.to_string());
        }
    }
    if let Some(changes) = item.get("changes").and_then(Value::as_array) {
        for change in changes {
            for key in ["path", "filePath", "file_path"] {
                if let Some(path) = text_at(change, key) {
                    if !files.iter().any(|existing| existing == path) {
                        files.push(path.to_string());
                    }
                    break;
                }
            }
        }
    }
    files.truncate(48);
    files
}

fn first_value_text(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value_text(value.get(*key)))
        .filter(|value| !value.trim().is_empty())
}

fn value_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Array(values) if values.iter().all(Value::is_string) => Some(
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Value::Null => None,
        value => serde_json::to_string_pretty(value).ok(),
    }
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let shortened = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

fn notification_event(
    value: &Value,
    method: &str,
    profiles: &HashMap<String, PermissionProfile>,
    responses: &mut HashMap<String, String>,
) -> Option<HookEvent> {
    let params = value.get("params")?;
    let (event, thread_id, status_label, cwd, name, started_at, last_response) = match method {
        "thread/started" => {
            let thread = params.get("thread")?;
            (
                HookEventKind::SessionStarted,
                text_at(thread, "id")?,
                "Sessão iniciada",
                text_at(thread, "cwd"),
                text_at(thread, "name"),
                thread
                    .get("createdAt")
                    .and_then(Value::as_i64)
                    .map(|value| value.to_string()),
                None,
            )
        }
        "thread/name/updated" => (
            HookEventKind::Activity,
            text_at(params, "threadId")?,
            "Nome atualizado",
            None,
            text_at(params, "threadName"),
            None,
            None,
        ),
        "turn/started" => {
            let thread_id = text_at(params, "threadId")?;
            responses.remove(thread_id);
            (
                HookEventKind::Running,
                thread_id,
                "Executando",
                None,
                None,
                None,
                None,
            )
        }
        "turn/completed" => {
            let thread_id = text_at(params, "threadId")?;
            let status = params
                .get("turn")
                .and_then(|turn| text_at(turn, "status"))
                .unwrap_or("completed");
            let (event, label) = match status {
                "failed" => (HookEventKind::Failed, "Tarefa encerrada com erro"),
                "interrupted" => (HookEventKind::WaitingForInput, "Prompt interrompido"),
                _ => (HookEventKind::Completed, "Tarefa finalizada"),
            };
            let last_response = responses
                .remove(thread_id)
                .or_else(|| response_from_turn(params));
            (event, thread_id, label, None, None, None, last_response)
        }
        // App Server emits thread/closed when an unsubscribed thread is merely
        // unloaded from memory. The persisted conversation still exists and
        // must remain available in Lume.
        "thread/closed" => (
            HookEventKind::WaitingForInput,
            text_at(params, "threadId")?,
            "Esperando ação",
            None,
            None,
            None,
            None,
        ),
        _ => return None,
    };
    Some(HookEvent {
        event,
        session_id: session_id(thread_id),
        agent: AgentKind::Codex,
        agent_label: Some("Codex".into()),
        session_name: name.map(str::to_string),
        project: cwd.and_then(project_name),
        source: Some(SessionSource::Cli),
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status_label: Some(status_label.into()),
        started_at,
        process_id: None,
        native_session_id: Some(thread_id.into()),
        working_directory: cwd.map(str::to_string),
        permission_profile: Some(
            profiles
                .get(thread_id)
                .cloned()
                .unwrap_or_else(direct_profile),
        ),
        permission: None,
        question: None,
        last_response,
        activity: None,
        activities: Vec::new(),
        wait_for_decision: false,
    })
}

fn remember_response(value: &Value, method: &str, responses: &mut HashMap<String, String>) {
    if method != "item/completed" {
        return;
    }
    let Some(params) = value.get("params") else {
        return;
    };
    let Some(thread_id) = text_at(params, "threadId") else {
        return;
    };
    let Some(item) = params.get("item") else {
        return;
    };
    if text_at(item, "type") != Some("agentMessage") {
        return;
    }
    let Some(text) = text_at(item, "text").and_then(response_text) else {
        return;
    };
    responses.insert(thread_id.to_string(), text);
}

fn response_from_turn(params: &Value) -> Option<String> {
    params
        .get("turn")?
        .get("items")?
        .as_array()?
        .iter()
        .rev()
        .find(|item| text_at(item, "type") == Some("agentMessage"))
        .and_then(|item| text_at(item, "text"))
        .and_then(response_text)
}

fn response_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    const LIMIT: usize = 32 * 1024;
    let mut response = value.chars().take(LIMIT).collect::<String>();
    if value.chars().count() > LIMIT {
        response.push('…');
    }
    Some(response)
}

fn direct_profile() -> PermissionProfile {
    PermissionProfile {
        mode: AccessMode::Custom,
        label: "Permissões desta sessão".into(),
        approval_policy: "Decisões encaminhadas pelo Codex App Server".into(),
        approvals_reviewer: None,
        can_respond_from_lume: true,
        available_actions: vec![
            PermissionAction::AllowOnce,
            PermissionAction::AllowSession,
            PermissionAction::Deny,
        ],
    }
}

fn profile_from_params(params: &Value, mut profile: PermissionProfile) -> PermissionProfile {
    let sandbox = text_at(params, "sandbox").or_else(|| {
        params
            .get("sandboxPolicy")
            .and_then(|policy| text_at(policy, "type"))
    });
    if let Some(sandbox) = sandbox {
        let (mode, label) = match sandbox {
            "danger-full-access" | "dangerFullAccess" => (AccessMode::FullAccess, "Acesso total"),
            "read-only" | "readOnly" => (AccessMode::ReadOnly, "Somente leitura"),
            "workspace-write" | "workspaceWrite" => {
                (AccessMode::WorkspaceWrite, "Acesso ao projeto")
            }
            _ => (AccessMode::Custom, "Permissões personalizadas"),
        };
        profile.mode = mode;
        profile.label = label.into();
    }
    if let Some(policy) = params
        .get("approvalPolicy")
        .filter(|value| !value.is_null())
    {
        profile.approval_policy = policy
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| "Política granular".into());
    }
    if let Some(reviewer) = params
        .get("approvalsReviewer")
        .or_else(|| params.get("approvals_reviewer"))
        .and_then(Value::as_str)
    {
        profile.approvals_reviewer = Some(reviewer.into());
    }
    profile
}

fn text_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn session_id(thread_id: &str) -> String {
    format!("codex-app-server:{thread_id}")
}

fn project_name(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_wait_retries_transient_socket_timeouts_without_resending_the_request() {
        let expected = Message::Text(json!({ "id": 3, "result": {} }).to_string().into());
        let mut reads = VecDeque::from([
            Err(tungstenite::Error::Io(ErrorKind::WouldBlock.into())),
            Err(tungstenite::Error::Io(ErrorKind::TimedOut.into())),
            Err(tungstenite::Error::Io(ErrorKind::Interrupted.into())),
            Ok(expected.clone()),
        ]);
        let received = read_message_until(
            Instant::now() + Duration::from_secs(1),
            "response timeout",
            |remaining| {
                assert!(!remaining.is_zero());
                reads.pop_front().expect("scripted socket read")
            },
        )
        .expect("acknowledgement after temporary socket errors");
        assert_eq!(received, expected);
        assert!(reads.is_empty());
    }

    #[test]
    fn rpc_wait_reports_a_timeout_instead_of_raw_would_block_os_errors() {
        let expected_error = rpc_timeout_message(CODEX_PROMPT_ACK_TIMEOUT);
        let error = read_message_until(
            Instant::now() + Duration::from_millis(25),
            &expected_error,
            |_| Err(tungstenite::Error::Io(ErrorKind::WouldBlock.into())),
        )
        .expect_err("bounded wait");
        assert_eq!(error, expected_error);
        assert!(!error.contains("Resource temporarily unavailable"));
    }

    #[test]
    fn rpc_socket_remains_readable_after_a_temporary_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("isolated test socket");
        let client =
            TcpStream::connect(listener.local_addr().expect("test address")).expect("test client");
        let (server, _) = listener.accept().expect("test server");
        let mut client = WebSocket::from_raw_socket(
            MaybeTlsStream::Plain(client),
            tungstenite::protocol::Role::Client,
            None,
        );
        let (release, released) = mpsc::channel();
        let sender = thread::spawn(move || {
            let mut server =
                WebSocket::from_raw_socket(server, tungstenite::protocol::Role::Server, None);
            let request = server.read().expect("one test request");
            assert!(matches!(request, Message::Text(_)));
            released
                .recv_timeout(Duration::from_secs(2))
                .expect("client observed the temporary timeout");
            server
                .send(Message::Text(
                    json!({ "id": 3, "result": { "accepted": true } })
                        .to_string()
                        .into(),
                ))
                .expect("delayed acknowledgement");
        });
        send_json(&mut client, json!({ "id": 3, "method": "test-only" })).expect("request");
        let error = wait_for_plain_value_response_until(&mut client, 3, Duration::from_millis(20))
            .expect_err("temporary timeout");
        assert_eq!(error, rpc_timeout_message(Duration::from_millis(20)));
        release.send(()).expect("release the delayed response");
        let response = wait_for_plain_value_response_until(&mut client, 3, Duration::from_secs(1))
            .expect("read the acknowledgement without resending");
        assert_eq!(
            response.pointer("/result/accepted"),
            Some(&Value::Bool(true))
        );
        sender.join().expect("test server completed");
    }

    #[test]
    fn rpc_wait_deadline_is_not_reset_by_a_stream_of_unrelated_notifications() {
        let deadline = Instant::now() + Duration::from_millis(25);
        let notification = Message::Text(
            json!({ "method": "thread/status/changed" })
                .to_string()
                .into(),
        );
        let mut received = 0;
        loop {
            let message = read_message_until(deadline, "response timeout", |_| {
                thread::sleep(Duration::from_millis(10));
                Ok(notification.clone())
            });
            match message {
                Ok(message) => {
                    assert!(rpc_response_value(&message, 3)
                        .expect("notification")
                        .is_none());
                    received += 1;
                }
                Err(error) => {
                    assert_eq!(error, "response timeout");
                    break;
                }
            }
        }
        assert!(received > 0);
        assert!(received <= 3);
    }

    #[test]
    fn rpc_wait_preserves_fatal_socket_errors() {
        let error = read_message_until(
            Instant::now() + Duration::from_secs(1),
            "response timeout",
            |_| Err(tungstenite::Error::ConnectionClosed),
        )
        .expect_err("closed connection");
        assert_eq!(error, tungstenite::Error::ConnectionClosed.to_string());
    }

    #[test]
    fn rpc_acknowledgement_requires_a_response_not_a_permission_request_with_the_same_id() {
        let request = Message::Text(
            json!({
                "id": 3, "method": "item/commandExecution/requestApproval", "params": {}
            })
            .to_string()
            .into(),
        );
        assert!(rpc_response_value(&request, 3)
            .expect("server request")
            .is_none());
        let response = Message::Text(json!({ "id": 3, "result": null }).to_string().into());
        assert!(rpc_response_value(&response, 3)
            .expect("acknowledgement")
            .is_some());
        assert!(rpc_response_value(&response, 2)
            .expect("other request")
            .is_none());
        let refused = Message::Text(
            json!({
                "id": 3, "error": { "code": -32600, "message": "already has an active writer" }
            })
            .to_string()
            .into(),
        );
        assert_eq!(
            rpc_response_value(&refused, 3).expect_err("refusal"),
            "already has an active writer"
        );
    }

    #[cfg(unix)]
    #[test]
    fn failed_port_probe_does_not_kill_a_running_app_server() {
        let mut process = Command::new("sleep").arg("5").spawn().expect("test child");
        let result = probe_existing_server(&mut process, || false, Duration::from_millis(20))
            .expect("probe");
        assert!(matches!(result, ExistingServer::Unresponsive));
        assert!(process.try_wait().expect("child status").is_none());
        process.kill().expect("stop test child");
        process.wait().expect("reap test child");
    }

    #[test]
    fn identifies_only_the_lume_codex_server_command() {
        let managed = [
            std::ffi::OsString::from("codex"),
            std::ffi::OsString::from("app-server"),
            std::ffi::OsString::from("--listen"),
            std::ffi::OsString::from(server_url()),
        ];
        let unrelated = [
            std::ffi::OsString::from("codex"),
            std::ffi::OsString::from("resume"),
            std::ffi::OsString::from("thread-id"),
        ];

        assert!(is_lume_server_command(&managed));
        assert!(!is_lume_server_command(&unrelated));
    }

    #[test]
    fn proxy_requires_its_secret_and_rejects_browser_origins() {
        let authorized = tungstenite::handshake::server::Request::builder()
            .uri("/?token=secret")
            .body(())
            .expect("request");
        let missing = tungstenite::handshake::server::Request::builder()
            .uri("/")
            .body(())
            .expect("request");
        let browser = tungstenite::handshake::server::Request::builder()
            .uri("/?token=secret")
            .header("origin", "http://localhost")
            .body(())
            .expect("request");

        assert!(proxy_request_authorized(&authorized, "secret"));
        assert!(!proxy_request_authorized(&missing, "secret"));
        assert!(!proxy_request_authorized(&browser, "secret"));
    }

    #[test]
    fn cloned_bridge_does_not_own_the_shared_server_process() {
        let bridge = CodexBridge {
            process: Arc::new(Mutex::new(None)),
            queued_prompts: Arc::new(Mutex::new(HashMap::new())),
            collaboration_modes: Arc::new(Mutex::new(HashMap::new())),
            active_proxy_threads: Arc::new(Mutex::new(HashMap::new())),
            proxy_url: PROXY_BASE_URL.into(),
            owns_process: true,
        };

        let command_bridge = bridge.clone();

        assert!(bridge.owns_process);
        assert!(!command_bridge.owns_process);
    }

    #[test]
    fn queued_prompts_keep_their_order_until_the_active_turn_finishes() {
        let bridge = CodexBridge {
            process: Arc::new(Mutex::new(None)),
            queued_prompts: Arc::new(Mutex::new(HashMap::new())),
            collaboration_modes: Arc::new(Mutex::new(HashMap::new())),
            active_proxy_threads: Arc::new(Mutex::new(HashMap::new())),
            proxy_url: PROXY_BASE_URL.into(),
            owns_process: true,
        };
        bridge
            .queue_prompt(
                "session-1",
                "activity-1",
                "thread-1",
                "First",
                &[],
                direct_profile(),
            )
            .expect("first queued prompt");
        bridge
            .queue_prompt(
                "session-1",
                "activity-2",
                "thread-1",
                "Second",
                &[],
                direct_profile(),
            )
            .expect("second queued prompt");

        let queues = bridge.queued_prompts.lock().expect("prompt queues");
        let queue = queues.get("thread-1").expect("thread queue");
        assert_eq!(queue[0].prompt, "First");
        assert_eq!(queue[1].prompt, "Second");
    }

    #[test]
    fn queue_is_bounded_per_thread() {
        let bridge = CodexBridge {
            process: Arc::new(Mutex::new(None)),
            queued_prompts: Arc::new(Mutex::new(HashMap::new())),
            collaboration_modes: Arc::new(Mutex::new(HashMap::new())),
            active_proxy_threads: Arc::new(Mutex::new(HashMap::new())),
            proxy_url: PROXY_BASE_URL.into(),
            owns_process: true,
        };
        for index in 0..MAX_QUEUED_PROMPTS_PER_THREAD {
            bridge
                .queue_prompt(
                    "session-1",
                    &format!("activity-{index}"),
                    "thread-1",
                    "queued",
                    &[],
                    direct_profile(),
                )
                .expect("bounded prompt");
        }
        assert!(bridge
            .queue_prompt(
                "session-1",
                "overflow",
                "thread-1",
                "queued",
                &[],
                direct_profile(),
            )
            .expect_err("queue limit")
            .contains("too many"));
    }

    #[test]
    fn maps_command_decisions_to_codex_protocol() {
        let params = json!({});
        assert_eq!(
            decision_result(
                "item/commandExecution/requestApproval",
                PermissionAction::AllowSession,
                &params
            ),
            json!({ "decision": "acceptForSession" })
        );
        assert_eq!(
            decision_result(
                "item/fileChange/requestApproval",
                PermissionAction::Deny,
                &params
            ),
            json!({ "decision": "decline" })
        );
    }

    #[test]
    fn permission_grants_echo_requested_profile_without_extra_data() {
        let params = json!({ "permissions": { "network": { "enabled": true } } });
        assert_eq!(
            decision_result(
                "item/permissions/requestApproval",
                PermissionAction::AllowOnce,
                &params
            ),
            json!({
                "permissions": { "network": { "enabled": true } },
                "scope": "turn"
            })
        );
    }

    #[test]
    fn reads_per_thread_codex_access_configuration() {
        let profile = profile_from_params(
            &json!({
                "threadId": "thread",
                "sandboxPolicy": { "type": "readOnly", "networkAccess": false },
                "approvalPolicy": "on-request",
                "approvalsReviewer": "auto_review"
            }),
            direct_profile(),
        );
        assert_eq!(profile.mode, AccessMode::ReadOnly);
        assert_eq!(profile.label, "Somente leitura");
        assert_eq!(profile.approval_policy, "on-request");
        assert_eq!(profile.approvals_reviewer.as_deref(), Some("auto_review"));
    }

    #[test]
    fn automatic_profiles_do_not_pause_on_app_server_approval() {
        let mut profile = direct_profile();
        profile.approvals_reviewer = Some("auto_review".into());
        let params = json!({
            "threadId": "thread",
            "itemId": "command",
            "command": "npm test"
        });

        let response =
            automatic_approval_result(&profile, "item/commandExecution/requestApproval", &params)
                .expect("aprovação automática");
        assert_eq!(response["decision"], "accept");
    }

    #[test]
    fn interactive_user_input_is_distinct_from_approvals() {
        assert!(!is_approval("item/tool/requestUserInput"));
        let questions = codex_questions(&json!({
            "questions": [{
                "id": "approach",
                "header": "Approach",
                "question": "Which approach?",
                "isOther": true,
                "isSecret": false,
                "options": [
                    { "label": "A", "description": "First" },
                    { "label": "B", "description": "Second" }
                ]
            }]
        }));
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].id, "approach");
        assert_eq!(questions[0].options[1].label, "B");
    }

    #[test]
    fn prompt_uses_the_documented_turn_start_shape() {
        assert_eq!(
            prompt_turn_request("thread-1", "Continue os testes", &[]),
            json!({
                "method": "turn/start",
                "id": 3,
                "params": {
                    "threadId": "thread-1",
                    "input": [{ "type": "text", "text": "Continue os testes" }]
                }
            })
        );
    }

    #[test]
    fn model_catalog_keeps_server_advertised_efforts() {
        let settings = model_settings_from_responses(
            &json!({
                "result": {
                    "model": "gpt-test",
                    "reasoningEffort": "high",
                    "serviceTier": "fast"
                }
            }),
            &json!({
                "result": {
                    "data": [{
                        "model": "gpt-test",
                        "displayName": "GPT Test",
                        "description": "Test model",
                        "isDefault": true,
                        "defaultReasoningEffort": "medium",
                        "supportedReasoningEfforts": [
                            { "reasoningEffort": "medium", "description": "Balanced" },
                            { "reasoningEffort": "high", "description": "Deeper" }
                        ]
                    }]
                }
            }),
        )
        .expect("model settings");

        assert_eq!(settings.model, "gpt-test");
        assert_eq!(settings.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(settings.service_tier.as_deref(), Some("fast"));
        assert_eq!(settings.models[0].supported_reasoning_efforts.len(), 2);
        assert_eq!(
            settings.models[0].supported_reasoning_efforts[1].value,
            "high"
        );
    }

    #[test]
    fn model_catalog_can_supply_defaults_before_a_rollout_exists() {
        let settings = default_model_settings_from_catalog(&json!({
            "result": {
                "data": [
                    {
                        "model": "gpt-other",
                        "isDefault": false,
                        "defaultReasoningEffort": "low",
                        "supportedReasoningEfforts": [
                            { "reasoningEffort": "low", "description": "Quick" }
                        ]
                    },
                    {
                        "model": "gpt-default",
                        "isDefault": true,
                        "defaultReasoningEffort": "high",
                        "supportedReasoningEfforts": [
                            { "reasoningEffort": "high", "description": "Deep" }
                        ]
                    }
                ]
            }
        }))
        .expect("default settings");

        assert_eq!(settings.model, "gpt-default");
        assert_eq!(settings.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(settings.models.len(), 2);
    }

    #[test]
    fn model_settings_update_uses_thread_resume_configuration_overrides() {
        assert_eq!(
            thread_model_settings_update_request("thread-1", "gpt-test", "high"),
            json!({
                "method": "thread/resume",
                "id": 2,
                "params": {
                    "threadId": "thread-1",
                    "excludeTurns": true,
                    "model": "gpt-test",
                    "config": { "model_reasoning_effort": "high" }
                }
            })
        );
    }

    fn scripted_model_settings_update(response: Value) -> Result<CodexThreadModelSettings, String> {
        let listener = TcpListener::bind("127.0.0.1:0").expect("isolated test socket");
        let client =
            TcpStream::connect(listener.local_addr().expect("test address")).expect("test client");
        let (server, _) = listener.accept().expect("test server");
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("bounded test server");
        let sender = thread::spawn(move || {
            let mut server =
                WebSocket::from_raw_socket(server, tungstenite::protocol::Role::Server, None);
            let request = server.read().expect("settings update");
            let request: Value = serde_json::from_str(request.to_text().expect("JSON request"))
                .expect("settings request");
            assert_eq!(
                request,
                thread_model_settings_update_request("thread-1", "gpt-test", "high")
            );
            server
                .send(Message::Text(response.to_string().into()))
                .expect("scripted response");
            assert!(
                server.read().is_err(),
                "the catalog must not be fetched again"
            );
        });
        let mut client = WebSocket::from_raw_socket(
            MaybeTlsStream::Plain(client),
            tungstenite::protocol::Role::Client,
            None,
        );
        let models = default_model_settings_from_catalog(&json!({
            "result": { "data": [{
                "model": "gpt-test",
                "defaultReasoningEffort": "medium",
                "supportedReasoningEfforts": [
                    { "reasoningEffort": "medium" },
                    { "reasoningEffort": "high" }
                ]
            }] }
        }))
        .expect("catalog")
        .models;
        let result =
            apply_thread_model_settings(&mut client, "thread-1", "gpt-test", "high", &models);
        drop(client);
        sender.join().expect("test server completed");
        result
    }

    #[test]
    fn model_settings_update_reuses_catalog_and_keeps_acknowledged_overrides() {
        let settings = scripted_model_settings_update(json!({
            "id": 2,
            "result": {
                "model": "previous-turn-model",
                "reasoningEffort": "low",
                "serviceTier": "fast"
            }
        }))
        .expect("one acknowledged update");
        assert_eq!(settings.model, "gpt-test");
        assert_eq!(settings.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(settings.service_tier.as_deref(), Some("fast"));
        assert_eq!(settings.models.len(), 1);
        assert_eq!(settings.models[0].supported_reasoning_efforts.len(), 2);
    }

    #[test]
    fn model_settings_update_does_not_hide_provider_errors() {
        let error = scripted_model_settings_update(json!({
            "id": 2,
            "error": { "code": -32600, "message": "model unavailable" }
        }))
        .expect_err("provider rejected the update");
        assert!(error.contains("model unavailable"));
    }

    #[test]
    fn pending_model_settings_are_applied_when_starting_a_recovered_thread() {
        let (_, mut params) = prepare_thread_request_params("/work/lume", None, None, None);
        apply_model_override_to_thread_start(
            &mut params,
            &SessionModelOverride {
                model: Some("gpt-test".into()),
                reasoning_effort: Some("xhigh".into()),
            },
        );

        assert_eq!(params.get("model"), Some(&json!("gpt-test")));
        assert_eq!(
            params.pointer("/config/model_reasoning_effort"),
            Some(&json!("xhigh"))
        );
    }

    #[test]
    fn fast_mode_is_a_service_tier_not_a_reasoning_effort() {
        assert_eq!(
            thread_fast_mode_request("thread-1", true),
            json!({"method": "thread/resume", "id": 2, "params": {"threadId": "thread-1", "excludeTurns": true, "serviceTier": "fast"}})
        );
        assert_eq!(
            confirmed_fast_mode(&json!({"result": {"serviceTier": "fast"}}), true),
            Ok(true)
        );
        assert_eq!(
            confirmed_fast_mode(&json!({"result": {"serviceTier": null}}), false),
            Ok(false)
        );
        assert!(confirmed_fast_mode(&json!({"result": {"serviceTier": "default"}}), true).is_err());
    }

    #[test]
    fn selected_permission_mode_is_applied_to_each_turn_start() {
        let mut request = prompt_turn_request("thread-1", "Continue", &[]);
        assert!(apply_permission_override_to_turn_request(
            &mut request,
            Some("auto_review")
        ));
        let params = &request["params"];
        assert_eq!(params["approvalsReviewer"], json!("auto_review"));
        assert_eq!(params["approvalPolicy"], json!("on-request"));
        assert_eq!(params["sandboxPolicy"]["type"], json!("workspaceWrite"));
        // The prompt itself is left alone.
        assert_eq!(params["threadId"], json!("thread-1"));
        assert_eq!(params["input"][0]["text"], json!("Continue"));

        // Going back to full access replaces the whole scope, not just the reviewer.
        assert!(apply_permission_override_to_turn_request(
            &mut request,
            Some("full_access")
        ));
        let params = &request["params"];
        assert_eq!(params["approvalPolicy"], json!("never"));
        assert_eq!(params["approvalsReviewer"], json!("user"));
        assert_eq!(
            params["sandboxPolicy"],
            json!({ "type": "dangerFullAccess" })
        );

        // With no mode picked, or one Lume does not offer, the turn is untouched.
        let mut untouched = prompt_turn_request("thread-1", "Continue", &[]);
        assert!(!apply_permission_override_to_turn_request(
            &mut untouched,
            None
        ));
        assert!(!apply_permission_override_to_turn_request(
            &mut untouched,
            Some("read_only")
        ));
        assert_eq!(untouched, prompt_turn_request("thread-1", "Continue", &[]));
    }

    #[test]
    fn selected_model_and_effort_are_applied_to_each_turn_start() {
        let mut request = prompt_turn_request("thread-1", "Continue", &[]);
        let changed = apply_model_override_to_turn_request(
            &mut request,
            &SessionModelOverride {
                model: Some("gpt-test".into()),
                reasoning_effort: Some("xhigh".into()),
            },
        );

        assert!(changed);
        assert_eq!(request["params"]["model"], "gpt-test");
        assert_eq!(request["params"]["effort"], "xhigh");
        assert_eq!(request["params"]["input"][0]["text"], "Continue");
    }

    #[test]
    fn turn_without_saved_model_override_keeps_client_selection() {
        let mut request = json!({
            "method": "turn/start",
            "params": {"model": "client-model", "effort": "high"}
        });
        assert!(!apply_model_override_to_turn_request(
            &mut request,
            &SessionModelOverride::default(),
        ));
        assert_eq!(request["params"]["model"], "client-model");
        assert_eq!(request["params"]["effort"], "high");
    }

    #[test]
    fn rename_uses_the_documented_thread_name_shape() {
        assert_eq!(
            thread_name_request("thread-1", "Lume principal"),
            json!({
                "method": "thread/name/set",
                "id": 2,
                "params": {
                    "threadId": "thread-1",
                    "name": "Lume principal"
                }
            })
        );
    }

    #[test]
    fn active_proxy_prompt_keeps_its_private_request_id() {
        assert_eq!(
            prompt_turn_request_with_id(
                "thread-live",
                "Primeiro prompt",
                &[],
                json!("lume-prompt:7"),
            ),
            json!({
                "method": "turn/start",
                "id": "lume-prompt:7",
                "params": {
                    "threadId": "thread-live",
                    "input": [{ "type": "text", "text": "Primeiro prompt" }]
                }
            })
        );
    }

    #[test]
    fn proxy_tracks_live_thread_lifecycle() {
        let started = Message::Text(
            json!({
                "method": "thread/started",
                "params": { "thread": { "id": "thread-live" } }
            })
            .to_string()
            .into(),
        );
        let closed = Message::Text(
            json!({
                "method": "thread/closed",
                "params": { "threadId": "thread-live" }
            })
            .to_string()
            .into(),
        );
        assert_eq!(
            proxy_thread_lifecycle(&started),
            Some(("thread-live".into(), true))
        );
        assert_eq!(
            proxy_thread_lifecycle(&closed),
            Some(("thread-live".into(), false))
        );
    }

    #[test]
    fn proxy_tracks_a_resume_request_until_the_server_accepts_it() {
        let resumed = Message::Text(
            json!({
                "method": "thread/resume",
                "id": 2,
                "params": { "threadId": "thread-live" }
            })
            .to_string()
            .into(),
        );
        let accepted = Message::Text(
            json!({ "id": 2, "result": { "thread": { "id": "thread-live" } } })
                .to_string()
                .into(),
        );
        let rejected = Message::Text(
            json!({ "id": 2, "error": { "message": "active writer" } })
                .to_string()
                .into(),
        );
        let unrelated = Message::Text(
            json!({ "method": "turn/start", "params": { "threadId": "thread-live" } })
                .to_string()
                .into(),
        );

        assert_eq!(
            proxy_client_resume_request(&resumed),
            Some(("2".into(), "thread-live".into()))
        );
        assert_eq!(proxy_response_outcome(&accepted), Some(("2".into(), true)));
        assert_eq!(proxy_response_outcome(&rejected), Some(("2".into(), false)));
        assert_eq!(proxy_client_resume_request(&unrelated), None);
    }

    #[test]
    fn proxy_thread_is_ready_only_after_resume_acceptance() {
        let threads = Arc::new(Mutex::new(HashMap::new()));
        let (sender, _receiver) = mpsc::channel();
        threads.lock().expect("proxy threads").insert(
            "thread-live".into(),
            ActiveProxyConnection {
                connection_id: 1,
                sender,
                ready: false,
            },
        );

        assert!(!proxy_thread_ready(&threads, "thread-live").expect("pending resume"));
        threads
            .lock()
            .expect("proxy threads")
            .get_mut("thread-live")
            .expect("thread")
            .ready = true;
        assert!(proxy_thread_ready(&threads, "thread-live").expect("accepted resume"));
    }

    #[test]
    fn proxy_consumes_only_lume_prompt_responses() {
        let accepted = Message::Text(
            json!({ "id": "lume-prompt:8", "result": { "turn": { "id": "turn-1" } } })
                .to_string()
                .into(),
        );
        let rejected = Message::Text(
            json!({ "id": "lume-prompt:9", "error": { "message": "turn unavailable" } })
                .to_string()
                .into(),
        );
        let unrelated = Message::Text(json!({ "id": 3, "result": {} }).to_string().into());

        assert_eq!(
            proxy_prompt_response(&accepted),
            Some(("lume-prompt:8".into(), Ok(())))
        );
        assert_eq!(
            proxy_prompt_response(&rejected),
            Some(("lume-prompt:9".into(), Err("turn unavailable".into())))
        );
        assert_eq!(proxy_prompt_response(&unrelated), None);
    }

    #[test]
    fn prompt_can_include_local_images() {
        assert_eq!(
            prompt_turn_request(
                "thread-1",
                "Analise esta tela",
                &["/tmp/screenshot.png".into()],
            )["params"]["input"],
            json!([
                { "type": "text", "text": "Analise esta tela" },
                { "type": "localImage", "path": "/tmp/screenshot.png" }
            ])
        );
    }

    #[test]
    fn collaboration_mode_uses_the_server_preset_and_current_model_fallback() {
        let resumed = json!({
            "result": {
                "model": "gpt-test-default",
                "effort": "medium"
            }
        });
        let presets = json!({
            "result": {
                "data": [
                    {
                        "name": "Default",
                        "mode": "default",
                        "model": null,
                        "reasoning_effort": null
                    },
                    {
                        "name": "Plan",
                        "mode": "plan",
                        "model": "gpt-test-plan",
                        "reasoning_effort": "high"
                    }
                ]
            }
        });

        assert_eq!(
            collaboration_mode_from_responses("plan", &resumed, &presets).expect("plan preset"),
            json!({
                "mode": "plan",
                "settings": {
                    "model": "gpt-test-plan",
                    "developer_instructions": null,
                    "reasoning_effort": "high"
                }
            })
        );
        assert_eq!(
            collaboration_mode_from_responses("default", &resumed, &presets)
                .expect("default preset"),
            json!({
                "mode": "default",
                "settings": {
                    "model": "gpt-test-default",
                    "developer_instructions": null,
                    "reasoning_effort": "medium"
                }
            })
        );
    }

    #[test]
    fn codex_rate_limits_expose_remaining_windows() {
        let limits = rate_limits_from_message(&json!({
            "result": {
                "rateLimits": {
                    "limitId": "codex",
                    "primary": {
                        "usedPercent": 32,
                        "windowDurationMins": 300,
                        "resetsAt": 1_800_000_000
                    },
                    "secondary": {
                        "usedPercent": 78,
                        "windowDurationMins": 10_080
                    }
                }
            }
        }));
        assert_eq!(limits.len(), 2);
        assert_eq!(limits[0].used_percent, 32);
        assert_eq!(limits[0].resets_at, Some(1_800_000_000_000));
        assert_eq!(limits[1].label, "7d");
    }

    #[test]
    fn token_usage_notification_exposes_the_latest_turn_breakdown() {
        let notification = json!({
            "method": "thread/tokenUsage/updated",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-2",
                "tokenUsage": {
                    "last": {
                        "totalTokens": 4_200,
                        "inputTokens": 3_000,
                        "outputTokens": 1_200
                    }
                }
            }
        });
        let usage = turn_token_usage_from_message(&notification).expect("token usage");

        assert_eq!(usage, ("thread-1", "turn-2", 4_200, 3_000, 1_200));
    }

    #[test]
    fn canonical_codex_rate_limit_wins_over_unrelated_metered_buckets() {
        let limits = rate_limits_from_message(&json!({
            "result": {
                "rateLimits": {
                    "limitId": "codex",
                    "primary": { "usedPercent": 24, "windowDurationMins": 300 }
                },
                "rateLimitsByLimitId": {
                    "codex": {
                        "primary": { "usedPercent": 24, "windowDurationMins": 300 }
                    },
                    "codex_other": {
                        "primary": { "usedPercent": 91, "windowDurationMins": 60 }
                    }
                }
            }
        }));

        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].id, "codex:primary");
        assert_eq!(limits[0].used_percent, 24);
    }

    #[test]
    fn disconnected_prompt_monitor_releases_the_running_state_without_closing_the_chat() {
        let event = prompt_monitor_settled_event("thread-1");
        assert!(matches!(event.event, HookEventKind::WaitingForInput));
        assert_eq!(event.native_session_id.as_deref(), Some("thread-1"));
        assert_eq!(event.status_label.as_deref(), Some("Esperando ação"));
    }

    #[test]
    fn late_prompt_disconnect_does_not_replace_a_completed_thread() {
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        let mut started = prompt_monitor_settled_event("thread-1");
        started.event = HookEventKind::SessionStarted;
        started.status_label = Some("Session started".into());
        state.ingest(started).expect("started session");
        assert!(!prompt_thread_already_finished("thread-1", &state));

        let mut completed = prompt_monitor_settled_event("thread-1");
        completed.event = HookEventKind::Completed;
        completed.status_label = Some("Task completed".into());
        completed.last_response = Some("Done".into());
        state.ingest(completed).expect("completed session");

        assert!(prompt_thread_already_finished("thread-1", &state));
    }

    #[test]
    fn unloaded_thread_remains_available_for_resume() {
        let event = notification_event(
            &json!({
                "method": "thread/closed",
                "params": { "threadId": "thread-1" }
            }),
            "thread/closed",
            &HashMap::new(),
            &mut HashMap::new(),
        )
        .expect("thread event");

        assert!(matches!(event.event, HookEventKind::WaitingForInput));
        assert_eq!(event.status_label.as_deref(), Some("Esperando ação"));
    }

    #[test]
    fn completed_turn_carries_the_last_agent_message() {
        let mut responses = HashMap::new();
        remember_response(
            &json!({
                "method": "item/completed",
                "params": {
                    "threadId": "thread-1",
                    "item": { "type": "agentMessage", "text": "Resposta final" }
                }
            }),
            "item/completed",
            &mut responses,
        );
        let event = notification_event(
            &json!({
                "method": "turn/completed",
                "params": { "threadId": "thread-1", "turn": { "status": "completed" } }
            }),
            "turn/completed",
            &HashMap::new(),
            &mut responses,
        )
        .expect("evento concluído");

        assert_eq!(event.last_response.as_deref(), Some("Resposta final"));
        assert!(responses.is_empty());
    }

    #[test]
    fn started_thread_keeps_its_name_separate_from_the_project() {
        let event = notification_event(
            &json!({
                "method": "thread/started",
                "params": {
                    "thread": {
                        "id": "thread-1",
                        "name": "Review authentication",
                        "cwd": "/work/lume"
                    }
                }
            }),
            "thread/started",
            &HashMap::new(),
            &mut HashMap::new(),
        )
        .expect("evento da thread");

        assert_eq!(event.session_name.as_deref(), Some("Review authentication"));
        assert_eq!(event.project.as_deref(), Some("lume"));
    }

    #[test]
    fn thread_name_updates_are_forwarded_to_the_session() {
        let event = notification_event(
            &json!({
                "method": "thread/name/updated",
                "params": {
                    "threadId": "thread-1",
                    "threadName": "Lume principal"
                }
            }),
            "thread/name/updated",
            &HashMap::new(),
            &mut HashMap::new(),
        )
        .expect("evento de nome");

        assert!(matches!(event.event, HookEventKind::Activity));
        assert_eq!(event.session_name.as_deref(), Some("Lume principal"));
    }

    #[test]
    fn interrupted_turn_returns_to_waiting_without_a_completion_result() {
        let event = notification_event(
            &json!({
                "method": "turn/completed",
                "params": { "threadId": "thread-1", "turn": { "status": "interrupted" } }
            }),
            "turn/completed",
            &HashMap::new(),
            &mut HashMap::new(),
        )
        .expect("interrupted event");

        assert!(matches!(event.event, HookEventKind::WaitingForInput));
        assert_eq!(event.status_label.as_deref(), Some("Prompt interrompido"));
        assert!(event.last_response.is_none());
    }

    #[test]
    fn codex_items_expose_commands_and_changed_files() {
        let command = codex_item_activity(
            "thread-1",
            &json!({
                "id": "command-1",
                "type": "commandExecution",
                "command": ["npm", "test"],
                "status": "completed",
                "aggregatedOutput": "12 tests passed"
            }),
            true,
            None,
        )
        .expect("atividade de comando");
        assert_eq!(command.kind, "test");
        assert_eq!(command.title, "npm test");
        assert_eq!(command.status, "completed");
        assert_eq!(command.detail.as_deref(), Some("12 tests passed"));

        let files = codex_item_activity(
            "thread-1",
            &json!({
                "id": "file-1",
                "type": "fileChange",
                "changes": [
                    { "path": "src/lib/domain.ts" },
                    { "path": "src/lib/TerminalWindow.svelte" }
                ]
            }),
            true,
            None,
        )
        .expect("atividade de arquivo");
        assert_eq!(files.kind, "file");
        assert_eq!(files.files.len(), 2);

        let mcp_failure = codex_item_activity(
            "thread-1",
            &json!({
                "id": "mcp-1",
                "type": "mcpToolCall",
                "server": "github",
                "tool": "create_issue",
                "status": "failed",
                "error": "connection refused"
            }),
            true,
            None,
        )
        .expect("atividade MCP");
        assert_eq!(mcp_failure.title, "MCP · github · create_issue");
        assert_eq!(mcp_failure.status, "failed");

        assert_eq!(
            files_from_diff(
                "*** Begin Patch\n*** Update File: src/lib/TerminalWindow.svelte\n@@\n-old\n+new\n*** End Patch"
            ),
            vec!["src/lib/TerminalWindow.svelte"]
        );
        assert_eq!(
            files_from_diff(
                "*** Begin Patch *** Update File: /work/lume/src-tauri/src/control.rs @@ old + new *** End Patch"
            ),
            vec!["/work/lume/src-tauri/src/control.rs"]
        );
    }

    #[test]
    fn app_server_subagent_lifecycle_keeps_the_child_identity() {
        let started = codex_item_activity(
            "parent-1",
            &json!({
                "type": "subAgentActivity",
                "id": "event-1",
                "agentThreadId": "child-1",
                "agentPath": "/root/prompt_index_backend_test",
                "kind": "started"
            }),
            true,
            Some("turn-1"),
        )
        .expect("subagent started");
        let completed = codex_item_activity(
            "parent-1",
            &json!({
                "type": "subAgentActivity",
                "id": "event-2",
                "agentThreadId": "child-1",
                "agentPath": "/root/prompt_index_backend_test",
                "kind": "completed"
            }),
            true,
            Some("turn-1"),
        )
        .expect("subagent completed");
        assert_eq!(started.id, "codex:parent-1:subagent:child-1");
        assert_eq!(started.kind, "subagent");
        assert_eq!(started.status, "running");
        assert_eq!(completed.id, started.id);
        assert_eq!(completed.status, "completed");
    }

    #[test]
    fn codex_stream_deltas_update_the_existing_activity() {
        let event = activity_event(
            &json!({
                "method": "item/commandExecution/outputDelta",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "itemId": "command-1",
                    "delta": "compiling…"
                }
            }),
            "item/commandExecution/outputDelta",
            None,
        )
        .expect("delta de comando");
        let activity = event.activity.expect("atividade");
        assert_eq!(activity.id, "codex:thread-1:turn:turn-1:item:command-1");
        assert!(activity.append_detail);
        assert_eq!(activity.detail.as_deref(), Some("compiling…"));

        let diff = activity_event(
            &json!({
                "method": "turn/diff/updated",
                "params": {
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "diff": "--- a/src/old.rs\n+++ b/src/new.rs\n@@ -1 +1 @@"
                }
            }),
            "turn/diff/updated",
            None,
        )
        .expect("diff da tarefa")
        .activity
        .expect("atividade de diff");
        assert_eq!(diff.files, vec!["src/old.rs", "src/new.rs"]);
    }

    #[test]
    fn plan_updates_without_thread_id_use_the_active_turn_thread() {
        let event = activity_event(
            &json!({
                "method": "turn/plan/updated",
                "params": {
                    "turnId": "turn-1",
                    "explanation": "Starting",
                    "plan": [{ "step": "Inspect", "status": "in_progress" }]
                }
            }),
            "turn/plan/updated",
            Some("thread-1"),
        )
        .expect("plan activity");
        let activity = event.activity.expect("plan detail");
        assert_eq!(event.native_session_id.as_deref(), Some("thread-1"));
        assert_eq!(activity.kind, "plan");
        assert!(activity
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("Inspect")));
    }

    #[test]
    fn runtime_warnings_become_session_alert_activities() {
        let event = activity_event(
            &json!({
                "method": "warning",
                "params": { "message": "MCP server is unavailable" }
            }),
            "warning",
            Some("thread-1"),
        )
        .expect("warning activity");
        let activity = event.activity.expect("warning detail");
        assert_eq!(activity.kind, "warning");
        assert_eq!(activity.status, "warning");
        assert_eq!(
            activity.detail.as_deref(),
            Some("MCP server is unavailable")
        );
    }

    #[test]
    fn app_server_exec_exposes_nested_todo_and_plan_tools() {
        let todo = codex_item_activity(
            "thread-1",
            &json!({
                "id": "tool-1",
                "type": "dynamicToolCall",
                "tool": "exec",
                "arguments": {
                    "source": "const value = await tools.todo_write({todos:[{content:\"Inspect\",status:\"in_progress\"}]}); text(value);"
                }
            }),
            true,
            Some("turn-1"),
        )
        .expect("todo activity");
        assert_eq!(todo.kind, "tool");
        assert_eq!(todo.title, "todo_write");
        assert!(todo
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("Inspect")));

        let plan = codex_item_activity(
            "thread-1",
            &json!({
                "id": "tool-2",
                "type": "dynamicToolCall",
                "tool": "exec",
                "arguments": "await tools.update_plan({plan:[{step:\"Fix\",status:\"pending\"}]});"
            }),
            true,
            Some("turn-1"),
        )
        .expect("plan activity");
        assert_eq!(plan.kind, "plan");
        assert_eq!(plan.title, "Plano atualizado");
        assert!(plan
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("Fix")));
    }

    #[test]
    fn new_threads_are_created_with_the_selected_project_profile() {
        let (method, params) = prepare_thread_request_params(
            "/work/lume",
            None,
            Some(&AccessMode::WorkspaceWrite),
            Some("on-request"),
        );

        assert_eq!(method, "thread/start");
        assert_eq!(text_at(&params, "cwd"), Some("/work/lume"));
        assert_eq!(text_at(&params, "serviceName"), Some("lume"));
        assert_eq!(text_at(&params, "sandbox"), Some("workspace-write"));
        assert_eq!(text_at(&params, "approvalPolicy"), Some("on-request"));
        assert!(params.get("threadId").is_none());
    }

    #[test]
    fn legacy_thread_sandbox_uses_protocol_enum_values() {
        for (mode, expected) in [
            (AccessMode::ReadOnly, "read-only"),
            (AccessMode::Plan, "read-only"),
            (AccessMode::WorkspaceWrite, "workspace-write"),
            (AccessMode::FullAccess, "danger-full-access"),
        ] {
            let (_, params) =
                prepare_thread_request_params("/work/lume", Some("thread-1"), Some(&mode), None);
            assert_eq!(text_at(&params, "sandbox"), Some(expected));
        }
    }

    #[test]
    fn resumed_threads_keep_the_known_id_before_the_first_prompt() {
        let (method, params) =
            prepare_thread_request_params("/work/lume", Some("thread-1"), None, None);

        assert_eq!(method, "thread/resume");
        assert_eq!(text_at(&params, "threadId"), Some("thread-1"));
        assert_eq!(params.get("excludeTurns"), Some(&json!(true)));
        assert_eq!(text_at(&params, "cwd"), Some("/work/lume"));
        assert!(params.get("serviceName").is_none());
    }

    #[test]
    fn fork_turn_matching_uses_the_prompt_and_final_response() {
        let turn = json!({
            "id": "turn-2",
            "items": [
                { "type": "userMessage", "content": [{ "type": "text", "text": "Review the current change" }] },
                { "type": "agentMessage", "text": "The change is ready. All checks passed." }
            ]
        });

        assert!(turn_matches_response(
            &turn,
            "Review the current change",
            "The change is ready. All checks passed.",
            true,
        ));
        assert!(!turn_matches_response(
            &turn,
            "Review another change",
            "The change is ready. All checks passed.",
            true,
        ));
    }

    #[test]
    fn fork_turn_matching_accepts_cleaned_lume_prompt_text() {
        let turn = json!({
            "id": "turn-3",
            "items": [{
                "type": "userMessage",
                "content": [{ "type": "text", "text": "Inspect this image\n\nFiles attached through Lume: /tmp/image.png" }]
            }]
        });

        assert!(turn_matches_response(
            &turn,
            "Inspect this image",
            "",
            false,
        ));
    }
}
