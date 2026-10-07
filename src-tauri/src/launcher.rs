use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
use crate::state::now_millis;
use crate::{
    domain::{
        AccessMode, AgentKind, HookEvent, HookEventKind, SessionActivity, SessionControlOrigin,
    },
    event_server,
    integrations::IntegrationKind,
    state::AppState,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRequest {
    pub agent: IntegrationKind,
    pub working_directory: String,
    pub resume: bool,
    pub resume_id: Option<String>,
    pub target: String,
    #[serde(default)]
    pub initial_prompt: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<AccessMode>,
    #[serde(default)]
    pub approval_policy: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TerminalPayload {
    command: String,
    arguments: Vec<String>,
    working_directory: String,
    #[serde(default)]
    retry_quick_resume: bool,
}

const QUICK_RESUME_MAX_ATTEMPTS: usize = 4;
const QUICK_RESUME_EXIT_WINDOW: Duration = Duration::from_secs(8);
static LAUNCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn launch(
    request: LaunchRequest,
    executable: &Path,
    app_data_dir: &Path,
    codex_remote: Option<&str>,
) -> Result<(), String> {
    if request.agent == IntegrationKind::OpenCode {
        return Err("OpenCode is launched through its ACP bridge, not a terminal".into());
    }
    let mut payload = payload_for(&request, codex_remote);
    if request.agent == IntegrationKind::Claude {
        add_scoped_claude_hooks(&mut payload, executable)?;
    }
    if request.target == "vscode" {
        if crate::integrations::vscode_status().configured {
            return launch_vscode(&payload, &request.agent);
        }
        return Err("Conecte o Lume Companion ao VS Code nos Ajustes".into());
    }
    if request.agent == IntegrationKind::Claude
        && request
            .initial_prompt
            .as_deref()
            .is_some_and(|prompt| !prompt.trim().is_empty())
    {
        return Err("Um prompt do Claude precisa da execução observável do Lume".into());
    }
    launch_terminal(payload, executable, app_data_dir)
}

pub fn run_terminal_payload(path: &str) -> i32 {
    let path = PathBuf::from(path);
    let payload = match fs::read_to_string(&path)
        .map_err(|error| error.to_string())
        .and_then(|value| {
            serde_json::from_str::<TerminalPayload>(&value).map_err(|error| error.to_string())
        }) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("Não foi possível abrir a sessão do Lume: {error}");
            return 1;
        }
    };
    let _ = fs::remove_file(path);
    for attempt in 0..QUICK_RESUME_MAX_ATTEMPTS {
        let mut command = match crate::executables::command(&payload.command) {
            Ok(command) => command,
            Err(error) => {
                eprintln!("{error}");
                return 1;
            }
        };
        let started_at = Instant::now();
        match command
            .args(&payload.arguments)
            .env("LUME_MANAGED_SESSION", "1")
            .current_dir(&payload.working_directory)
            .status()
        {
            Ok(status)
                if should_retry_quick_resume(
                    payload.retry_quick_resume,
                    attempt,
                    status.code(),
                    started_at.elapsed(),
                ) =>
            {
                eprintln!("Lume: the resumed session ended during startup. Retrying…");
                thread::sleep(quick_resume_retry_delay(attempt));
            }
            Ok(status) => return status.code().unwrap_or(1),
            Err(error) => {
                eprintln!("Não foi possível iniciar {}: {error}", payload.command);
                return 1;
            }
        }
    }
    1
}

fn should_retry_quick_resume(
    enabled: bool,
    attempt: usize,
    exit_code: Option<i32>,
    elapsed: Duration,
) -> bool {
    enabled
        && attempt + 1 < QUICK_RESUME_MAX_ATTEMPTS
        && exit_code == Some(1)
        && elapsed <= QUICK_RESUME_EXIT_WINDOW
}

fn quick_resume_retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(350 * (attempt as u64 + 1))
}

fn payload_for(request: &LaunchRequest, codex_remote: Option<&str>) -> TerminalPayload {
    let (command, mut arguments) = match request.agent {
        IntegrationKind::Codex => {
            let mut arguments = Vec::new();
            if let Some(remote) = codex_remote {
                arguments.extend(["--remote".into(), remote.into()]);
            }
            ("codex".to_string(), arguments)
        }
        IntegrationKind::Claude => ("claude".to_string(), Vec::new()),
        IntegrationKind::Antigravity => ("agy".to_string(), Vec::new()),
        IntegrationKind::DeepSeek => ("dsh".to_string(), vec!["--profile".into(), "tui".into()]),
        IntegrationKind::Gemini => ("gemini".to_string(), Vec::new()),
        IntegrationKind::OpenCode => ("opencode".to_string(), Vec::new()),
    };
    if request.agent == IntegrationKind::Claude
        && request
            .initial_prompt
            .as_deref()
            .is_some_and(|prompt| !prompt.trim().is_empty())
    {
        arguments.push("--print".into());
    }
    apply_permission_profile(request, &mut arguments);
    if request.agent == IntegrationKind::Claude {
        if let Some(model) = request
            .model
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            arguments.extend(["--model".into(), model.into()]);
        }
        if let Some(effort) = request
            .reasoning_effort
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            arguments.extend(["--effort".into(), effort.into()]);
        }
    }
    if request.resume {
        match request.agent {
            IntegrationKind::Codex => {
                arguments.push("resume".into());
                if let Some(id) = &request.resume_id {
                    arguments.push(id.clone());
                }
            }
            IntegrationKind::Claude | IntegrationKind::Gemini => {
                arguments.push("--resume".into());
                if let Some(id) = &request.resume_id {
                    arguments.push(id.clone());
                }
            }
            IntegrationKind::Antigravity => {
                if let Some(id) = &request.resume_id {
                    arguments.extend(["--conversation".into(), id.clone()]);
                } else {
                    arguments.push("--continue".into());
                }
            }
            IntegrationKind::DeepSeek => {}
            IntegrationKind::OpenCode => {}
        }
    }
    if let Some(prompt) = request
        .initial_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        if request.agent == IntegrationKind::Antigravity {
            arguments.extend(["-i".into(), prompt.to_string()]);
        } else {
            arguments.push(prompt.to_string());
        }
    }
    TerminalPayload {
        command,
        arguments,
        working_directory: request.working_directory.clone(),
        retry_quick_resume: request.resume
            && matches!(
                request.agent,
                IntegrationKind::Codex | IntegrationKind::Claude
            ),
    }
}

/// Messages sent while a Claude prompt runs. Each one is its own `claude --print`,
/// so the queue lives here and the next one starts when the running one ends.
struct QueuedClaudePrompt {
    activity_id: String,
    prompt: String,
}

#[derive(Default)]
struct ClaudeQueue {
    prompts: std::collections::VecDeque<QueuedClaudePrompt>,
    /// After a cancel the queue waits for you instead of running on by itself.
    paused: bool,
}

/// Why a running prompt was stopped from Lume.
#[derive(Clone, Copy, PartialEq)]
pub enum ClaudeInterrupt {
    Cancel,
    /// Stopped so the next queued message can take its place.
    Steer,
}

fn claude_queues() -> &'static Mutex<HashMap<String, ClaudeQueue>> {
    static QUEUES: OnceLock<Mutex<HashMap<String, ClaudeQueue>>> = OnceLock::new();
    QUEUES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn claude_interrupts() -> &'static Mutex<HashMap<String, ClaudeInterrupt>> {
    static INTERRUPTS: OnceLock<Mutex<HashMap<String, ClaudeInterrupt>>> = OnceLock::new();
    INTERRUPTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn queue_claude_prompt(session_id: &str, activity_id: String, prompt: String) {
    if let Ok(mut queues) = claude_queues().lock() {
        queues
            .entry(session_id.to_string())
            .or_default()
            .prompts
            .push_back(QueuedClaudePrompt {
                activity_id,
                prompt,
            });
    }
}

pub fn claude_queue_len(session_id: &str) -> usize {
    claude_queues()
        .lock()
        .ok()
        .and_then(|queues| queues.get(session_id).map(|queue| queue.prompts.len()))
        .unwrap_or(0)
}

/// Records that the next exit of this session's prompt is Lume stopping it.
pub fn note_claude_interrupt(session_id: &str, kind: ClaudeInterrupt) {
    if let Ok(mut interrupts) = claude_interrupts().lock() {
        interrupts.insert(session_id.to_string(), kind);
    }
}

pub fn forget_claude_interrupt(session_id: &str) {
    if let Ok(mut interrupts) = claude_interrupts().lock() {
        interrupts.remove(session_id);
    }
}

fn take_claude_interrupt(session_id: &str) -> Option<ClaudeInterrupt> {
    claude_interrupts()
        .lock()
        .ok()
        .and_then(|mut interrupts| interrupts.remove(session_id))
}

fn set_claude_queue_paused(session_id: &str, paused: bool) {
    if let Ok(mut queues) = claude_queues().lock() {
        if let Some(queue) = queues.get_mut(session_id) {
            queue.paused = paused;
        }
    }
}

/// Starts the next queued message. `force` is for when you ask for it: it runs even
/// when a cancel paused the queue.
pub fn run_next_queued_claude_prompt(
    app: &AppHandle,
    state: &AppState,
    session_id: &str,
    force: bool,
) -> Result<bool, String> {
    let next = {
        let mut queues = claude_queues()
            .lock()
            .map_err(|_| "Could not read the Claude queue".to_string())?;
        let Some(queue) = queues.get_mut(session_id) else {
            return Ok(false);
        };
        if queue.paused && !force {
            return Ok(false);
        }
        queue.paused = false;
        queue.prompts.pop_front()
    };
    let Some(next) = next else {
        return Ok(false);
    };
    let started = (|| {
        let session = state.connected_session(session_id)?;
        let (request, permission_mode) =
            crate::control::claude_launch_request(state, &session, &next.prompt)?;
        state.promote_queued_prompt_activity(session_id, &next.activity_id)?;
        state.mark_prompt_started(session_id)?;
        launch_claude_prompt(
            request,
            app.clone(),
            state.clone(),
            session_id.to_string(),
            permission_mode,
        )
    })();
    if let Err(error) = started {
        let _ = state.mark_queued_prompt_needs_attention(session_id, &next.activity_id);
        // The session may already read as running; report the failure so it does not stay so.
        if let Some(native_id) = state
            .connected_session(session_id)
            .ok()
            .and_then(|session| session.native_session_id)
        {
            let mut event = claude_prompt_event(session_id, &native_id);
            event.event = HookEventKind::Failed;
            event.status_label = Some(error.chars().take(240).collect());
            event.activity = Some(claude_error_activity(&native_id, &error));
            let _ = event_server::publish_event(state, app, event);
        }
        crate::protocol::emit_sessions_changed(app);
        return Err(error);
    }
    crate::protocol::emit_sessions_changed(app);
    Ok(true)
}

pub fn launch_claude_prompt(
    request: LaunchRequest,
    app: AppHandle,
    state: AppState,
    session_id: String,
    permission_mode: Option<String>,
) -> Result<(), String> {
    let native_id = request
        .resume_id
        .clone()
        .ok_or("A sessão Claude não informou o ID para retomada")?;
    if crate::integrations::claude_session_open_interactively(&native_id) {
        return Err("Esta conversa do Claude está aberta no terminal. Envie a mensagem por lá ou feche o terminal para continuar pelo Lume.".into());
    }
    // A stop that raced the previous run's exit must not be taken for this run's.
    forget_claude_interrupt(&session_id);
    let mut payload = payload_for(&request, None);
    if let Some(mode) = permission_mode.as_deref() {
        apply_claude_permission_mode(&mut payload.arguments, mode);
    }
    if !crate::integrations::claude_transcript_can_resume(&native_id) {
        start_claude_session_with_id(&mut payload.arguments, &native_id);
    }
    add_scoped_claude_hooks(&mut payload, &crate::integrations::lume_executable()?)?;
    let mut command = crate::executables::command(&payload.command)?;
    command
        .args(&payload.arguments)
        .env("LUME_MANAGED_SESSION", "1")
        .env("LUME_CLAUDE_PROMPT_CAPTURE", "1")
        .current_dir(&payload.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let child = command
        .spawn()
        .map_err(|error| format!("Não foi possível iniciar o Claude: {error}"))?;
    thread::Builder::new()
        .name("lume-claude-prompt".into())
        .spawn(move || {
            let output = child.wait_with_output();
            // Lume stopped this prompt: it is already marked as interrupted, so the
            // process dying is not a failure. A cancel pauses the queue; a steer moves on.
            if let Some(interrupt) = take_claude_interrupt(&session_id) {
                match interrupt {
                    ClaudeInterrupt::Cancel => set_claude_queue_paused(&session_id, true),
                    ClaudeInterrupt::Steer => {
                        let _ = run_next_queued_claude_prompt(&app, &state, &session_id, true);
                    }
                }
                return;
            }
            let mut event = claude_prompt_event(&session_id, &native_id);
            match output {
                Ok(output) if output.status.success() => {
                    let response = String::from_utf8_lossy(&output.stdout)
                        .trim()
                        .chars()
                        .take(32 * 1024)
                        .collect::<String>();
                    if response.is_empty() {
                        let recorded_response = state
                            .connected_session(&session_id)
                            .ok()
                            .and_then(|session| current_claude_reply(&session.activities, None));
                        if let Some(recorded_response) = recorded_response {
                            event.event = HookEventKind::Completed;
                            event.last_response = Some(recorded_response);
                        } else {
                            event.event = HookEventKind::Failed;
                            let detail = "Claude terminou sem enviar uma resposta";
                            event.status_label = Some(detail.into());
                            event.activity = Some(claude_error_activity(&native_id, detail));
                        }
                    } else {
                        event.event = HookEventKind::Completed;
                        event.last_response = Some(response.clone());
                        let already_recorded = state
                            .connected_session(&session_id)
                            .ok()
                            .and_then(|session| {
                                current_claude_reply(&session.activities, Some(&response))
                            })
                            .is_some();
                        if !already_recorded {
                            event.activity = Some(SessionActivity {
                                id: format!(
                                    "claude:{native_id}:managed-response:{}",
                                    crate::state::now_millis()
                                ),
                                kind: "message".into(),
                                title: "Resposta do agente".into(),
                                detail: Some(response),
                                status: "completed".into(),
                                created_at: crate::state::now_millis(),
                                files: Vec::new(),
                                attachments: Vec::new(),
                                append_detail: false,
                            });
                        }
                    }
                }
                Ok(output) => {
                    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    event.event = HookEventKind::Failed;
                    let detail = if detail.is_empty() {
                        format!("Claude encerrou com código {}", output.status)
                    } else {
                        detail
                    };
                    event.status_label = Some(detail.chars().take(240).collect());
                    event.activity = Some(claude_error_activity(&native_id, &detail));
                }
                Err(error) => {
                    event.event = HookEventKind::Failed;
                    let detail = format!("Falha ao aguardar o Claude: {error}");
                    event.status_label = Some(detail.clone());
                    event.activity = Some(claude_error_activity(&native_id, &detail));
                }
            }
            let failed = matches!(event.event, HookEventKind::Failed);
            let _ = event_server::publish_event(&state, &app, event);
            // The next queued message follows a prompt that finished; after a failure
            // the queue waits, so one error does not send everything behind it.
            if failed {
                set_claude_queue_paused(&session_id, true);
            } else {
                let _ = run_next_queued_claude_prompt(&app, &state, &session_id, false);
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

// O Claude só grava o transcript após a primeira mensagem, então uma sessão aberta
// sem prompt ainda não pode ser retomada; criamos a conversa com o mesmo ID.
fn start_claude_session_with_id(arguments: &mut Vec<String>, native_id: &str) {
    if let Some(index) = arguments
        .windows(2)
        .position(|pair| pair[0] == "--resume" && pair[1] == native_id)
    {
        arguments[index] = "--session-id".into();
    } else if !arguments.iter().any(|argument| argument == "--session-id") {
        let insert_at = if arguments
            .first()
            .is_some_and(|argument| argument == "--print")
        {
            1
        } else {
            0
        };
        arguments.splice(
            insert_at..insert_at,
            ["--session-id".into(), native_id.into()],
        );
    }
}

/// Runs the prompt in the chosen mode, replacing whatever the profile put there.
fn apply_claude_permission_mode(arguments: &mut Vec<String>, mode: &str) {
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--permission-mode" => {
                arguments.drain(index..(index + 2).min(arguments.len()));
            }
            "--allow-dangerously-skip-permissions" => {
                arguments.remove(index);
            }
            _ => index += 1,
        }
    }
    let mut flags = Vec::new();
    if mode == "bypassPermissions" {
        flags.push("--allow-dangerously-skip-permissions".to_string());
    }
    flags.extend(["--permission-mode".into(), mode.to_string()]);
    arguments.splice(0..0, flags);
}

fn current_claude_reply(activities: &[SessionActivity], response: Option<&str>) -> Option<String> {
    let last_prompt = activities
        .iter()
        .rposition(|activity| activity.kind == "prompt")
        .map_or(activities.len(), |index| index + 1);
    activities[last_prompt..]
        .iter()
        .rev()
        .find(|activity| {
            activity.kind == "message"
                && activity.title != "Thinking"
                && response.is_none_or(|expected| {
                    activity.detail.as_deref().map(str::trim) == Some(expected.trim())
                })
        })
        .and_then(|activity| activity.detail.clone())
}

fn claude_error_activity(native_id: &str, detail: &str) -> SessionActivity {
    SessionActivity {
        id: format!(
            "claude:{native_id}:managed-error:{}",
            crate::state::now_millis()
        ),
        kind: "error".into(),
        title: "Erro do Claude".into(),
        detail: Some(detail.chars().take(16 * 1024).collect()),
        status: "failed".into(),
        created_at: crate::state::now_millis(),
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    }
}

fn add_scoped_claude_hooks(payload: &mut TerminalPayload, executable: &Path) -> Result<(), String> {
    if let Some(settings) = crate::integrations::managed_claude_hook_settings(
        &executable.to_string_lossy(),
        &payload.working_directory,
    )? {
        payload
            .arguments
            .splice(0..0, ["--settings".into(), settings]);
    }
    Ok(())
}

fn claude_prompt_event(session_id: &str, native_id: &str) -> HookEvent {
    HookEvent {
        event: HookEventKind::Running,
        session_id: session_id.into(),
        agent: AgentKind::ClaudeCode,
        agent_label: Some("Claude Code".into()),
        session_name: None,
        project: None,
        source: None,
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status_label: None,
        started_at: None,
        process_id: None,
        native_session_id: Some(native_id.into()),
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

fn apply_permission_profile(request: &LaunchRequest, arguments: &mut Vec<String>) {
    match request.agent {
        IntegrationKind::Codex => {
            if let Some(mode) = request.permission_mode.as_ref() {
                let sandbox = match mode {
                    AccessMode::ReadOnly | AccessMode::Plan => Some("read-only"),
                    AccessMode::WorkspaceWrite => Some("workspace-write"),
                    AccessMode::FullAccess => Some("danger-full-access"),
                    AccessMode::Custom => None,
                };
                if let Some(sandbox) = sandbox {
                    arguments.extend(["--sandbox".into(), sandbox.into()]);
                }
            }
            if matches!(
                request.approval_policy.as_deref(),
                Some("untrusted" | "on-request" | "never")
            ) {
                arguments.extend([
                    "--ask-for-approval".into(),
                    request.approval_policy.clone().unwrap_or_default(),
                ]);
            }
        }
        IntegrationKind::Claude => {
            let mode = match request.permission_mode.as_ref() {
                Some(AccessMode::Plan | AccessMode::ReadOnly) => Some("plan"),
                Some(AccessMode::WorkspaceWrite) => Some("acceptEdits"),
                Some(AccessMode::FullAccess) => Some("bypassPermissions"),
                Some(AccessMode::Custom) | None => None,
            };
            if let Some(mode) = mode {
                if mode == "bypassPermissions" {
                    arguments.push("--allow-dangerously-skip-permissions".into());
                }
                arguments.extend(["--permission-mode".into(), mode.into()]);
            }
        }
        IntegrationKind::Antigravity
        | IntegrationKind::OpenCode
        | IntegrationKind::DeepSeek
        | IntegrationKind::Gemini => {}
    }
}

#[cfg(target_os = "linux")]
fn launch_terminal(
    payload: TerminalPayload,
    executable: &Path,
    app_data_dir: &Path,
) -> Result<(), String> {
    let payload_path = persist_terminal_payload(&payload, app_data_dir)?;
    let id = payload_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("session");
    let launches = app_data_dir.join("launches");
    let desktop_path = launches.join(format!("{id}.desktop"));
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Lume session\nExec=\"{}\" terminal-run \"{}\"\nTerminal=true\nNoDisplay=true\n",
        desktop_escape(executable),
        desktop_escape(&payload_path)
    );
    fs::write(&desktop_path, desktop).map_err(|error| error.to_string())?;
    Command::new("gio")
        .arg("launch")
        .arg(&desktop_path)
        .spawn()
        .map_err(|error| format!("Não foi possível abrir o terminal: {error}"))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn launch_terminal(
    payload: TerminalPayload,
    executable: &Path,
    app_data_dir: &Path,
) -> Result<(), String> {
    let payload_path = persist_terminal_payload(&payload, app_data_dir)?;
    if command_available("wt.exe") {
        Command::new("wt.exe")
            .args(windows_terminal_arguments(
                executable,
                &payload_path,
                &payload.working_directory,
            ))
            .spawn()
            .map_err(|error| error.to_string())?;
    } else {
        use std::os::windows::process::CommandExt;

        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        Command::new("cmd.exe")
            .args(["/D", "/K"])
            .arg(executable)
            .arg("terminal-run")
            .arg(&payload_path)
            .current_dir(&payload.working_directory)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map_err(|error| format!("Não foi possível abrir o Prompt de Comando: {error}"))?;
    }
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
fn windows_terminal_arguments(
    executable: &Path,
    payload_path: &Path,
    working_directory: &str,
) -> Vec<String> {
    vec![
        "-w".into(),
        "-1".into(),
        "new-tab".into(),
        "-d".into(),
        working_directory.into(),
        executable.to_string_lossy().into_owned(),
        "terminal-run".into(),
        payload_path.to_string_lossy().into_owned(),
    ]
}

#[cfg(target_os = "macos")]
fn launch_terminal(
    payload: TerminalPayload,
    executable: &Path,
    app_data_dir: &Path,
) -> Result<(), String> {
    let payload_path = persist_terminal_payload(&payload, app_data_dir)?;
    let shell_command = format!(
        "cd -- {} && exec {} terminal-run {}",
        shell_quote(&payload.working_directory),
        shell_quote(&executable.to_string_lossy()),
        shell_quote(&payload_path.to_string_lossy()),
    );
    let script = format!(
        "with timeout of 30 seconds\ntell application \"Terminal\"\ndo script {}\nactivate\nend tell\nend timeout",
        applescript_string(&shell_command),
    );
    let opened = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| format!("Não foi possível abrir o Terminal.app: {error}"))
        .and_then(|output| {
            if output.status.success() {
                return Ok(());
            }
            let detail = String::from_utf8_lossy(&output.stderr);
            if detail.contains("-1743") {
                return Err("Permita que o Lume controle o Terminal em Ajustes do Sistema → Privacidade e Segurança → Automação.".into());
            }
            Err(format!("Não foi possível abrir o Terminal.app: {}", detail.trim()))
        });
    if opened.is_err() {
        let _ = fs::remove_file(&payload_path);
    }
    opened
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\r', "\\r")
            .replace('\n', "\\n"),
    )
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn persist_terminal_payload(
    payload: &TerminalPayload,
    app_data_dir: &Path,
) -> Result<PathBuf, String> {
    let launches = app_data_dir.join("launches");
    fs::create_dir_all(&launches).map_err(|error| error.to_string())?;
    let sequence = LAUNCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let payload_path = launches.join(format!("{}-{sequence}.json", now_millis()));
    fs::write(
        &payload_path,
        serde_json::to_vec(payload).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(payload_path)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn launch_terminal(
    _payload: TerminalPayload,
    _executable: &Path,
    _app_data_dir: &Path,
) -> Result<(), String> {
    Err("Esta plataforma ainda não possui um lançador de terminal".into())
}

fn launch_vscode(payload: &TerminalPayload, agent: &IntegrationKind) -> Result<(), String> {
    let request = serde_json::json!({
        "agent": match agent {
            IntegrationKind::Codex => "codex",
            IntegrationKind::Claude => "claude",
            IntegrationKind::Antigravity => "antigravity",
            IntegrationKind::DeepSeek => "deepseek",
            IntegrationKind::Gemini => "gemini",
            IntegrationKind::OpenCode => "opencode",
        },
        "cwd": payload.working_directory,
        "args": payload.arguments,
    });
    let encoded = percent_encode(&request.to_string());
    crate::integrations::code_command()
        .arg("--reuse-window")
        .arg(format!("vscode://tulerws.lume/session?payload={encoded}"))
        .spawn()
        .map_err(|error| format!("Não foi possível abrir o VS Code: {error}"))?;
    Ok(())
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn desktop_escape(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
}

#[cfg(target_os = "windows")]
fn command_available(command: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new("where.exe")
        .arg(command)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_reply_must_belong_to_current_prompt() {
        let activity = |kind: &str, detail: &str| SessionActivity {
            id: format!("{kind}:{detail}"),
            kind: kind.into(),
            title: kind.into(),
            detail: Some(detail.into()),
            status: "completed".into(),
            created_at: 1,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        };
        let mut activities = vec![
            activity("message", "resposta antiga"),
            activity("prompt", "novo"),
        ];
        assert_eq!(current_claude_reply(&activities, None), None);
        activities.push(activity("message", "resposta nova"));
        assert_eq!(
            current_claude_reply(&activities, None).as_deref(),
            Some("resposta nova")
        );
        assert_eq!(
            current_claude_reply(&activities, Some("resposta nova")).as_deref(),
            Some("resposta nova")
        );
        assert_eq!(
            current_claude_reply(&activities, Some("resposta antiga")),
            None
        );
    }

    fn request(agent: IntegrationKind, resume: bool, resume_id: Option<&str>) -> LaunchRequest {
        LaunchRequest {
            agent,
            working_directory: "/work/project".into(),
            resume,
            resume_id: resume_id.map(str::to_string),
            target: "auto".into(),
            initial_prompt: None,
            permission_mode: None,
            approval_policy: None,
            model: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn codex_remote_is_applied_before_resume_subcommand() {
        let payload = payload_for(
            &request(IntegrationKind::Codex, true, Some("thread-id")),
            Some("ws://127.0.0.1:43131"),
        );
        assert_eq!(payload.command, "codex");
        assert_eq!(
            payload.arguments,
            vec!["--remote", "ws://127.0.0.1:43131", "resume", "thread-id"]
        );
        assert!(payload.retry_quick_resume);
    }

    #[test]
    fn claude_resume_keeps_native_cli_shape() {
        let payload = payload_for(
            &request(IntegrationKind::Claude, true, Some("session-id")),
            None,
        );
        assert_eq!(payload.command, "claude");
        assert_eq!(payload.arguments, vec!["--resume", "session-id"]);
    }

    #[test]
    fn deepseek_launches_the_configured_tui_profile() {
        let payload = payload_for(&request(IntegrationKind::DeepSeek, false, None), None);
        assert_eq!(payload.command, "dsh");
        assert_eq!(payload.arguments, vec!["--profile", "tui"]);
        assert!(!payload.retry_quick_resume);
    }

    #[test]
    fn antigravity_prompt_uses_interactive_mode_when_resuming() {
        let mut request = request(IntegrationKind::Antigravity, true, Some("conversation-id"));
        request.initial_prompt = Some("Continue a tarefa".into());

        let payload = payload_for(&request, None);

        assert_eq!(payload.command, "agy");
        assert_eq!(
            payload.arguments,
            vec![
                "--conversation",
                "conversation-id",
                "-i",
                "Continue a tarefa"
            ]
        );
    }

    #[test]
    fn antigravity_initial_prompt_keeps_a_new_cli_interactive() {
        let mut request = request(IntegrationKind::Antigravity, false, None);
        request.initial_prompt = Some("Inspecione o projeto".into());

        let payload = payload_for(&request, None);

        assert_eq!(payload.command, "agy");
        assert_eq!(payload.arguments, vec!["-i", "Inspecione o projeto"]);
    }

    #[test]
    fn quick_resume_retries_only_fast_startup_failures() {
        assert!(should_retry_quick_resume(
            true,
            0,
            Some(1),
            Duration::from_secs(1)
        ));
        assert!(!should_retry_quick_resume(
            true,
            0,
            Some(130),
            Duration::from_secs(1)
        ));
        assert!(!should_retry_quick_resume(
            true,
            QUICK_RESUME_MAX_ATTEMPTS - 1,
            Some(1),
            Duration::from_secs(1)
        ));
        assert!(!should_retry_quick_resume(
            true,
            0,
            Some(1),
            QUICK_RESUME_EXIT_WINDOW + Duration::from_millis(1)
        ));
    }

    #[test]
    fn claude_resume_applies_the_selected_model_and_effort() {
        let mut request = request(IntegrationKind::Claude, true, Some("session-id"));
        request.model = Some("sonnet".into());
        request.reasoning_effort = Some("high".into());
        let payload = payload_for(&request, None);
        assert_eq!(
            payload.arguments,
            vec![
                "--model",
                "sonnet",
                "--effort",
                "high",
                "--resume",
                "session-id"
            ]
        );
    }

    #[test]
    fn resumed_claude_session_receives_its_prompt() {
        let mut request = request(IntegrationKind::Claude, true, Some("session-id"));
        request.initial_prompt = Some("Continue a tarefa".into());
        let payload = payload_for(&request, None);
        assert_eq!(
            payload.arguments,
            vec!["--print", "--resume", "session-id", "Continue a tarefa"]
        );
    }

    #[test]
    fn claude_messages_queue_in_order_and_a_cancel_pauses_them() {
        let session = "queue-test-session";
        assert_eq!(claude_queue_len(session), 0);
        queue_claude_prompt(session, "a1".into(), "primeira".into());
        queue_claude_prompt(session, "a2".into(), "segunda".into());
        assert_eq!(claude_queue_len(session), 2);

        let take_next = |force: bool| {
            let mut queues = claude_queues().lock().unwrap();
            let queue = queues.get_mut(session).unwrap();
            if queue.paused && !force {
                return None;
            }
            queue.paused = false;
            queue.prompts.pop_front().map(|prompt| prompt.activity_id)
        };
        assert_eq!(
            take_next(false).as_deref(),
            Some("a1"),
            "runs in the order sent"
        );
        // A cancel (or a failure) holds the rest until you ask for it.
        set_claude_queue_paused(session, true);
        assert_eq!(take_next(false), None);
        assert_eq!(claude_queue_len(session), 1, "nothing is lost while paused");
        assert_eq!(
            take_next(true).as_deref(),
            Some("a2"),
            "send now overrides the pause"
        );
        assert_eq!(claude_queue_len(session), 0);
        claude_queues().lock().unwrap().remove(session);
    }

    #[test]
    fn a_stop_from_lume_is_remembered_once() {
        let session = "interrupt-test-session";
        assert!(take_claude_interrupt(session).is_none());
        note_claude_interrupt(session, ClaudeInterrupt::Steer);
        assert!(take_claude_interrupt(session) == Some(ClaudeInterrupt::Steer));
        assert!(
            take_claude_interrupt(session).is_none(),
            "the next exit is an ordinary one"
        );
        note_claude_interrupt(session, ClaudeInterrupt::Cancel);
        forget_claude_interrupt(session);
        assert!(
            take_claude_interrupt(session).is_none(),
            "a stop that failed is forgotten"
        );
    }

    #[test]
    fn claude_prompt_runs_in_the_chosen_permission_mode() {
        let mut request = request(IntegrationKind::Claude, true, Some("session-id"));
        request.initial_prompt = Some("Continue".into());
        request.permission_mode = Some(AccessMode::FullAccess);
        let arguments = |mode: &str| {
            let mut payload = payload_for(&request, None);
            apply_claude_permission_mode(&mut payload.arguments, mode);
            payload.arguments
        };
        // The profile's own flags give way to the mode picked in Lume.
        assert_eq!(
            arguments("default"),
            [
                "--permission-mode",
                "default",
                "--print",
                "--resume",
                "session-id",
                "Continue"
            ]
        );
        assert_eq!(
            arguments("bypassPermissions"),
            [
                "--allow-dangerously-skip-permissions",
                "--permission-mode",
                "bypassPermissions",
                "--print",
                "--resume",
                "session-id",
                "Continue"
            ]
        );
        assert_eq!(arguments("auto")[..2], ["--permission-mode", "auto"]);
    }

    #[test]
    fn claude_session_without_transcript_is_created_with_its_id() {
        let mut request = request(IntegrationKind::Claude, true, Some("session-id"));
        request.initial_prompt = Some("--resume".into());
        let mut payload = payload_for(&request, None);
        start_claude_session_with_id(&mut payload.arguments, "session-id");
        assert_eq!(
            payload.arguments,
            vec!["--print", "--session-id", "session-id", "--resume"]
        );
    }

    #[test]
    fn resumed_claude_prompt_preserves_its_permission_mode() {
        let mut request = request(IntegrationKind::Claude, true, Some("session-id"));
        request.initial_prompt = Some("Continue".into());
        request.permission_mode = Some(AccessMode::WorkspaceWrite);
        request.approval_policy = Some("on-request".into());
        let payload = payload_for(&request, None);
        assert_eq!(
            payload.arguments,
            vec![
                "--print",
                "--permission-mode",
                "acceptEdits",
                "--resume",
                "session-id",
                "Continue",
            ]
        );
    }

    #[test]
    fn windows_terminal_runs_the_shared_payload_runner() {
        assert_eq!(
            windows_terminal_arguments(
                Path::new(r"C:\Program Files\Lume\lume.exe"),
                Path::new(r"C:\Users\user\AppData\Local\Lume\launches\1.json"),
                r"C:\work\project",
            ),
            vec![
                "-w",
                "-1",
                "new-tab",
                "-d",
                r"C:\work\project",
                r"C:\Program Files\Lume\lume.exe",
                "terminal-run",
                r"C:\Users\user\AppData\Local\Lume\launches\1.json",
            ]
        );
    }

    #[test]
    fn windows_terminal_does_not_expose_prompt_or_resume_arguments_to_cmd() {
        let arguments = windows_terminal_arguments(
            Path::new(r"C:\Lume\lume.exe"),
            Path::new(r"C:\Lume\launches\resume.json"),
            r"C:\work\project",
        );
        assert_eq!(arguments.len(), 8);
        assert!(!arguments.iter().any(|value| value == "cmd.exe"));
        assert!(!arguments.iter().any(|value| value == "thread-id"));
    }

    #[test]
    fn codex_project_profile_applies_sandbox_and_approval_policy() {
        let mut request = request(IntegrationKind::Codex, false, None);
        request.permission_mode = Some(AccessMode::WorkspaceWrite);
        request.approval_policy = Some("on-request".into());
        let payload = payload_for(&request, None);
        assert_eq!(
            payload.arguments,
            vec![
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "on-request"
            ]
        );
    }
}
