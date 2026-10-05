use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
use crate::state::now_millis;
use crate::{domain::AccessMode, integrations::IntegrationKind};

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
    let payload = payload_for(&request, codex_remote);
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
        return launch_background(payload);
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

fn launch_background(payload: TerminalPayload) -> Result<(), String> {
    let mut command = crate::executables::command(&payload.command)?;
    command
        .args(&payload.arguments)
        .env("LUME_MANAGED_SESSION", "1")
        .current_dir(&payload.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start {}: {error}", payload.command))?;
    thread::Builder::new()
        .name("lume-agent-prompt".into())
        .spawn(move || {
            let _ = child.wait();
        })
        .map_err(|error| error.to_string())?;
    Ok(())
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
