use std::{
    collections::{HashMap, HashSet},
    thread,
    time::Duration,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{
    io::{BufRead, BufReader},
    path::Path,
};

use sysinfo::{
    get_current_pid, Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind,
};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    agent_plugins::{self, ExternalAgentPlugin},
    domain::{AgentKind, InternalService, SessionSource},
    state::AppState,
};

fn omp_config_root() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".omp")
}

pub(crate) fn omp_native_session_ids(
    pid: Option<u32>,
    command: &[std::ffi::OsString],
    tty_name: Option<&str>,
    root: &std::path::Path,
) -> Option<String> {
    let args = command
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>();
    if let Some(id) = args.windows(2).find_map(|pair| {
        matches!(pair[0].as_ref(), "--resume" | "-r")
            .then(|| pair[1].to_string())
            .filter(|id| !id.is_empty())
    }) {
        return Some(id);
    }
    if let Some(tty_name) = tty_name {
        for directory in omp_terminal_session_dirs(root) {
            let breadcrumb = directory.join(std::path::Path::new(tty_name).file_name()?);
            if let Some(path) = session_path_from_breadcrumb(&breadcrumb) {
                if let Some(id) = omp_session_header(&path).map(|(id, _)| id) {
                    return Some(id);
                }
            }
        }
    }
    if let Some(pid) = pid {
        if let Some(id) = omp_process_lease_id(pid, root) {
            return Some(id);
        }
    }
    None
}

pub(crate) fn omp_native_session_ids_for_pid(
    pid: u32,
    command: &[std::ffi::OsString],
    root: &std::path::Path,
) -> Option<String> {
    let tty_name = omp_process_tty_name(pid, root);
    omp_native_session_ids(Some(pid), command, tty_name.as_deref(), root)
}

fn session_path_from_breadcrumb(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let contents = std::fs::read_to_string(path).ok()?;
    contents
        .lines()
        .find(|line| {
            line.ends_with(".jsonl")
                && (line.contains("/sessions/") || line.contains("\\sessions\\"))
        })
        .map(std::path::PathBuf::from)
}

fn omp_agent_dirs(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![root.join("agent")];
    if let Ok(profiles) = std::fs::read_dir(root.join("profiles")) {
        dirs.extend(
            profiles
                .flatten()
                .map(|entry| entry.path().join("agent"))
                .filter(|dir| dir.is_dir()),
        );
    }
    dirs
}

fn omp_terminal_session_dirs(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    omp_agent_dirs(root)
        .into_iter()
        .map(|dir| dir.join("terminal-sessions"))
        .collect()
}

fn omp_session_header(path: &std::path::Path) -> Option<(String, String)> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    let mut lines = std::io::BufReader::new(file).lines();
    lines.next()?.ok()?;
    let header: serde_json::Value = serde_json::from_str(&lines.next()?.ok()?).ok()?;
    (header["type"] == "session").then(|| {
        Some((
            header["id"].as_str()?.to_string(),
            header["cwd"].as_str()?.to_string(),
        ))
    })?
}
#[cfg(target_os = "linux")]
fn omp_process_tty_name(pid: u32, root: &std::path::Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let device = fields.get(4)?.parse::<i64>().ok()? as u64;
    if device == 0 {
        return None;
    }
    for directory in omp_terminal_session_dirs(root) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let terminals = [
                std::path::Path::new("/dev").join(&name),
                std::path::Path::new("/dev/pts").join(&name),
            ];
            if terminals.iter().any(|terminal| {
                std::fs::metadata(terminal)
                    .ok()
                    .is_some_and(|metadata| metadata.rdev() == device)
            }) {
                return name.into_string().ok();
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn omp_process_tty_name(_pid: u32, _root: &std::path::Path) -> Option<String> {
    None
}
#[cfg(target_os = "macos")]
fn omp_process_tty_name(pid: u32, root: &std::path::Path) -> Option<String> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let read = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int,
        )
    };
    if read != std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int {
        return None;
    }
    let device = unsafe { info.assume_init() }.e_tdev as u64;
    if device == 0 {
        return None;
    }
    for directory in omp_terminal_session_dirs(root) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let terminal = std::path::Path::new("/dev").join(&name);
            if std::fs::metadata(terminal).ok().is_some_and(|metadata| {
                use std::os::unix::fs::MetadataExt;
                metadata.rdev() == device
            }) {
                return name.into_string().ok();
            }
        }
    }
    None
}
#[cfg(target_os = "macos")]
#[repr(C)]
struct ProcFileInfo {
    open_flags: u32,
    status: u32,
    offset: libc::off_t,
    file_type: i32,
    guard_flags: u32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct VnodeFdInfoWithPath {
    file: ProcFileInfo,
    vnode: libc::vnode_info_path,
}

#[cfg(target_os = "macos")]
const PROX_FDTYPE_VNODE: u32 = 1;
#[cfg(target_os = "macos")]
const PROC_PIDFDVNODEPATHINFO: libc::c_int = 2;
#[cfg(target_os = "macos")]
const MAX_DESCRIPTORS: usize = 4096;

#[cfg(target_os = "macos")]
fn omp_process_lease_id(pid: u32, root: &std::path::Path) -> Option<String> {
    use std::os::unix::ffi::OsStrExt;
    let native_pid = libc::pid_t::try_from(pid).ok()?;
    let descriptor_size = std::mem::size_of::<libc::proc_fdinfo>();
    let required = unsafe {
        libc::proc_pidinfo(
            native_pid,
            libc::PROC_PIDLISTFDS,
            0,
            std::ptr::null_mut(),
            0,
        )
    };
    if required <= 0 || required as usize > MAX_DESCRIPTORS * descriptor_size {
        return None;
    }
    let count = ((required as usize).div_ceil(descriptor_size) + 32).min(MAX_DESCRIPTORS);
    let mut descriptors = vec![unsafe { std::mem::zeroed::<libc::proc_fdinfo>() }; count];
    let buffer_size = descriptors.len() * descriptor_size;
    let written = unsafe {
        libc::proc_pidinfo(
            native_pid,
            libc::PROC_PIDLISTFDS,
            0,
            descriptors.as_mut_ptr().cast(),
            buffer_size as libc::c_int,
        )
    };
    if written <= 0 || written as usize > buffer_size || written as usize % descriptor_size != 0 {
        return None;
    }
    descriptors.truncate(written as usize / descriptor_size);

    let session_owners_dir = root.join("run/session-owners");

    for descriptor in descriptors {
        if descriptor.proc_fdtype != PROX_FDTYPE_VNODE || descriptor.proc_fd < 0 {
            continue;
        }
        let mut info = std::mem::MaybeUninit::<VnodeFdInfoWithPath>::zeroed();
        let size = std::mem::size_of::<VnodeFdInfoWithPath>();
        let written = unsafe {
            libc::proc_pidfdinfo(
                native_pid,
                descriptor.proc_fd,
                PROC_PIDFDVNODEPATHINFO,
                info.as_mut_ptr().cast(),
                size as libc::c_int,
            )
        };
        if written != size as libc::c_int {
            continue;
        }
        let info = unsafe { info.assume_init() };
        let bytes = info
            .vnode
            .vip_path
            .iter()
            .flatten()
            .take_while(|&&b| b != 0)
            .map(|&b| b as u8)
            .collect::<Vec<_>>();
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&bytes));
        if path.parent() == Some(&session_owners_dir) {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(id) = file_name.strip_suffix(".lock") {
                    if !id.is_empty() {
                        return Some(id.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn omp_process_lease_id(pid: u32, root: &std::path::Path) -> Option<String> {
    let session_owners_dir = root.join("run/session-owners");
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return None;
    };
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        if target.parent() == Some(&session_owners_dir) {
            if let Some(file_name) = target.file_name().and_then(|n| n.to_str()) {
                if let Some(id) = file_name.strip_suffix(".lock") {
                    if !id.is_empty() {
                        return Some(id.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn omp_process_lease_id(_pid: u32, _root: &std::path::Path) -> Option<String> {
    None
}

#[derive(Clone, Debug)]
pub struct DiscoveredProcess {
    pub agent: AgentKind,
    pub agent_label: String,
    pub process_id: u32,
    #[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
    pub started_at: u64,
    pub native_session_ids: Vec<String>,
    pub working_directory: Option<String>,
    pub source: SessionSource,
}

#[derive(Clone, Debug)]
pub struct ExternalWriterAttempt {
    pub process_id: u32,
    pub native_session_id: String,
}

struct ProcessScan {
    discovered: Vec<DiscoveredProcess>,
    external_writer_attempts: Vec<ExternalWriterAttempt>,
    internal_services: Vec<InternalService>,
    live_pids: HashSet<u32>,
}

/// Built-in agent detection for headless observers; does not acquire ownership,
/// reconcile desktop state, send events or terminate a process.
pub(crate) fn read_only_process_snapshot(system: &mut System) -> Vec<DiscoveredProcess> {
    scan(system, &[], None).discovered
}

pub fn start(state: AppState, app: AppHandle) -> Result<(), String> {
    let managed_proxy_url = app
        .try_state::<crate::codex_bridge::CodexBridge>()
        .map(|bridge| bridge.proxy_url().to_string());
    thread::Builder::new()
        .name("lume-process-discovery".into())
        .spawn(move || {
            let mut system = System::new();
            let mut last_external_conflicts = HashSet::new();
            loop {
                let plugins = agent_plugins::external_catalog(&app);
                let scan = scan(&mut system, &plugins, managed_proxy_url.as_deref());
                let cli_pids = scan
                    .discovered
                    .iter()
                    .filter(|process| process.agent == AgentKind::Codex)
                    .map(|process| process.process_id)
                    .collect::<Vec<_>>();
                crate::codex_identity_probe::observe_runtime(
                    crate::codex_identity_probe::RuntimeStage::Discovery,
                    &cli_pids,
                    false,
                );
                let _ = state.observe_external_writer_attempts(
                    &scan.external_writer_attempts,
                    &scan.live_pids,
                );
                if let Ok(conflicts) = state.list_external_writer_conflicts() {
                    let current = conflicts.iter().cloned().collect::<HashSet<_>>();
                    if current != last_external_conflicts {
                        let _ = app.emit("lume://external-writer-conflicts-changed", conflicts);
                        last_external_conflicts = current;
                    }
                }
                let internal_changed = state
                    .replace_internal_services(scan.internal_services)
                    .unwrap_or(false);
                let reconciliation =
                    state.reconcile_process_snapshot(scan.discovered, scan.live_pids);
                let current_pids = state.codex_cli_process_ids();
                crate::codex_identity_probe::observe_runtime(
                    crate::codex_identity_probe::RuntimeStage::State,
                    current_pids.as_deref().unwrap_or_default(),
                    reconciliation.is_err() || current_pids.is_err(),
                );
                let sessions_changed = reconciliation.unwrap_or(false);
                if internal_changed || sessions_changed {
                    crate::protocol::emit_sessions_changed(&app);
                }
                thread::sleep(Duration::from_secs(2));
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn scan(
    system: &mut System,
    external_plugins: &[ExternalAgentPlugin],
    managed_proxy_url: Option<&str>,
) -> ProcessScan {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            // Keep the all-process inventory cheap. An exec can change a
            // command line without changing its PID; refresh agent/launcher
            // arguments below, before infrastructure and ancestry filters.
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .without_tasks(),
    );
    let command_refresh_pids = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let name = process.name().to_string_lossy().to_lowercase();
            if is_cli_launcher_name(&name) {
                return Some(*pid);
            }
            let arguments = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>();
            (arguments
                .first()
                .is_some_and(|arg| is_cli_launcher_name(arg))
                || detect_agent_arguments(&name, &arguments).is_some()
                || detect_external_agent_arguments(&name, &arguments, external_plugins).is_some())
            .then_some(*pid)
        })
        .collect::<Vec<_>>();
    if !command_refresh_pids.is_empty() {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&command_refresh_pids),
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .without_tasks(),
        );
    }
    let own_pid = get_current_pid().ok();
    let live_pids = system
        .processes()
        .keys()
        .map(|pid| pid.as_u32())
        .collect::<HashSet<_>>();
    let ignored_codex_pids = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let arguments = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>();
            is_codex_infrastructure_arguments(
                &process.name().to_string_lossy().to_lowercase(),
                &arguments,
            )
            .then_some(*pid)
        })
        .collect::<Vec<_>>();
    let candidates = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            if Some(*pid) == own_pid {
                return None;
            }
            let arguments = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>();
            let command = arguments.join(" ");
            let name = process.name().to_string_lossy().to_lowercase();
            if is_codex_infrastructure_arguments(&name, &arguments)
                || is_omp_helper_arguments(&arguments)
            {
                return None;
            }
            if is_claude_headless_resume(&command)
                || crate::claude_control::is_probe_process(pid.as_u32())
            {
                return None;
            }
            if ignored_codex_pids
                .iter()
                .any(|root| process_descends_from(system, *pid, *root))
            {
                return None;
            }
            let (agent, agent_label) = detect_agent_arguments(&name, &arguments)
                .map(|agent| {
                    let label = match agent {
                        AgentKind::Codex => "Codex",
                        AgentKind::ChatGpt => "ChatGPT",
                        AgentKind::Claude => "Claude",
                        AgentKind::ClaudeCode => "Claude Code",
                        AgentKind::Antigravity => "Antigravity",
                        AgentKind::OpenCode => "OpenCode",
                        AgentKind::DeepSeek => "DeepSeek",
                        AgentKind::Gemini => "Gemini",
                        AgentKind::Omp => "Oh My Pi",
                        AgentKind::Unknown => "Agent",
                    };
                    (agent, label.to_string())
                })
                .or_else(|| detect_external_agent_arguments(&name, &arguments, external_plugins))?;
            // The CLI's agent view starts background sessions nobody has used yet.
            if agent == AgentKind::ClaudeCode
                && crate::integrations::claude_background_pid_unused(pid.as_u32())
            {
                return None;
            }
            let working_directory = command_working_directory(process.cmd());
            Some((
                *pid,
                process.parent(),
                agent,
                agent_label,
                working_directory,
            ))
        })
        .collect::<Vec<_>>();
    let candidate_pids = candidates
        .iter()
        .map(|(pid, _, _, _, _)| *pid)
        .collect::<Vec<_>>();
    if !candidate_pids.is_empty() {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&candidate_pids),
            true,
            ProcessRefreshKind::nothing()
                .with_cwd(UpdateKind::Always)
                .without_tasks(),
        );
    }
    let codex_candidate_pids = candidates
        .iter()
        .filter(|(_, _, agent, _, _)| *agent == AgentKind::Codex)
        .map(|(pid, _, _, _, _)| *pid)
        .collect::<Vec<_>>();
    if !codex_candidate_pids.is_empty() {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&codex_candidate_pids),
            true,
            ProcessRefreshKind::nothing()
                .with_environ(UpdateKind::Always)
                .without_tasks(),
        );
    }
    let agents_by_pid = candidates
        .iter()
        .map(|(pid, _, agent, label, _)| (*pid, (agent.clone(), label.clone())))
        .collect::<HashMap<_, _>>();

    // Keep process-owned work visible in Workspace without turning it into a
    // chat, a terminal target, or an extra agent in the main session list.
    let internal_candidates = candidates
        .iter()
        .filter_map(|(pid, _, agent, _, explicit_working_directory)| {
            if *agent != AgentKind::Codex {
                return None;
            }
            let process = system.process(*pid)?;
            let working_directory = explicit_working_directory
                .as_deref()
                .or_else(|| process.cwd().and_then(|path| path.to_str()));
            is_codex_internal_process(system, *pid, working_directory).then(|| {
                (
                    *pid,
                    InternalService {
                        id: format!("codex:internal:{}", pid.as_u32()),
                        agent: AgentKind::Codex,
                        label: "Memories".into(),
                        process_id: pid.as_u32(),
                    },
                )
            })
        })
        .collect::<Vec<_>>();
    let internal_services = internal_candidates
        .iter()
        .filter(|(pid, _)| {
            !internal_candidates.iter().any(|(ancestor, _)| {
                ancestor != pid && process_descends_from(system, *pid, *ancestor)
            })
        })
        .map(|(_, service)| service.clone())
        .collect::<Vec<_>>();
    let internal_pids = internal_candidates
        .iter()
        .map(|(pid, _)| *pid)
        .collect::<HashSet<_>>();

    let discovered = candidates
        .into_iter()
        // Mantém o processo detectado mais próximo da raiz. Um comando executado
        // pelo agente pode conter "codex", "claude" ou "gemini" nos argumentos;
        // escolher esse descendente efêmero faria a sessão trocar de PID.
        .filter(|(pid, _, agent, label, _)| {
            !agents_by_pid
                .iter()
                .any(|(ancestor_pid, (ancestor_agent, ancestor_label))| {
                    ancestor_agent == agent
                        && ancestor_label == label
                        && process_descends_from(&system, *pid, *ancestor_pid)
                })
        })
        .filter_map(|(pid, _, agent, agent_label, explicit_working_directory)| {
            let process = system.process(pid)?;
            let working_directory = explicit_working_directory
                .or_else(|| process.cwd().map(|path| path.to_string_lossy().to_string()));
            if agent == AgentKind::Codex && internal_pids.contains(&pid) {
                return None;
            }
            let native_session_ids = match &agent {
                AgentKind::Codex => native_session_ids_for_process_tree(&system, pid),
                AgentKind::Antigravity => antigravity_session_ids_for_process_tree(&system, pid),
                AgentKind::Omp => {
                    let root = omp_config_root();
                    omp_native_session_ids(
                        Some(pid.as_u32()),
                        process.cmd(),
                        omp_process_tty_name(pid.as_u32(), &root).as_deref(),
                        &root,
                    )
                    .into_iter()
                    .collect()
                }
                _ => Vec::new(),
            };
            Some(DiscoveredProcess {
                // These are actual TUIs. A CLI inside VS Code's integrated
                // terminal is still a CLI, not the Codex extension server.
                source: if agent == AgentKind::Codex {
                    SessionSource::Cli
                } else {
                    source_for(&system, pid)
                },
                agent,
                agent_label,
                process_id: pid.as_u32(),
                started_at: process.start_time(),
                native_session_ids,
                working_directory,
            })
        })
        .collect::<Vec<_>>();
    let external_writer_attempts = discovered
        .iter()
        .filter(|process| {
            process.agent == AgentKind::Codex
                && !system
                    .process(Pid::from_u32(process.process_id))
                    .is_some_and(|process| {
                        is_lume_managed_codex_process(
                            process.cmd(),
                            process.environ(),
                            managed_proxy_url,
                        )
                    })
        })
        .flat_map(|process| {
            process
                .native_session_ids
                .iter()
                .map(|native_session_id| ExternalWriterAttempt {
                    process_id: process.process_id,
                    native_session_id: native_session_id.clone(),
                })
        })
        .collect();

    ProcessScan {
        discovered,
        external_writer_attempts,
        internal_services,
        live_pids,
    }
}

fn is_lume_managed_codex_process(
    command: &[std::ffi::OsString],
    environment: &[std::ffi::OsString],
    managed_proxy_url: Option<&str>,
) -> bool {
    if environment
        .iter()
        .any(|value| value == "LUME_MANAGED_SESSION=1")
    {
        return true;
    }
    // Terminal launchers can reuse a running terminal service and lose the
    // environment marker. Match the complete, authenticated proxy URL, not
    // merely its loopback port: another CLI's endpoint is not Lume ownership.
    let Some(proxy_url) = managed_proxy_url.filter(|url| !url.is_empty()) else {
        return false;
    };
    command.iter().enumerate().any(|(index, argument)| {
        if argument == "--remote" {
            command
                .get(index + 1)
                .is_some_and(|value| value == proxy_url)
        } else {
            argument
                .to_str()
                .and_then(|value| value.strip_prefix("--remote="))
                == Some(proxy_url)
        }
    })
}

fn is_cli_launcher_name(name: &str) -> bool {
    let executable = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim_matches(['\'', '"'])
        .trim_end_matches(".exe");
    matches!(
        executable,
        "node"
            | "nodejs"
            | "bun"
            | "deno"
            | "python"
            | "python3"
            | "bash"
            | "sh"
            | "dash"
            | "zsh"
            | "fish"
            | "cmd"
            | "powershell"
            | "pwsh"
    )
}

/// Servers and updater loops are shared infrastructure, never a user CLI.
/// Inspect the executable and subcommand, not a port or arbitrary prompt text.
pub(crate) fn is_codex_infrastructure_arguments(name: &str, arguments: &[String]) -> bool {
    let executable_name = |value: &str| {
        value
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(value)
            .trim_matches(['\'', '"'])
            .trim_end_matches(".exe")
            .to_string()
    };
    let tokens = launch_tokens(name, arguments);
    if tokens
        .iter()
        .any(|token| executable_name(token) == "codex-code-mode-host")
    {
        return true;
    }
    let Some(executable) = tokens.iter().find(|token| {
        executable_name(token) == "codex" || token.replace('\\', "/").contains("/@openai/codex/")
    }) else {
        return false;
    };
    let Some(index) = arguments
        .iter()
        .position(|argument| argument.as_str() == *executable)
    else {
        return false;
    };
    let mut options = arguments.iter().skip(index + 1);
    while let Some(argument) = options.next() {
        if argument == "--" {
            return false;
        }
        if matches!(
            argument.as_str(),
            "-c" | "--config"
                | "--enable"
                | "--disable"
                | "--remote"
                | "--remote-auth-token-env"
                | "-p"
                | "--profile"
                | "-m"
                | "--model"
                | "-C"
                | "--cd"
                | "--add-dir"
                | "-s"
                | "--sandbox"
                | "-a"
                | "--ask-for-approval"
        ) {
            options.next();
        } else if !argument.starts_with('-') {
            return matches!(argument.as_str(), "app-server" | "exec-server");
        }
    }
    false
}

#[cfg(test)]
fn is_lume_codex_infrastructure_process(command: &str) -> bool {
    let arguments = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    is_codex_infrastructure_arguments("codex", &arguments)
}

fn native_session_ids_for_process_tree(system: &System, root: sysinfo::Pid) -> Vec<String> {
    let Some(root_process) = system.process(root) else {
        return Vec::new();
    };
    let process_tree = system
        .processes()
        .keys()
        .filter(|pid| **pid == root || process_descends_from(system, **pid, root))
        .filter(|pid| native_identity_process_belongs_to_cli_tree(system, **pid, root))
        .filter_map(|pid| system.process(*pid))
        .collect::<Vec<_>>();
    if process_tree.is_empty() {
        return Vec::new();
    }
    // Descendant commands can be unrelated tool calls or nested CLIs. Only the
    // CLI's own launch arguments are fallback evidence, and /resume can age them.
    let command_ids = native_session_ids_from_command(root_process.cmd());

    #[cfg(target_os = "linux")]
    {
        // Descriptor numbers are process-local slots, not recency evidence.
        // Inherited or multiple user-facing rollouts must remain ambiguous.
        let rollout_ids = process_tree
            .iter()
            .flat_map(|process| native_session_ids_for_pid(process.pid()))
            .collect::<Vec<_>>();
        return select_native_session_ids(command_ids, rollout_ids);
    }

    #[cfg(target_os = "macos")]
    {
        // A descriptor belongs to this CLI tree, unlike a recently modified
        // rollout or a thread loaded in a daemon shared by unrelated CLIs.
        // Multiple visible rollouts remain ambiguous; never pick one by time.
        if process_tree.len() > 64 {
            return Vec::new();
        }
        let mut rollout_ids = Vec::new();
        for process in &process_tree {
            let Some(ids) = macos_native_session_ids_for_process(process) else {
                return Vec::new();
            };
            rollout_ids.extend(ids);
        }
        return select_native_session_ids(command_ids, rollout_ids);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    select_native_session_ids(command_ids, Vec::new())
}

fn native_identity_process_belongs_to_cli_tree(
    system: &System,
    pid: sysinfo::Pid,
    root: sysinfo::Pid,
) -> bool {
    native_identity_lineage_belongs_to_cli_tree(pid, root, |pid| {
        let process = system.process(pid)?;
        let arguments = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy().to_lowercase())
            .collect::<Vec<_>>();
        let infrastructure = is_codex_infrastructure_arguments(
            &process.name().to_string_lossy().to_lowercase(),
            &arguments,
        );
        Some((process.parent(), infrastructure))
    })
}

fn native_identity_lineage_belongs_to_cli_tree(
    mut pid: sysinfo::Pid,
    root: sysinfo::Pid,
    mut context: impl FnMut(sysinfo::Pid) -> Option<(Option<sysinfo::Pid>, bool)>,
) -> bool {
    for _ in 0..=12 {
        let Some((parent, infrastructure)) = context(pid) else {
            return false;
        };
        if infrastructure {
            // Even a daemon launched below this CLI may serve other clients.
            // Neither its descriptors nor those of its children identify it.
            return false;
        }
        if pid == root {
            return true;
        }
        let Some(parent) = parent else {
            return false;
        };
        pid = parent;
    }
    false
}

fn select_native_session_ids(command_ids: Vec<String>, rollout_ids: Vec<String>) -> Vec<String> {
    // Open rollouts precede launch arguments, but never resolve ambiguity by
    // falling back to a potentially stale `resume` ID.
    let ids = if rollout_ids.is_empty() {
        command_ids
    } else {
        rollout_ids
    }
    .into_iter()
    .collect::<HashSet<_>>();
    if ids.len() == 1 {
        ids.into_iter().collect()
    } else {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn native_session_ids_for_pid(pid: sysinfo::Pid) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(format!("/proc/{}/fd", pid.as_u32())) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = std::fs::read_link(entry.path()).ok()?;
            let id = codex_rollout_id_from_path(&path)?;
            if !rollout_is_user_facing(&path) {
                return None;
            }
            Some(id)
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_native_session_ids_for_process(process: &sysinfo::Process) -> Option<Vec<String>> {
    use std::{
        ffi::OsStr,
        fs::OpenOptions,
        io::Read,
        mem::{size_of, MaybeUninit},
        os::unix::{ffi::OsStrExt, fs::MetadataExt, fs::OpenOptionsExt},
        ptr,
    };

    // These public libproc ABI definitions are absent from libc's Apple module.
    // Layouts/constants: apple-oss-distributions/xnu, bsd/sys/proc_info.h.

    let pid = process.pid().as_u32();
    let start = crate::codex_identity_probe::process_start_marker(pid)?;
    if start / 1_000_000 != process.start_time() {
        return None;
    }
    let native_pid = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: a null buffer/zero size queries the required byte count.
    let required =
        unsafe { libc::proc_pidinfo(native_pid, libc::PROC_PIDLISTFDS, 0, ptr::null_mut(), 0) };
    let descriptor_size = size_of::<libc::proc_fdinfo>();
    if required <= 0 || required as usize > MAX_DESCRIPTORS * descriptor_size {
        return None;
    }
    // Extra room handles descriptors opened between the query and the read.
    let count = ((required as usize).div_ceil(descriptor_size) + 32).min(MAX_DESCRIPTORS);
    // SAFETY: proc_fdinfo consists solely of integer fields; zero is valid.
    let mut descriptors = vec![unsafe { std::mem::zeroed::<libc::proc_fdinfo>() }; count];
    let buffer_size = descriptors.len() * descriptor_size;
    // SAFETY: this aligned, initialized buffer has exactly buffer_size bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            native_pid,
            libc::PROC_PIDLISTFDS,
            0,
            descriptors.as_mut_ptr().cast(),
            buffer_size as libc::c_int,
        )
    };
    if written <= 0 || written as usize >= buffer_size || written as usize % descriptor_size != 0 {
        // An incomplete inventory cannot establish a unique conversation.
        return None;
    }
    descriptors.truncate(written as usize / descriptor_size);
    let mut ids = Vec::new();
    for descriptor in descriptors {
        if descriptor.proc_fdtype != PROX_FDTYPE_VNODE || descriptor.proc_fd < 0 {
            continue;
        }
        let mut info = MaybeUninit::<VnodeFdInfoWithPath>::zeroed();
        let size = size_of::<VnodeFdInfoWithPath>();
        // SAFETY: the buffer has the vnode_fdinfowithpath C layout and size.
        let written = unsafe {
            libc::proc_pidfdinfo(
                native_pid,
                descriptor.proc_fd,
                PROC_PIDFDVNODEPATHINFO,
                info.as_mut_ptr().cast(),
                size as libc::c_int,
            )
        };
        if written != size as libc::c_int {
            return None;
        }
        // SAFETY: the complete response initialized every field above.
        let info = unsafe { info.assume_init() };
        let stat = &info.vnode.vip_vi.vi_stat;
        if u32::from(stat.vst_mode) & u32::from(libc::S_IFMT) != u32::from(libc::S_IFREG) {
            continue;
        }
        let bytes = info
            .vnode
            .vip_path
            .iter()
            .flatten()
            .map(|byte| *byte as u8)
            .collect::<Vec<_>>();
        let Some(end) = bytes.iter().position(|byte| *byte == 0) else {
            return None;
        };
        let path = Path::new(OsStr::from_bytes(&bytes[..end]));
        let Some(id) = codex_rollout_id_from_path(path).filter(|_| path.is_absolute()) else {
            continue;
        };
        // Never open a terminal/FIFO or follow a replaced symlink. Verify that
        // the metadata still belongs to the regular file observed via libproc.
        let Ok(file) = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        else {
            return None;
        };
        let Ok(metadata) = file.metadata() else {
            return None;
        };
        if !metadata.is_file()
            || metadata.ino() != stat.vst_ino
            || metadata.dev() != u64::from(stat.vst_dev)
            || metadata.uid() != stat.vst_uid
        {
            return None;
        }
        let mut line = String::new();
        if BufReader::new(file.take(16 * 1024))
            .read_line(&mut line)
            .is_err()
        {
            return None;
        }
        let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&line) else {
            return None;
        };
        if metadata.get("type").and_then(serde_json::Value::as_str) != Some("session_meta")
            || metadata
                .get("payload")
                .and_then(|payload| payload.get("id"))
                .and_then(serde_json::Value::as_str)
                != Some(id.as_str())
            || !rollout_metadata_is_user_facing(&metadata)
        {
            continue;
        }
        ids.push(id);
    }
    // PID reuse/process exit during the inspection invalidates every result.
    if crate::codex_identity_probe::process_start_marker(pid) != Some(start) {
        return None;
    }
    Some(ids)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn codex_rollout_id_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".jsonl")?;
    let id = if let Some(rollout) = stem.strip_prefix("rollout-") {
        rollout.get(rollout.len().checked_sub(36)?..)?
    } else {
        stem
    };
    is_codex_session_id(id).then(|| id.to_string())
}

#[cfg(target_os = "linux")]
fn rollout_is_user_facing(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut first_line = String::new();
    if BufReader::new(file).read_line(&mut first_line).is_err() {
        return false;
    }
    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&first_line) else {
        return false;
    };
    metadata.get("type").and_then(serde_json::Value::as_str) == Some("session_meta")
        && rollout_metadata_is_user_facing(&metadata)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rollout_metadata_is_user_facing(metadata: &serde_json::Value) -> bool {
    let Some(payload) = metadata.get("payload") else {
        return false;
    };
    !payload
        .get("parent_thread_id")
        .is_some_and(|parent| !parent.is_null())
        && payload
            .get("thread_source")
            .and_then(serde_json::Value::as_str)
            != Some("subagent")
        && payload
            .get("source")
            .and_then(serde_json::Value::as_object)
            .is_none_or(|source| !source.contains_key("subagent"))
}

pub(crate) fn native_session_ids_from_command(command: &[std::ffi::OsString]) -> Vec<String> {
    let parts = command
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>();
    let mut ids = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let candidate = if part == "resume" || part == "--resume" {
            parts.get(index + 1).map(|value| value.as_ref())
        } else {
            part.strip_prefix("--resume=")
        };
        if let Some(candidate) = candidate.filter(|value| is_codex_session_id(value)) {
            if !ids.iter().any(|existing| existing == candidate) {
                ids.push(candidate.to_string());
            }
        }
    }
    ids
}

fn antigravity_session_ids_for_process_tree(system: &System, root: sysinfo::Pid) -> Vec<String> {
    let ids = system
        .processes()
        .keys()
        .filter(|pid| **pid == root || process_descends_from(system, **pid, root))
        .filter_map(|pid| system.process(*pid))
        .flat_map(|process| antigravity_session_ids_from_command(process.cmd()))
        .collect::<HashSet<_>>();
    if ids.len() == 1 {
        ids.into_iter().collect()
    } else {
        Vec::new()
    }
}

fn antigravity_session_ids_from_command(command: &[std::ffi::OsString]) -> Vec<String> {
    let parts = command
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>();
    let mut ids = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let candidate = if part == "--conversation" {
            parts.get(index + 1).map(|value| value.as_ref())
        } else {
            part.strip_prefix("--conversation=")
        };
        if let Some(candidate) = candidate.filter(|value| is_safe_antigravity_session_id(value)) {
            if !ids.iter().any(|existing| existing == candidate) {
                ids.push(candidate.to_string());
            }
        }
    }
    ids
}

fn is_safe_antigravity_session_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn is_codex_session_id(value: &str) -> bool {
    value.len() == 36
        && value
            .chars()
            .enumerate()
            .all(|(index, character)| match index {
                8 | 13 | 18 | 23 => character == '-',
                _ => character.is_ascii_hexdigit(),
            })
}

fn command_working_directory(command: &[std::ffi::OsString]) -> Option<String> {
    let mut parts = command.iter().map(|part| part.to_string_lossy());
    while let Some(part) = parts.next() {
        if part == "--command-cwd" {
            return parts.next().map(|value| value.into_owned());
        }
        if let Some(value) = part.strip_prefix("--command-cwd=") {
            return Some(value.to_string());
        }
    }
    None
}

fn is_codex_internal_process(
    system: &System,
    mut pid: sysinfo::Pid,
    working_directory: Option<&str>,
) -> bool {
    if working_directory.is_some_and(crate::session_filters::is_codex_internal_workspace) {
        return true;
    }
    for _ in 0..12 {
        let Some(process) = system.process(pid) else {
            break;
        };
        if process.cwd().is_some_and(|path| {
            crate::session_filters::is_codex_internal_workspace(&path.to_string_lossy())
        }) || command_has_codex_internal_workspace(process.cmd())
        {
            return true;
        }
        let Some(parent) = process.parent() else {
            break;
        };
        pid = parent;
    }
    false
}

fn command_has_codex_internal_workspace(command: &[std::ffi::OsString]) -> bool {
    let mut parts = command.iter().map(|part| part.to_string_lossy());
    while let Some(part) = parts.next() {
        if matches!(part.as_ref(), "--command-cwd" | "--sandbox-policy-cwd") {
            if parts
                .next()
                .is_some_and(|path| crate::session_filters::is_codex_internal_workspace(&path))
            {
                return true;
            }
            continue;
        }
        if ["--command-cwd=", "--sandbox-policy-cwd="]
            .iter()
            .find_map(|prefix| part.strip_prefix(prefix))
            .is_some_and(crate::session_filters::is_codex_internal_workspace)
        {
            return true;
        }
    }
    false
}

/// Subcomandos do Claude Code que são infraestrutura, não conversas.
fn is_claude_infrastructure(tokens: &[&str]) -> bool {
    const SUBCOMMANDS: [&str; 2] = ["daemon", "bg-pty-host"];
    tokens
        .windows(2)
        .any(|pair| pair[0] == "claude" && SUBCOMMANDS.contains(&pair[1]))
}

/// Tokens of a command line for infrastructure checks. Claude retitles its
/// daemon processes, so `argv[0]` arrives as the single string
/// `"claude bg-pty-host"`; every element is split to see `claude` and its
/// subcommand side by side whatever the shape.
fn claude_command_tokens(arguments: &[String]) -> Vec<&str> {
    arguments
        .iter()
        .flat_map(|argument| argument.split_whitespace())
        .map(|token| {
            token
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(token)
                .trim_matches(['"', '\''])
        })
        .collect()
}

/// A `claude bg-spare` process is the daemon's pre-warmed pool. It only becomes
/// a conversation once claimed, and then its hooks report it, so the process
/// itself is never listed as a session.
fn is_claude_spare(tokens: &[&str]) -> bool {
    tokens
        .windows(2)
        .any(|pair| pair[0] == "claude" && pair[1] == "bg-spare")
}

/// Whether these arguments belong to the Claude daemon's own processes. Hooks
/// stop climbing there: above them is the CLI that merely launched the daemon.
pub(crate) fn is_claude_daemon_arguments(arguments: &[String]) -> bool {
    is_claude_infrastructure(&claude_command_tokens(arguments))
}

fn is_claude_headless_resume(command: &str) -> bool {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    tokens.iter().any(|token| *token == "--print")
        && tokens
            .iter()
            .any(|token| matches!(*token, "--resume" | "--session-id"))
        && tokens.iter().any(|token| {
            let file = token
                .trim_matches(['"', '\''])
                .split(['/', '\\'])
                .next_back()
                .unwrap_or_default();
            // Windows launches `claude.exe` (or `claude.cmd`); without the extension every headless
            // resume Lume starts for a prompt would be listed as a new Claude Code session.
            matches!(file, "claude" | "claude.exe" | "claude.cmd" | "claude.bat")
        })
}

fn process_descends_from(system: &System, mut child: sysinfo::Pid, ancestor: sysinfo::Pid) -> bool {
    for _ in 0..12 {
        let Some(parent) = system.process(child).and_then(|process| process.parent()) else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        child = parent;
    }
    false
}

fn source_for(system: &System, mut pid: sysinfo::Pid) -> SessionSource {
    for _ in 0..8 {
        let Some(process) = system.process(pid) else {
            break;
        };
        let name = process.name().to_string_lossy().to_lowercase();
        let command = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if name == "code"
            || name == "code.exe"
            || command.contains("visual studio code")
            || command.contains(".vscode/extensions")
        {
            return SessionSource::Vscode;
        }
        let Some(parent) = process.parent() else {
            break;
        };
        pid = parent;
    }
    SessionSource::Cli
}

/// Reconhece o executável versionado instalado abaixo de um diretório `claude`.
fn is_versioned_claude_executable(token: &str) -> bool {
    token
        .split(['/', '\\'])
        .any(|segment| segment.trim_matches(['"', '\'']) == "claude")
}

fn detect_agent(name: &str, command: &str) -> Option<AgentKind> {
    detect_agent_arguments(
        name,
        &command
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>(),
    )
}

/// Only inspect the executable and, for interpreters, its actual script.
/// Prompt text, shell command bodies and search arguments are never executables.
fn launch_tokens<'a>(name: &str, arguments: &'a [String]) -> Vec<&'a str> {
    let Some(first) = arguments.first() else {
        return Vec::new();
    };
    let mut tokens = vec![first.as_str()];
    let executable = first
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(first)
        .trim_matches(['"', '\''])
        .trim_end_matches(".exe");
    if matches!(
        executable,
        "node" | "nodejs" | "bun" | "deno" | "python" | "python3" | "bash" | "sh"
    ) || matches!(
        name.trim_end_matches(".exe"),
        "node" | "nodejs" | "bun" | "deno" | "python" | "python3"
    ) {
        for argument in arguments.iter().skip(1) {
            if matches!(
                argument.as_str(),
                "-c" | "-lc"
                    | "-ic"
                    | "-e"
                    | "--eval"
                    | "-p"
                    | "--print"
                    | "-r"
                    | "--require"
                    | "--import"
            ) {
                break;
            }
            if argument.starts_with('-') {
                continue;
            }
            tokens.push(argument.as_str());
            break;
        }
    }
    tokens
}

fn is_omp_helper_arguments(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        argument.contains("__omp_worker_")
            || matches!(
                argument.rsplit(['/', '\\']).next().unwrap_or(argument),
                "daemon" | "--daemon" | "omp-daemon" | "omp_daemon"
            )
    })
}
pub(crate) fn paths_match(a: &str, b: &std::path::Path) -> bool {
    let path_a = std::path::Path::new(a);
    if path_a == b {
        return true;
    }
    if let (Ok(canon_a), Ok(canon_b)) = (std::fs::canonicalize(path_a), std::fs::canonicalize(b)) {
        return canon_a == canon_b;
    }
    path_a.to_string_lossy().trim_end_matches(['/', '\\'])
        == b.to_string_lossy().trim_end_matches(['/', '\\'])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmpLiveness {
    Live,
    MaybeLive,
    NotLive,
}

pub fn check_omp_session_liveness(
    native_session_id: &str,
    cwd: &str,
    profile: Option<&str>,
) -> OmpLiveness {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );

    let root = omp_config_root();
    let mut any_in_cwd = false;

    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy();
        if name != "omp" && name != "omp.exe" {
            continue;
        }
        let args = process
            .cmd()
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if is_omp_helper_arguments(&args) {
            continue;
        }

        let pid_u32 = pid.as_u32();
        let bound_id = omp_native_session_ids_for_pid(pid_u32, process.cmd(), &root);

        if let Some(bound) = &bound_id {
            if bound == native_session_id {
                return OmpLiveness::Live;
            }
        }

        let tty_name = omp_process_tty_name(pid_u32, &root);
        if let Some(tty) = &tty_name {
            let mut dirs = vec![root.join("agent/terminal-sessions")];
            if let Some(prof) = profile {
                dirs.push(
                    root.join("profiles")
                        .join(prof)
                        .join("agent/terminal-sessions"),
                );
            }
            for dir in dirs {
                let breadcrumb =
                    dir.join(std::path::Path::new(tty).file_name().unwrap_or_default());
                if let Some(path) = session_path_from_breadcrumb(&breadcrumb) {
                    if let Some((id, _)) = omp_session_header(&path) {
                        if id == native_session_id {
                            return OmpLiveness::Live;
                        }
                    }
                }
            }
        }

        if let Some(process_cwd) = process.cwd() {
            if paths_match(cwd, process_cwd) {
                // Windows has no inspectable lease file; another omp in the
                // same cwd is still ambiguous even when its argv names a session.
                if cfg!(target_os = "windows") || bound_id.is_none() {
                    any_in_cwd = true;
                }
            }
        }
    }

    if any_in_cwd {
        OmpLiveness::MaybeLive
    } else {
        OmpLiveness::NotLive
    }
}

pub fn check_omp_takeover_safety(
    native_session_id: &str,
    cwd: &str,
    profile: Option<&str>,
    force: bool,
) -> Result<(), &'static str> {
    match check_omp_session_liveness(native_session_id, cwd, profile) {
        OmpLiveness::Live => Err("omp_session_live: Close omp in the terminal to take control."),
        OmpLiveness::MaybeLive if !cfg!(target_os = "windows") => {
            Err("omp_session_live: Close omp in the terminal to take control.")
        }
        OmpLiveness::MaybeLive if !force => Err("omp_session_maybe_live: Close omp in the terminal to take control, or confirm force takeover."),
        OmpLiveness::MaybeLive | OmpLiveness::NotLive => Ok(()),
    }
}

pub(crate) fn detect_agent_arguments(name: &str, arguments: &[String]) -> Option<AgentKind> {
    let raw_tokens = launch_tokens(name, arguments);
    let command_tokens = claude_command_tokens(arguments);
    let tokens = raw_tokens
        .iter()
        .map(|token| {
            token
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(token)
                .trim_matches(['"', '\''])
        })
        .collect::<Vec<_>>();
    let executable_matches = |candidate: &str| {
        name == candidate
            || name.strip_suffix(".exe") == Some(candidate)
            || name.strip_suffix(".cmd") == Some(candidate)
            || name.strip_suffix(".bat") == Some(candidate)
            || tokens.iter().any(|token| {
                *token == candidate
                    || token.strip_suffix(".exe") == Some(candidate)
                    || token.strip_suffix(".cmd") == Some(candidate)
                    || token.strip_suffix(".bat") == Some(candidate)
                    || token.strip_suffix(".js") == Some(candidate)
                    || token.strip_suffix(".mjs") == Some(candidate)
                    || token.strip_suffix(".cjs") == Some(candidate)
                    || token.strip_suffix(".py") == Some(candidate)
            })
            || raw_tokens.iter().any(|token| {
                let normalized = token.replace('\\', "/");
                match candidate {
                    "codex" => normalized.contains("/@openai/codex/"),
                    "claude" => normalized.contains("/@anthropic-ai/claude-code/"),
                    "gemini" => normalized.contains("/@google/gemini-cli/"),
                    _ => false,
                }
            })
    };
    if executable_matches("omp") {
        (!is_omp_helper_arguments(arguments)).then_some(AgentKind::Omp)
    } else if executable_matches("codex") {
        Some(AgentKind::Codex)
    } else if is_claude_infrastructure(&command_tokens) || is_claude_spare(&command_tokens) {
        None
    } else if executable_matches("claude")
        || raw_tokens
            .first()
            .is_some_and(|executable| is_versioned_claude_executable(executable))
    {
        Some(AgentKind::ClaudeCode)
    } else if matches!(name, "agy" | "agy.exe")
        || tokens
            .iter()
            .any(|token| matches!(*token, "agy" | "agy.exe"))
    {
        (!arguments
            .iter()
            .any(|argument| argument == "--input-format")
            || !arguments.iter().any(|argument| argument == "stream-json"))
        .then_some(AgentKind::Antigravity)
    } else if executable_matches("opencode") && !arguments.iter().any(|argument| argument == "acp")
    {
        Some(AgentKind::OpenCode)
    } else if matches!(name, "dsh" | "dsh.exe")
        || tokens
            .iter()
            .any(|token| matches!(*token, "dsh" | "dsh.exe"))
    {
        Some(AgentKind::DeepSeek)
    } else if executable_matches("gemini") {
        Some(AgentKind::Gemini)
    } else {
        None
    }
}

#[cfg(test)]
fn detect_external_agent(
    name: &str,
    command: &str,
    plugins: &[ExternalAgentPlugin],
) -> Option<(AgentKind, String)> {
    detect_external_agent_arguments(
        name,
        &command
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>(),
        plugins,
    )
}

fn detect_external_agent_arguments(
    name: &str,
    arguments: &[String],
    plugins: &[ExternalAgentPlugin],
) -> Option<(AgentKind, String)> {
    let command = launch_tokens(name, arguments).join(" ");
    plugins.iter().find_map(|plugin| {
        let matches_name = plugin
            .process_names
            .iter()
            .any(|candidate| name == candidate.to_lowercase());
        let matches_command = plugin
            .command_tokens
            .iter()
            .any(|candidate| command.contains(&candidate.to_lowercase()));
        (matches_name || matches_command).then(|| (AgentKind::Unknown, plugin.name.clone()))
    })
}

pub fn terminate_agent_process(
    process_id: u32,
    expected_agent: &AgentKind,
    expected_native_session_id: Option<&str>,
) -> Result<(), String> {
    terminate_agent_process_with_identity_policy(
        process_id,
        expected_agent,
        expected_native_session_id,
        false,
    )
}

pub fn terminate_external_writer_attempt(
    process_id: u32,
    expected_native_session_id: &str,
) -> Result<(), String> {
    terminate_agent_process_with_identity_policy(
        process_id,
        &AgentKind::Codex,
        Some(expected_native_session_id),
        true,
    )
}

fn terminate_agent_process_with_identity_policy(
    process_id: u32,
    expected_agent: &AgentKind,
    expected_native_session_id: Option<&str>,
    require_session_identity: bool,
) -> Result<(), String> {
    let (system, target_pid, targets) = agent_process_tree(process_id, expected_agent)?;
    verify_process_session_identity(
        &system,
        target_pid,
        expected_native_session_id,
        "terminate",
        require_session_identity,
    )?;
    #[cfg(not(target_os = "windows"))]
    if let Some(process) = system.process(target_pid) {
        let _ = process.kill_with(Signal::Interrupt);
        if wait_for_process_exit(target_pid, 8, Duration::from_millis(100)) {
            return Ok(());
        }
    }
    let mut requested_root = false;
    for pid in &targets {
        let Some(process) = system.process(*pid) else {
            continue;
        };
        #[cfg(not(target_os = "windows"))]
        let requested = process.kill_with(Signal::Term).unwrap_or(false);
        #[cfg(target_os = "windows")]
        let requested = process.kill();
        if *pid == target_pid {
            requested_root = requested;
        }
    }
    if !requested_root {
        return Err("O sistema recusou o encerramento do agente".into());
    }
    if wait_for_process_exit(target_pid, 30, Duration::from_millis(100)) {
        return Ok(());
    }
    let mut terminated_root = false;
    for pid in targets {
        let Some(process) = system.process(pid) else {
            continue;
        };
        let terminated = process.kill();
        if pid == target_pid {
            terminated_root = terminated;
        }
    }
    if terminated_root {
        Ok(())
    } else {
        Err("O sistema recusou o encerramento do agente".into())
    }
}

pub fn release_agent_process_for_takeover(
    process_id: u32,
    expected_agent: &AgentKind,
    expected_native_session_id: Option<&str>,
) -> Result<(), String> {
    let (system, target_pid, targets) = agent_process_tree(process_id, expected_agent)?;
    verify_process_session_identity(
        &system,
        target_pid,
        expected_native_session_id,
        "transfer",
        false,
    )?;

    #[cfg(not(target_os = "windows"))]
    if let Some(process) = system.process(target_pid) {
        let _ = process.kill_with(Signal::Interrupt);
        if wait_for_process_exit(target_pid, 8, Duration::from_millis(100)) {
            return Ok(());
        }
    }

    let mut requested_root = false;
    for pid in targets {
        let Some(process) = system.process(pid) else {
            continue;
        };
        #[cfg(not(target_os = "windows"))]
        let requested = process.kill_with(Signal::Term).unwrap_or(false);
        #[cfg(target_os = "windows")]
        let requested = process.kill();
        if pid == target_pid {
            requested_root = requested;
        }
    }
    if !requested_root {
        return Err("The operating system refused to release this agent session".into());
    }

    if wait_for_process_exit(target_pid, 30, Duration::from_millis(100)) {
        return Ok(());
    }
    Err("The external CLI did not close in time; Lume did not take control".into())
}

fn verify_process_session_identity(
    system: &System,
    target_pid: Pid,
    expected_native_session_id: Option<&str>,
    operation: &str,
    require_match: bool,
) -> Result<(), String> {
    let Some(expected_native_session_id) = expected_native_session_id else {
        return Ok(());
    };
    let observed = native_session_ids_for_process_tree(system, target_pid);
    if (require_match && observed.is_empty())
        || (!observed.is_empty() && !observed.iter().any(|id| id == expected_native_session_id))
    {
        return Err(format!(
            "The detected process no longer owns the session selected to {operation}"
        ));
    }
    Ok(())
}

fn wait_for_process_exit(target_pid: Pid, attempts: usize, delay: Duration) -> bool {
    for _ in 0..attempts {
        std::thread::sleep(delay);
        let mut refreshed = System::new();
        refreshed.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[target_pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        if refreshed.process(target_pid).is_none() {
            return true;
        }
    }
    false
}

fn agent_process_tree(
    process_id: u32,
    expected_agent: &AgentKind,
) -> Result<(System, Pid, Vec<Pid>), String> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .without_tasks(),
    );
    let target_pid = Pid::from_u32(process_id);
    let Some(target) = system.process(target_pid) else {
        return Err("The agent process is no longer open".into());
    };
    let command = target
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let name = target.name().to_string_lossy().to_lowercase();
    if detect_agent(&name, &command).as_ref() != Some(expected_agent) {
        return Err("O PID da sessão não pertence mais ao agente esperado".into());
    }
    let arguments = target
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    if is_codex_infrastructure_arguments(&name, &arguments) {
        return Err(
            "O servidor interno do Codex não pode ser encerrado como uma CLI externa".into(),
        );
    }
    if get_current_pid().ok().is_some_and(|own_pid| {
        own_pid == target_pid || process_descends_from(&system, own_pid, target_pid)
    }) {
        return Err(
            "O Lume está sendo executado dentro desse processo e não pode encerrá-lo".into(),
        );
    }

    let mut targets = system
        .processes()
        .keys()
        .copied()
        .filter(|pid| *pid == target_pid || process_descends_from(&system, *pid, target_pid))
        .collect::<Vec<_>>();
    targets.sort_by_key(|pid| std::cmp::Reverse(process_depth(&system, *pid)));
    Ok((system, target_pid, targets))
}

pub fn interrupt_resumed_prompt_process(
    native_session_id: &str,
    expected_agent: &AgentKind,
) -> Result<(), String> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .without_tasks(),
    );
    let process = system.processes().values().find(|process| {
        let command = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>();
        resumed_prompt_command_matches(&command, native_session_id, expected_agent)
    });
    let Some(process) = process else {
        return Err("The running Claude prompt process could not be found".into());
    };
    #[cfg(not(target_os = "windows"))]
    let interrupted = process.kill_with(Signal::Interrupt).unwrap_or(false);
    #[cfg(target_os = "windows")]
    let interrupted = process.kill();
    if interrupted {
        Ok(())
    } else {
        Err("The operating system could not interrupt this prompt safely".into())
    }
}

fn resumed_prompt_command_matches(
    command: &[std::borrow::Cow<'_, str>],
    native_session_id: &str,
    expected_agent: &AgentKind,
) -> bool {
    let joined = command.join(" ").to_lowercase();
    let executable = command
        .first()
        .and_then(|part| std::path::Path::new(part.as_ref()).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_lowercase();
    detect_agent(&executable, &joined).as_ref() == Some(expected_agent)
        && command.iter().any(|part| part.as_ref() == "--print")
        && command.windows(2).any(|parts| {
            matches!(parts[0].as_ref(), "--resume" | "--session-id")
                && parts[1].as_ref() == native_session_id
        })
}

fn process_depth(system: &System, mut pid: Pid) -> usize {
    let mut depth = 0;
    for _ in 0..32 {
        let Some(parent) = system.process(pid).and_then(|process| process.parent()) else {
            break;
        };
        depth += 1;
        pid = parent;
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omp_native_id_uses_resume_and_breadcrumb_without_cwd_fallback() {
        let root = std::env::temp_dir().join(format!(
            "lume-omp-discovery-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temporary omp root");
        let resumed = ["omp", "--resume", "resume-id"].map(std::ffi::OsString::from);
        assert_eq!(
            omp_native_session_ids(None, &resumed, None, &root).as_deref(),
            Some("resume-id")
        );
        let short_resume = ["omp", "-r", "short-id"].map(std::ffi::OsString::from);
        assert_eq!(
            omp_native_session_ids(None, &short_resume, None, &root).as_deref(),
            Some("short-id")
        );

        let agent = root.join("agent");
        let session_dir = agent.join("sessions/-work");
        std::fs::create_dir_all(&session_dir).expect("session directory");
        let session_file = session_dir.join("session.jsonl");
        std::fs::write(
            &session_file,
            format!("\n{{\"type\":\"session\",\"id\":\"breadcrumb-id\",\"cwd\":\"/work\"}}\n"),
        )
        .expect("session file");
        let terminal_dir = agent.join("terminal-sessions");
        std::fs::create_dir_all(&terminal_dir).expect("terminal directory");
        std::fs::write(
            terminal_dir.join("ttys000"),
            format!("/work\n{}\ncwdstat 1 2\n", session_file.display()),
        )
        .expect("breadcrumb");
        let command = ["omp"].map(std::ffi::OsString::from);
        assert_eq!(
            omp_native_session_ids(None, &command, Some("ttys000"), &root).as_deref(),
            Some("breadcrumb-id")
        );

        let newest = session_dir.join("newest.jsonl");
        std::thread::sleep(Duration::from_millis(10));
        std::fs::write(
            &newest,
            "\n{\"type\":\"session\",\"id\":\"newest-id\",\"cwd\":\"/work\"}\n",
        )
        .expect("newest session");
        // Without resume or breadcrumb, an unbound omp process MUST NOT fall back to newest-id or cwd!
        assert_eq!(
            omp_native_session_ids(None, &command, None, &root).as_deref(),
            None
        );
        std::fs::remove_dir_all(&root).expect("remove temporary omp root");
    }

    #[test]
    fn omp_two_sessions_in_same_project_not_misbound() {
        let root = std::env::temp_dir().join(format!(
            "lume-omp-two-sessions-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temporary omp root");
        let agent = root.join("agent");
        let session_dir = agent.join("sessions/-project");
        std::fs::create_dir_all(&session_dir).expect("session directory");
        let sess1_file = session_dir.join("sess1.jsonl");
        let sess2_file = session_dir.join("sess2.jsonl");
        std::fs::write(
            &sess1_file,
            "\n{\"type\":\"session\",\"id\":\"sess-1\",\"cwd\":\"/project\"}\n",
        )
        .expect("session 1");
        std::fs::write(
            &sess2_file,
            "\n{\"type\":\"session\",\"id\":\"sess-2\",\"cwd\":\"/project\"}\n",
        )
        .expect("session 2");

        let terminal_dir = agent.join("terminal-sessions");
        std::fs::create_dir_all(&terminal_dir).expect("terminal directory");
        std::fs::write(
            terminal_dir.join("ttys001"),
            format!("/project\n{}\n", sess1_file.display()),
        )
        .expect("breadcrumb 1");
        std::fs::write(
            terminal_dir.join("ttys002"),
            format!("/project\n{}\n", sess2_file.display()),
        )
        .expect("breadcrumb 2");

        let cmd = ["omp"].map(std::ffi::OsString::from);
        // Each TTY breadcrumb cleanly maps to its own session
        assert_eq!(
            omp_native_session_ids(None, &cmd, Some("ttys001"), &root).as_deref(),
            Some("sess-1")
        );
        assert_eq!(
            omp_native_session_ids(None, &cmd, Some("ttys002"), &root).as_deref(),
            Some("sess-2")
        );

        // Unbound process in same cwd does not misbind to either session
        assert_eq!(
            omp_native_session_ids(None, &cmd, None, &root).as_deref(),
            None
        );

        // Resume command targets specific session
        let resume_sess1 = ["omp", "--resume", "sess-1"].map(std::ffi::OsString::from);
        assert_eq!(
            omp_native_session_ids(None, &resume_sess1, None, &root).as_deref(),
            Some("sess-1")
        );

        std::fs::remove_dir_all(&root).expect("remove temporary omp root");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn warm_process_scan_refreshes_arguments_after_same_pid_exec() {
        for launcher in ["codex", "node"] {
            assert_warm_process_scan_after_exec(launcher);
        }
    }

    #[cfg(target_os = "linux")]
    fn assert_warm_process_scan_after_exec(launcher: &str) {
        use std::{
            io::Write,
            os::unix::process::CommandExt,
            process::{Child, Command, Stdio},
            time::Instant,
        };

        // Only this disposable child is signalled/killed. An exec can replace
        // a launcher's command line without changing its PID or birth time.
        struct Fixture(Child);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut fixture = Fixture(
            Command::new("/bin/bash")
                .arg0(launcher)
                .args([
                    "-c",
                    "printf 'ready\\n'; read stage; exec -a codex /bin/sleep 20",
                    "app-server",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("disposable launcher"),
        );
        let pid = Pid::from_u32(fixture.0.id());
        let mut ready = String::new();
        BufReader::new(fixture.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .expect("launcher ready");
        assert_eq!(ready.trim(), "ready");

        let mut warm = System::new();
        read_only_process_snapshot(&mut warm);
        let original = warm.process(pid).unwrap().cmd().to_vec();
        let born = warm.process(pid).unwrap().start_time();
        writeln!(fixture.0.stdin.as_mut().unwrap()).expect("release owned child");
        let deadline = Instant::now() + Duration::from_secs(2);
        let command_path = format!("/proc/{}/cmdline", fixture.0.id());
        while std::fs::read(&command_path).unwrap_or_default() != b"codex\x0020\x00" {
            assert!(Instant::now() < deadline, "owned child did not exec");
            thread::sleep(Duration::from_millis(10));
        }

        let mut fresh = System::new();
        read_only_process_snapshot(&mut fresh);
        let current = fresh.process(pid).unwrap().cmd().to_vec();
        assert_ne!(original, current, "the command changed, not the PID");
        assert_eq!(fresh.process(pid).unwrap().start_time(), born);
        read_only_process_snapshot(&mut warm);
        assert_eq!(
            warm.process(pid).unwrap().cmd(),
            current,
            "a long-lived detector must not keep the pre-exec launcher command"
        );
    }

    #[test]
    fn managed_codex_cli_is_not_an_external_writer_when_terminal_loses_environment() {
        let proxy_url = "ws://127.0.0.1:43131/?token=private-test-token";
        let separated =
            ["codex", "--remote", proxy_url, "resume", "thread-id"].map(std::ffi::OsString::from);
        let equals = [
            std::ffi::OsString::from("codex.exe"),
            std::ffi::OsString::from(format!("--remote={proxy_url}")),
            std::ffi::OsString::from("resume"),
        ];
        assert!(is_lume_managed_codex_process(
            &separated,
            &[],
            Some(proxy_url)
        ));
        assert!(is_lume_managed_codex_process(&equals, &[], Some(proxy_url)));
        assert!(!is_lume_managed_codex_process(&separated, &[], None));
    }

    #[test]
    fn external_codex_remote_endpoints_are_not_mistaken_for_the_lume_proxy() {
        let proxy_url = "ws://127.0.0.1:43131/?token=private-test-token";
        for other_url in [
            "ws://127.0.0.1:43131",
            "ws://127.0.0.1:43131/?token=another-app",
            "ws://127.0.0.1:49131/?token=private-test-token",
        ] {
            let command = ["codex", "--remote", other_url, "resume", "thread-id"]
                .map(std::ffi::OsString::from);
            assert!(!is_lume_managed_codex_process(
                &command,
                &[],
                Some(proxy_url)
            ));
        }
        let prompt_argument =
            ["codex", "resume", "thread-id", proxy_url].map(std::ffi::OsString::from);
        assert!(!is_lume_managed_codex_process(
            &prompt_argument,
            &[],
            Some(proxy_url)
        ));
    }

    #[test]
    fn managed_codex_environment_marker_still_identifies_the_own_cli() {
        let command = ["codex", "resume", "thread-id"].map(std::ffi::OsString::from);
        assert!(is_lume_managed_codex_process(
            &command,
            &["LUME_MANAGED_SESSION=1".into()],
            None
        ));
        assert!(!is_lume_managed_codex_process(
            &command,
            &["LUME_MANAGED_SESSION=0".into()],
            None
        ));
    }

    #[test]
    fn vscode_codex_app_server_is_ignored_until_a_real_chat_emits_events() {
        assert!(is_lume_codex_infrastructure_process(
            "/home/user/.vscode/extensions/openai.chatgpt/bin/codex app-server"
        ));
        assert!(is_lume_codex_infrastructure_process(
            r"C:\Users\user\.vscode\extensions\openai.chatgpt\bin\codex.exe app-server"
        ));
    }

    #[test]
    fn recognizes_rollout_session_ids_without_accepting_arbitrary_names() {
        assert!(is_codex_session_id("019f8061-7032-7521-b333-84f84c744fa8"));
        assert!(!is_codex_session_id("rollout-memory-maintenance"));
    }

    #[test]
    fn resumed_thread_id_is_recovered_from_the_process_command() {
        let command = [
            "codex",
            "--remote",
            "ws://127.0.0.1:43131",
            "resume",
            "019f8061-7032-7521-b333-84f84c744fa8",
        ]
        .map(std::ffi::OsString::from);
        assert_eq!(
            native_session_ids_from_command(&command),
            vec!["019f8061-7032-7521-b333-84f84c744fa8"],
        );
    }

    #[test]
    fn unrelated_uuid_in_a_prompt_is_not_treated_as_a_thread() {
        let command = ["codex", "explain", "019f8061-7032-7521-b333-84f84c744fa8"]
            .map(std::ffi::OsString::from);
        assert!(native_session_ids_from_command(&command).is_empty());
    }

    #[test]
    fn antigravity_conversation_id_is_recovered_only_from_its_flag() {
        let command = ["agy", "--conversation=conversation_42"].map(std::ffi::OsString::from);
        assert_eq!(
            antigravity_session_ids_from_command(&command),
            vec!["conversation_42"]
        );

        let unsafe_command = ["agy", "--conversation", "../outside"].map(std::ffi::OsString::from);
        assert!(antigravity_session_ids_from_command(&unsafe_command).is_empty());

        let unrelated = ["agy", "describe conversation_42"].map(std::ffi::OsString::from);
        assert!(antigravity_session_ids_from_command(&unrelated).is_empty());
    }

    #[test]
    fn rollout_descriptors_from_multiple_processes_cannot_resolve_conflicting_ids() {
        let first = "019f8061-7032-7521-b333-84f84c744fa8";
        let second = "019fcdac-85c9-77e2-871a-3583aa965a75";
        // FD 54 in one process is not newer than FD 32 in another. Descriptor
        // reuse also makes ordering unsafe even within the same process.
        for descriptors in [
            [(101_u32, 32_u64, first), (202, 54, second)],
            [(101_u32, 54_u64, first), (202, 32, second)],
            [(101_u32, 32_u64, first), (101, 54, second)],
        ] {
            assert!(select_native_session_ids(
                vec![first.into()],
                descriptors
                    .into_iter()
                    .map(|(_, _, id)| id.into())
                    .collect(),
            )
            .is_empty());
        }
    }

    #[test]
    fn duplicate_rollout_descriptors_keep_the_same_unambiguous_identity() {
        let id = "019f8061-7032-7521-b333-84f84c744fa8";
        let descriptors = [(101_u32, 32_u64, id), (202, 54, id), (202, 3, id)];
        assert_eq!(
            select_native_session_ids(
                Vec::new(),
                descriptors
                    .into_iter()
                    .map(|(_, _, id)| id.into())
                    .collect(),
            ),
            vec![id],
        );
    }

    #[test]
    fn open_rollout_identity_precedes_stale_resume_arguments() {
        let old = "019f8061-7032-7521-b333-84f84c744fa8";
        let current = "019fcdac-85c9-77e2-871a-3583aa965a75";
        let command = ["codex", "resume", old].map(std::ffi::OsString::from);
        assert_eq!(
            select_native_session_ids(
                native_session_ids_from_command(&command),
                vec![current.into()]
            ),
            vec![current],
        );
        assert_eq!(
            select_native_session_ids(native_session_ids_from_command(&command), Vec::new()),
            vec![old],
        );
    }

    #[test]
    fn codex_infrastructure_and_its_descendants_do_not_supply_cli_identity() {
        let root = Pid::from_u32(101);
        let wrapper = Pid::from_u32(102);
        let daemon = Pid::from_u32(103);
        let daemon_child = Pid::from_u32(104);
        let contexts = HashMap::from([
            (root, (None, false)),
            (wrapper, (Some(root), false)),
            (daemon, (Some(root), true)),
            (daemon_child, (Some(daemon), false)),
        ]);
        let belongs = |pid| {
            native_identity_lineage_belongs_to_cli_tree(pid, root, |pid| {
                contexts.get(&pid).copied()
            })
        };
        assert!(belongs(root));
        assert!(belongs(wrapper));
        assert!(!belongs(daemon));
        assert!(!belongs(daemon_child));
        assert!(!native_identity_lineage_belongs_to_cli_tree(
            daemon,
            daemon,
            |pid| contexts.get(&pid).copied()
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn internal_subagent_rollouts_are_not_used_as_visible_session_identity() {
        let guardian = serde_json::json!({
            "type": "session_meta",
            "payload": { "source": { "subagent": { "other": "guardian" } } }
        });
        let cli = serde_json::json!({
            "type": "session_meta",
            "payload": { "source": "cli" }
        });
        assert!(!rollout_metadata_is_user_facing(&guardian));
        assert!(rollout_metadata_is_user_facing(&cli));
        for payload in [
            serde_json::json!({ "parent_thread_id": "019f8061-7032-7521-b333-84f84c744fa8", "source": "cli" }),
            serde_json::json!({ "thread_source": "subagent", "source": "cli" }),
        ] {
            assert!(!rollout_metadata_is_user_facing(&serde_json::json!({
                "type": "session_meta", "payload": payload
            })));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn terminal_descriptors_are_rejected_before_any_rollout_read() {
        assert!(codex_rollout_id_from_path(Path::new("/dev/pts/7")).is_none());
        assert!(codex_rollout_id_from_path(Path::new(
            "/home/user/.codex/thread-writer-locks/019f8061-7032-7521-b333-84f84c744fa8.lock"
        ))
        .is_none());
        assert_eq!(
            codex_rollout_id_from_path(Path::new(
                "/home/user/.codex/sessions/2026/07/20/rollout-2026-07-20T13-34-57-019f8061-7032-7521-b333-84f84c744fa8.jsonl"
            )),
            Some("019f8061-7032-7521-b333-84f84c744fa8".into())
        );
    }

    #[test]
    fn lume_codex_app_server_is_ignored_but_its_user_cli_is_detectable() {
        assert!(is_lume_codex_infrastructure_process(
            "codex app-server --listen ws://127.0.0.1:43130"
        ));
        assert!(!is_lume_codex_infrastructure_process(
            "codex --remote ws://127.0.0.1:43131 resume chat"
        ));
        assert!(is_lume_codex_infrastructure_process(
            "codex app-server --listen ws://127.0.0.1:58473"
        ));
        assert!(is_lume_codex_infrastructure_process(
            r"C:\Users\user\codex.exe app-server --listen ws://127.0.0.1:59182"
        ));
        assert!(!is_lume_codex_infrastructure_process(
            "codex --remote ws://127.0.0.1:58473 resume chat"
        ));
        assert!(is_lume_codex_infrastructure_process(
            "codex app-server --listen ws://example.com:58473"
        ));
        assert_eq!(
            detect_agent("codex", "codex --remote ws://127.0.0.1:43131 resume chat"),
            Some(AgentKind::Codex)
        );
    }

    #[test]
    fn shared_codex_servers_and_updaters_are_not_session_processes() {
        for command in [
            "codex app-server --listen unix:// --managed-daemon",
            "codex app-server daemon pid-update-loop",
            "codex app-server",
            "codex -c features.code_mode_host=true app-server --analytics-default-enabled",
            "codex exec-server",
            "codex-code-mode-host",
        ] {
            assert!(is_lume_codex_infrastructure_process(command), "{command}");
        }
    }

    #[test]
    fn codex_server_detection_ignores_prompts_and_config_values() {
        for command in [
            "codex --no-daemon resume --all",
            "codex --remote unix:// resume thread-id",
            "codex --profile app-server resume --all",
            "codex -- app-server",
            "codex exec inspect app-server --listen unix://",
            "bash -lc codex app-server --listen unix://",
        ] {
            assert!(!is_lume_codex_infrastructure_process(command), "{command}");
        }
    }

    #[test]
    fn codex_servers_with_spaces_and_npm_wrappers_are_infrastructure() {
        let windows = [
            r"c:\program files\codex\codex.exe",
            "-c",
            "features.code_mode_host=true",
            "app-server",
            "--listen",
            "unix://",
        ]
        .map(str::to_string);
        assert!(is_codex_infrastructure_arguments("codex.exe", &windows));
        let npm = [
            "node",
            "/opt/node_modules/@openai/codex/bin/codex.js",
            "app-server",
            "--listen",
            "unix://",
        ]
        .map(str::to_string);
        assert!(is_codex_infrastructure_arguments("node", &npm));
    }

    #[test]
    fn windows_agent_executables_are_detected_before_the_first_prompt() {
        assert_eq!(
            detect_agent("codex.exe", r#"C:\Users\dev\.local\bin\codex.exe"#),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            detect_agent("claude.exe", r#"C:\Users\dev\bin\claude.exe"#),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            detect_agent("node.exe", r#"C:\Users\dev\bin\gemini.cmd"#),
            Some(AgentKind::Gemini)
        );
    }

    #[test]
    fn codex_sandbox_command_cwd_wins_over_wrapper_directory() {
        let command = [
            "codex-linux-sandbox",
            "--sandbox-policy-cwd",
            "/home/user",
            "--command-cwd",
            "/home/user/Documents/Projetos/Ideias/Lume",
            "codex",
        ]
        .map(std::ffi::OsString::from);
        assert_eq!(
            command_working_directory(&command).as_deref(),
            Some("/home/user/Documents/Projetos/Ideias/Lume")
        );
    }

    #[test]
    fn command_cwd_equals_syntax_is_supported() {
        let command = [
            "codex.exe",
            "--command-cwd=C:\\Users\\user\\Documents\\Lume",
        ]
        .map(std::ffi::OsString::from);
        assert_eq!(
            command_working_directory(&command).as_deref(),
            Some("C:\\Users\\user\\Documents\\Lume")
        );
    }

    #[test]
    fn codex_memory_maintenance_is_not_a_user_session() {
        let command = [
            "codex-linux-sandbox",
            "--sandbox-policy-cwd",
            "/home/user/.codex/memories",
            "codex",
        ]
        .map(std::ffi::OsString::from);
        assert!(command_has_codex_internal_workspace(&command));
        let regular = [
            "codex-linux-sandbox",
            "--command-cwd=/home/user/Documents/memories",
            "codex",
        ]
        .map(std::ffi::OsString::from);
        assert!(!command_has_codex_internal_workspace(&regular));
    }

    #[test]
    fn claude_infrastructure_is_not_a_session() {
        assert_eq!(
            detect_agent("claude", "/home/user/.local/bin/claude daemon"),
            None
        );
        assert_eq!(
            detect_agent(
                "2.1.220",
                "claude bg-pty-host --bg-pty-host /tmp/cc-daemon/x.sock 200 50 -- /home/user/.local/share/claude/versions/2.1.220"
            ),
            None
        );
    }

    // `argv` exactly as the OS reports it for the processes of `claude`'s
    // daemon: the pty host and the spare retitle `argv[0]` to one string.
    fn argv(elements: &[&str]) -> Vec<String> {
        elements.iter().map(|element| element.to_string()).collect()
    }

    #[test]
    fn claude_daemon_processes_are_not_sessions() {
        let pty_host = argv(&[
            "claude bg-pty-host",
            "--bg-pty-host",
            "/tmp/cc-daemon-1000/d1c0b541/pty/929e9dc1.sock",
            "120",
            "30",
            "--",
            "/home/user/.local/share/claude/versions/2.1.292",
            "--session-id",
            "929e9dc1",
            "--agent",
            "claude",
        ]);
        let spare = argv(&[
            "claude bg-spare",
            "--bg-spare",
            "/tmp/cc-daemon-1000/d1c0b541/spare/da2eba3f.claim.sock",
        ]);
        let daemon = argv(&[
            "/home/user/.local/bin/claude",
            "daemon",
            "run",
            "--origin",
            "transient",
        ]);
        for (name, arguments) in [
            ("2.1.292", &pty_host),
            ("2.1.292", &spare),
            ("claude", &daemon),
        ] {
            assert_eq!(
                detect_agent_arguments(name, arguments),
                None,
                "{arguments:?}"
            );
        }
        // Both ways a spawned `claude` can retitle itself.
        assert!(is_claude_daemon_arguments(&pty_host));
        assert!(is_claude_daemon_arguments(&daemon));
        assert!(is_claude_daemon_arguments(&argv(&[
            "claude",
            "bg-pty-host",
            "--bg-pty-host"
        ])));
        // A spare is a session host once claimed, so hooks must not stop on it.
        assert!(!is_claude_daemon_arguments(&spare));

        let panel = argv(&["claude"]);
        let background = argv(&[
            "/home/user/.local/share/claude/versions/2.1.292",
            "--session-id",
            "929e9dc1",
            "--agent",
            "claude",
            "--inherit-permission-mode",
            "auto",
        ]);
        assert_eq!(
            detect_agent_arguments("claude", &panel),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            detect_agent_arguments("2.1.292", &background),
            Some(AgentKind::ClaudeCode)
        );
    }

    #[test]
    fn claude_headless_resume_is_not_a_second_process_session() {
        assert!(is_claude_headless_resume(
            "/home/user/.local/bin/claude --print --resume session-id prompt"
        ));
        assert!(is_claude_headless_resume(
            "/home/user/.local/bin/claude --print --session-id session-id prompt"
        ));
        assert!(!is_claude_headless_resume(
            "/home/user/.local/bin/claude --resume session-id"
        ));
        assert!(is_claude_headless_resume(
            "\"c:\\users\\joão pedro\\.local\\bin\\claude.exe\" --print --resume session-id prompt"
        ));
    }

    #[test]
    fn interruption_targets_only_the_headless_process_for_the_exact_claude_session() {
        let command = [
            std::borrow::Cow::Borrowed("/home/user/.local/bin/claude"),
            std::borrow::Cow::Borrowed("--print"),
            std::borrow::Cow::Borrowed("--resume"),
            std::borrow::Cow::Borrowed("session-id"),
            std::borrow::Cow::Borrowed("Continue"),
        ];
        assert!(resumed_prompt_command_matches(
            &command,
            "session-id",
            &AgentKind::ClaudeCode
        ));
        assert!(!resumed_prompt_command_matches(
            &command,
            "another-session",
            &AgentKind::ClaudeCode
        ));
        assert!(!resumed_prompt_command_matches(
            &command,
            "session-id",
            &AgentKind::Codex
        ));
    }

    #[test]
    fn versioned_claude_binary_is_a_session() {
        assert_eq!(
            detect_agent(
                "2.1.220",
                "/home/user/.local/share/claude/versions/2.1.220 --session-id 9b7acb3c --fork-session"
            ),
            Some(AgentKind::ClaudeCode)
        );
    }

    #[test]
    fn claude_lookalikes_are_not_sessions() {
        assert_eq!(
            detect_agent("nvim", "/usr/bin/nvim /home/user/.claude/settings.json"),
            None
        );
        assert_eq!(
            detect_agent("nvim", "/usr/bin/nvim /home/user/claude-notes/a.md"),
            None
        );
        assert_eq!(
            detect_agent("nvim", "/usr/bin/nvim /home/user/claude/a.md"),
            None
        );
    }

    #[test]
    fn claude_detection_does_not_change_other_built_in_agents() {
        assert_eq!(
            detect_agent("codex", "/usr/bin/codex"),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            detect_agent("bash", "codex resume abc"),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            detect_agent("codex", "/usr/bin/codex daemon"),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            detect_agent("gemini", "/usr/bin/gemini"),
            Some(AgentKind::Gemini)
        );
        assert_eq!(detect_agent("bash", "gemini chat"), Some(AgentKind::Gemini));
        assert_eq!(
            detect_agent("agy", "/usr/local/bin/agy"),
            Some(AgentKind::Antigravity)
        );
        assert_eq!(
            detect_agent("bash", "agy --conversation abc"),
            Some(AgentKind::Antigravity)
        );
        assert_eq!(
            detect_agent("agy.exe", r#"C:\Users\dev\bin\agy.exe"#),
            Some(AgentKind::Antigravity)
        );
        assert_eq!(
            detect_agent("dsh", "/usr/local/bin/dsh --profile tui"),
            Some(AgentKind::DeepSeek)
        );
        assert_eq!(
            detect_agent("dsh.exe", r#"C:\Users\dev\bin\dsh.exe --profile tui"#),
            Some(AgentKind::DeepSeek)
        );
        assert_eq!(
            detect_agent("claude", "/usr/bin/claude"),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            detect_agent("bash", "claude --resume abc"),
            Some(AgentKind::ClaudeCode)
        );
    }

    #[test]
    fn external_manifest_detects_a_custom_cli_process() {
        let plugin = ExternalAgentPlugin {
            id: "local-agent".into(),
            name: "Local Agent".into(),
            executable: "local-agent".into(),
            process_names: vec!["local-agent".into(), "local-agent.exe".into()],
            command_tokens: vec!["local-agent".into()],
            ..ExternalAgentPlugin::default()
        };
        assert_eq!(
            detect_external_agent("local-agent", "/usr/bin/local-agent", &[plugin]),
            Some((AgentKind::Unknown, "Local Agent".into()))
        );
    }

    #[test]
    fn agent_names_in_searches_or_prompt_arguments_are_not_sessions() {
        for agent in ["codex", "claude", "gemini", "agy", "dsh"] {
            assert_eq!(detect_agent("rg", &format!("rg -n {agent} src")), None);
            assert_eq!(
                detect_agent("bash", &format!("/bin/bash -lc rg -n {agent} src")),
                None
            );
            assert_eq!(
                detect_agent_arguments(
                    "node",
                    &[
                        "/usr/bin/node".into(),
                        "/work/scripts/check.mjs".into(),
                        agent.into(),
                    ]
                ),
                None
            );
        }
        assert_eq!(
            detect_agent_arguments(
                "node",
                &[
                    "/usr/bin/node".into(),
                    "--eval".into(),
                    "require('codex')".into(),
                ]
            ),
            None
        );
    }

    #[test]
    fn node_agent_entrypoints_are_detected_without_matching_arbitrary_arguments() {
        assert_eq!(
            detect_agent_arguments(
                "node",
                &[
                    "/usr/bin/node".into(),
                    "/work/my tools/node_modules/@google/gemini-cli/dist/index.js".into(),
                ]
            ),
            Some(AgentKind::Gemini)
        );
        assert_eq!(
            detect_agent_arguments(
                "node",
                &[
                    "/usr/bin/node".into(),
                    "/work/node_modules/@openai/codex/bin/codex.js".into(),
                ]
            ),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            detect_agent_arguments(
                "node",
                &[
                    "/usr/bin/node".into(),
                    "/work/node_modules/@anthropic-ai/claude-code/cli.js".into(),
                ]
            ),
            Some(AgentKind::ClaudeCode)
        );
    }

    #[test]
    fn omp_detects_cli_but_excludes_worker_and_daemon_processes() {
        assert_eq!(
            detect_agent_arguments("omp", &["omp".into()]),
            Some(AgentKind::Omp)
        );
        assert_eq!(
            detect_agent_arguments("omp", &["omp".into(), "__omp_worker_2".into()]),
            None
        );
        assert_eq!(
            detect_agent_arguments("omp", &["omp".into(), "daemon".into()]),
            None
        );
        assert_eq!(
            detect_agent_arguments("omp.exe", &["C:\\omp.exe".into()]),
            Some(AgentKind::Omp)
        );
    }

    #[test]
    fn external_manifest_markers_in_search_arguments_are_ignored() {
        let plugin = ExternalAgentPlugin {
            id: "local-agent".into(),
            name: "Local Agent".into(),
            executable: "local-agent".into(),
            process_names: vec!["local-agent".into()],
            command_tokens: vec!["local-agent".into()],
            ..ExternalAgentPlugin::default()
        };
        assert_eq!(
            detect_external_agent_arguments(
                "rg",
                &["/usr/bin/rg".into(), "local-agent".into(), "/work".into(),],
                &[plugin]
            ),
            None
        );
    }
}
