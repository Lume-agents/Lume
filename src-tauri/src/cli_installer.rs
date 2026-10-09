//! Installs the agent CLIs, and GitHub's, from inside Lume.
//!
//! The commands are a fixed catalog: the webview only names an agent and one of its methods, never a
//! command line, and the exact command is shown to the user before anything runs.

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_LOG_LINES: usize = 4_000;

#[derive(Clone, Copy)]
enum Runner {
    /// A program resolved the way Lume resolves every agent CLI, with fixed arguments.
    Program(&'static str, &'static [&'static str]),
    PowerShell(&'static str),
    Bash(&'static str),
}

struct Method {
    id: &'static str,
    label: &'static str,
    /// The tool the method needs on this computer, or "" when it brings everything it needs.
    requires: &'static str,
    runner: Runner,
}

impl Method {
    fn display(&self) -> String {
        match self.runner {
            Runner::Program(program, arguments) => std::iter::once(program)
                .chain(arguments.iter().copied())
                .collect::<Vec<_>>()
                .join(" "),
            Runner::PowerShell(script) | Runner::Bash(script) => script.to_string(),
        }
    }

    fn available(&self) -> bool {
        match self.runner {
            Runner::PowerShell(_) => cfg!(target_os = "windows"),
            Runner::Bash(_) => {
                !cfg!(target_os = "windows")
                    && (self.requires.is_empty() || crate::executables::available(self.requires))
            }
            Runner::Program(..) => {
                self.requires.is_empty() || crate::executables::available(self.requires)
            }
        }
    }
}

struct Catalog {
    kind: &'static str,
    label: &'static str,
    /// What the CLI is called on the command line.
    binary: &'static str,
    login: &'static str,
    manual_url: Option<&'static str>,
    methods: Vec<Method>,
}

fn catalog() -> Vec<Catalog> {
    let windows = cfg!(target_os = "windows");
    let mac = cfg!(target_os = "macos");
    let mut claude = vec![];
    if windows {
        claude.push(Method {
            id: "script",
            label: "Official installer",
            requires: "",
            runner: Runner::PowerShell("irm https://claude.ai/install.ps1 | iex"),
        });
    } else {
        claude.push(Method {
            id: "script",
            label: "Official installer",
            requires: "curl",
            runner: Runner::Bash("curl -fsSL https://claude.ai/install.sh | bash"),
        });
    }
    claude.push(Method {
        id: "npm",
        label: "npm",
        requires: "npm",
        runner: Runner::Program("npm", &["install", "-g", "@anthropic-ai/claude-code"]),
    });

    let mut codex = vec![Method {
        id: "npm",
        label: "npm",
        requires: "npm",
        runner: Runner::Program("npm", &["install", "-g", "@openai/codex"]),
    }];
    if mac {
        codex.push(Method {
            id: "brew",
            label: "Homebrew",
            requires: "brew",
            runner: Runner::Program("brew", &["install", "--cask", "codex"]),
        });
    }

    let mut gemini = vec![Method {
        id: "npm",
        label: "npm",
        requires: "npm",
        runner: Runner::Program("npm", &["install", "-g", "@google/gemini-cli"]),
    }];
    if mac {
        gemini.push(Method {
            id: "brew",
            label: "Homebrew",
            requires: "brew",
            runner: Runner::Program("brew", &["install", "gemini-cli"]),
        });
    }

    let mut opencode = vec![];
    if !windows {
        opencode.push(Method {
            id: "script",
            label: "Official installer",
            requires: "curl",
            runner: Runner::Bash("curl -fsSL https://opencode.ai/install | bash"),
        });
    }
    opencode.push(Method {
        id: "npm",
        label: "npm",
        requires: "npm",
        runner: Runner::Program("npm", &["install", "-g", "opencode-ai"]),
    });

    let antigravity = vec![if windows {
        Method {
            id: "script",
            label: "Official installer",
            requires: "",
            runner: Runner::PowerShell("irm https://antigravity.google/cli/install.ps1 | iex"),
        }
    } else {
        Method {
            id: "script",
            label: "Official installer",
            requires: "curl",
            runner: Runner::Bash("curl -fsSL https://antigravity.google/cli/install.sh | bash"),
        }
    }];

    let mut github = vec![];
    if windows {
        github.push(Method {
            id: "winget",
            label: "winget",
            requires: "winget",
            runner: Runner::Program(
                "winget",
                &[
                    "install",
                    "--id",
                    "GitHub.cli",
                    "-e",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                ],
            ),
        });
    } else {
        github.push(Method {
            id: "brew",
            label: "Homebrew",
            requires: "brew",
            runner: Runner::Program("brew", &["install", "gh"]),
        });
    }

    vec![
        Catalog {
            kind: "claude",
            label: "Claude Code",
            binary: "claude",
            login: "claude",
            manual_url: Some("https://docs.claude.com/en/docs/claude-code/setup"),
            methods: claude,
        },
        Catalog {
            kind: "codex",
            label: "Codex",
            binary: "codex",
            login: "codex login",
            manual_url: Some("https://github.com/openai/codex"),
            methods: codex,
        },
        Catalog {
            kind: "gemini",
            label: "Gemini CLI",
            binary: "gemini",
            login: "gemini",
            manual_url: Some("https://github.com/google-gemini/gemini-cli"),
            methods: gemini,
        },
        Catalog {
            kind: "opencode",
            label: "OpenCode",
            binary: "opencode",
            login: "opencode auth login",
            manual_url: Some("https://opencode.ai"),
            methods: opencode,
        },
        Catalog {
            kind: "antigravity",
            label: "Antigravity",
            binary: "agy",
            login: "agy",
            manual_url: Some("https://antigravity.google/cli"),
            methods: antigravity,
        },
        Catalog {
            kind: "github",
            label: "GitHub CLI",
            binary: "gh",
            login: "gh auth login",
            manual_url: Some("https://cli.github.com"),
            methods: github,
        },
    ]
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallMethod {
    pub id: String,
    pub label: String,
    /// The exact command Lume will run.
    pub command: String,
    pub available: bool,
    /// The tool this method needs when it is not installed (for example `npm`).
    pub missing: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub kind: String,
    pub label: String,
    pub installed: bool,
    pub methods: Vec<InstallMethod>,
    /// Where to install it by hand when no method works here.
    pub manual_url: Option<String>,
    /// What to type in a terminal to sign in once it is installed.
    pub login_command: String,
}

pub fn plans() -> Vec<InstallPlan> {
    catalog()
        .into_iter()
        .map(|entry| InstallPlan {
            kind: entry.kind.into(),
            label: entry.label.into(),
            installed: crate::executables::available(entry.binary),
            methods: entry
                .methods
                .iter()
                .map(|method| {
                    let available = method.available();
                    InstallMethod {
                        id: method.id.into(),
                        label: method.label.into(),
                        command: method.display(),
                        available,
                        missing: (!available && !method.requires.is_empty())
                            .then(|| method.requires.to_string()),
                    }
                })
                .collect(),
            manual_url: entry.manual_url.map(str::to_string),
            login_command: entry.login.into(),
        })
        .collect()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallOutput {
    id: String,
    line: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallFinished {
    id: String,
    kind: String,
    success: bool,
    cancelled: bool,
    timed_out: bool,
    code: Option<i32>,
    /// Whether the CLI can be found now.
    installed: bool,
}

struct ActiveInstall {
    id: String,
    cancel: Arc<AtomicBool>,
}

fn active() -> &'static Mutex<Option<ActiveInstall>> {
    static ACTIVE: OnceLock<Mutex<Option<ActiveInstall>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(None))
}

/// Starts an install and returns its id; progress arrives as `lume://cli-install-output` events and
/// the end as `lume://cli-install-finished`.
pub fn start(app: &AppHandle, kind: &str, method_id: &str) -> Result<String, String> {
    let entry = catalog()
        .into_iter()
        .find(|entry| entry.kind == kind)
        .ok_or_else(|| "Unknown CLI".to_string())?;
    let binary = entry.binary;
    let kind_name = entry.kind;
    let method = entry
        .methods
        .into_iter()
        .find(|method| method.id == method_id)
        .ok_or_else(|| "Unknown install method".to_string())?;
    if !method.available() {
        return Err(match method.requires {
            "" => "This install method is not available on this system".to_string(),
            tool => format!("Install `{tool}` first to use this method"),
        });
    }

    let id = format!("install-{}", crate::state::now_millis());
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut running = active()
            .lock()
            .map_err(|_| "Could not start the install".to_string())?;
        if running.is_some() {
            return Err("Another install is already running".into());
        }
        *running = Some(ActiveInstall {
            id: id.clone(),
            cancel: cancel.clone(),
        });
    }

    let mut command = match build_command(&method) {
        Ok(command) => command,
        Err(error) => {
            release(&id);
            return Err(error);
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb");
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            release(&id);
            return Err(format!("Could not start the installer: {error}"));
        }
    };

    let log_budget = Arc::new(Mutex::new(0usize));
    let readers = [
        child.stdout.take().map(|stream| pipe_lines(app, &id, stream, log_budget.clone())),
        child.stderr.take().map(|stream| pipe_lines(app, &id, stream, log_budget.clone())),
    ];
    let app = app.clone();
    let run_id = id.clone();
    let _ = thread::Builder::new()
        .name("lume-cli-install".into())
        .spawn(move || {
            let started = Instant::now();
            let mut cancelled = false;
            let mut timed_out = false;
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) => {}
                    Err(_) => break None,
                }
                if cancel.load(Ordering::SeqCst) {
                    cancelled = true;
                    let _ = child.kill();
                    break child.wait().ok();
                }
                if started.elapsed() > INSTALL_TIMEOUT {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait().ok();
                }
                thread::sleep(Duration::from_millis(100));
            };
            for reader in readers.into_iter().flatten() {
                let _ = reader.join();
            }
            // A new PATH entry written by the installer is not in this process's environment.
            refresh_path();
            release(&run_id);
            let success = status.is_some_and(|status| status.success());
            let _ = app.emit(
                "lume://cli-install-finished",
                InstallFinished {
                    id: run_id,
                    kind: kind_name.into(),
                    success,
                    cancelled,
                    timed_out,
                    code: status.and_then(|status| status.code()),
                    installed: crate::executables::available(binary),
                },
            );
        });
    Ok(id)
}

pub fn cancel(id: &str) -> bool {
    active()
        .lock()
        .ok()
        .and_then(|running| {
            running.as_ref().filter(|install| install.id == id).map(|install| {
                install.cancel.store(true, Ordering::SeqCst);
            })
        })
        .is_some()
}

fn release(id: &str) {
    if let Ok(mut running) = active().lock() {
        if running.as_ref().is_some_and(|install| install.id == id) {
            *running = None;
        }
    }
}

fn build_command(method: &Method) -> Result<Command, String> {
    match method.runner {
        Runner::Program(program, arguments) => {
            let mut command = crate::executables::command(program)?;
            command.args(arguments);
            Ok(command)
        }
        Runner::PowerShell(script) => {
            let mut command = Command::new("powershell.exe");
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ]);
            hide_window(&mut command);
            Ok(command)
        }
        Runner::Bash(script) => {
            let mut command = Command::new("bash");
            command.args(["-c", script]);
            Ok(command)
        }
    }
}

#[cfg(target_os = "windows")]
fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(target_os = "windows"))]
fn hide_window(_command: &mut Command) {}

fn pipe_lines(
    app: &AppHandle,
    id: &str,
    stream: impl Read + Send + 'static,
    budget: Arc<Mutex<usize>>,
) -> thread::JoinHandle<()> {
    let app = app.clone();
    let id = id.to_string();
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::new();
        while reader.read_until(b'\n', &mut buffer).is_ok_and(|read| read > 0) {
            let text = String::from_utf8_lossy(&buffer).into_owned();
            buffer.clear();
            let line = clean_line(&text);
            if line.is_empty() {
                continue;
            }
            let allowed = budget
                .lock()
                .map(|mut used| {
                    *used += 1;
                    *used <= MAX_LOG_LINES
                })
                .unwrap_or(false);
            if allowed {
                let _ = app.emit(
                    "lume://cli-install-output",
                    InstallOutput {
                        id: id.clone(),
                        line,
                    },
                );
            }
        }
    })
}

/// Drops terminal escape sequences and keeps the last redraw of a progress line.
fn clean_line(raw: &str) -> String {
    let last = raw
        .split('\r')
        .map(str::trim_end)
        .filter(|part| !part.is_empty())
        .last()
        .unwrap_or("");
    let mut cleaned = String::with_capacity(last.len());
    let mut characters = last.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            if characters.peek() == Some(&'[') {
                characters.next();
                for next in characters.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if !character.is_control() || character == '\t' {
            cleaned.push(character);
        }
    }
    cleaned.trim().chars().take(400).collect()
}

/// Reads the PATH Windows keeps in the registry, so a CLI that was just installed is found
/// without restarting Lume.
#[cfg(target_os = "windows")]
fn refresh_path() {
    use std::os::windows::process::CommandExt;

    fn registry_path(key: &str) -> Option<String> {
        let output = Command::new("reg")
            .args(["query", key, "/v", "Path"])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let line = text.lines().find(|line| line.contains("REG_"))?;
        let (_, value) = line
            .split_once("REG_EXPAND_SZ")
            .or_else(|| line.split_once("REG_SZ"))?;
        Some(expand_variables(value.trim()))
    }

    fn expand_variables(value: &str) -> String {
        let mut result = String::new();
        let mut rest = value;
        while let Some(start) = rest.find('%') {
            result.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match after.find('%') {
                Some(end) => {
                    let name = &after[..end];
                    match std::env::var(name) {
                        Ok(expanded) => result.push_str(&expanded),
                        Err(_) => {
                            result.push('%');
                            result.push_str(name);
                            result.push('%');
                        }
                    }
                    rest = &after[end + 1..];
                }
                None => {
                    result.push('%');
                    result.push_str(after);
                    rest = "";
                }
            }
        }
        result.push_str(rest);
        result
    }

    let machine =
        registry_path(r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment");
    let user = registry_path(r"HKCU\Environment");
    let mut entries: Vec<std::path::PathBuf> = Vec::new();
    for source in [machine, user].into_iter().flatten() {
        entries.extend(std::env::split_paths(&source));
    }
    if let Some(current) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&current) {
            if !entries.contains(&entry) {
                entries.push(entry);
            }
        }
    }
    if let Ok(joined) = std::env::join_paths(entries) {
        std::env::set_var("PATH", joined);
    }
}

#[cfg(not(target_os = "windows"))]
fn refresh_path() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_covers_every_supported_cli_with_a_fixed_command() {
        let entries = catalog();
        let kinds = entries.iter().map(|entry| entry.kind).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            ["claude", "codex", "gemini", "opencode", "antigravity", "github"]
        );
        for entry in &entries {
            assert!(!entry.methods.is_empty(), "{} has no install method", entry.kind);
            for method in &entry.methods {
                assert!(!method.display().trim().is_empty());
            }
        }
    }

    #[test]
    fn progress_lines_lose_escape_codes_and_keep_the_last_redraw() {
        assert_eq!(clean_line("\u{1b}[32mok\u{1b}[0m installed\r\n"), "ok installed");
        assert_eq!(clean_line("10%\r50%\r100%\n"), "100%");
        assert_eq!(clean_line("   \r\n"), "");
    }

    #[test]
    fn every_plan_names_the_command_that_signs_in() {
        assert!(plans().iter().all(|plan| !plan.login_command.is_empty()));
    }
}
