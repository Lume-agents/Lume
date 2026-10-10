use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant, SystemTime},
};

use notify::{RecursiveMode, Watcher};
use serde_json::Value;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tauri::AppHandle;

use crate::{
    domain::{
        AgentKind, HookEvent, HookEventKind, SessionActivity, SessionControlOrigin, SessionSource,
    },
    event_server,
    state::AppState,
};

const TAIL_BYTES: u64 = 16 * 1024 * 1024;
const BOOTSTRAP_LOOKBACK: Duration = Duration::from_secs(5 * 60);
const RECOVERY_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Default)]
struct ParsedSession {
    id: String,
    cwd: String,
    title: Option<String>,
    started_at: Option<String>,
    status: Option<HookEventKind>,
    status_label: Option<String>,
    last_response: Option<String>,
    pending: HashSet<String>,
    stopped: bool,
    activities: Vec<SessionActivity>,
}

#[derive(Default)]
struct ChildFile {
    len: u64,
    offset: u64,
    modified: Option<SystemTime>,
    session: ParsedSession,
    activity: Option<SessionActivity>,
}

#[derive(Default)]
struct ObservedFile {
    offset: u64,
    len: u64,
    modified: Option<SystemTime>,
    session: ParsedSession,
    profile: Option<String>,
    published: bool,
    children: HashMap<PathBuf, ChildFile>,
}

pub fn start(state: AppState, app: AppHandle) -> Result<(), String> {
    thread::Builder::new()
        .name("lume-omp-session-monitor".into())
        .spawn(move || {
            let Some(root) = config_root() else { return };
            let mut system = System::new();
            let mut observed = initialize(&root, &state, &app, &mut system);
            loop {
                if watch_sessions(&root, &state, &app, &mut observed, &mut system).is_ok() {
                    return;
                }
                poll_live(&state, &app, &mut observed, &mut system);
                thread::sleep(RECOVERY_INTERVAL);
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn config_root() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|home| home.join(".omp"))
}

fn agent_dirs(root: &Path) -> Vec<(PathBuf, Option<String>)> {
    let mut result = vec![(root.join("agent"), None)];
    let Ok(profiles) = fs::read_dir(root.join("profiles")) else {
        return result;
    };
    result.extend(profiles.flatten().filter_map(|entry| {
        let profile = entry.file_name().into_string().ok()?;
        let agent = entry.path().join("agent");
        agent.is_dir().then_some((agent, Some(profile)))
    }));
    result
}

fn root_session_files(agent_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Ok(buckets) = fs::read_dir(agent_dir.join("sessions")) else {
        return paths;
    };
    for bucket in buckets.flatten() {
        if !bucket.path().is_dir() {
            continue;
        }
        let Ok(files) = fs::read_dir(bucket.path()) else {
            continue;
        };
        paths.extend(
            files
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl")),
        );
    }
    paths
}

fn initialize(
    root: &Path,
    state: &AppState,
    app: &AppHandle,
    system: &mut System,
) -> HashMap<PathBuf, ObservedFile> {
    let mut observed = HashMap::new();
    refresh_processes(system);
    let inventory = inventory_omp_processes(root, system);
    let breadcrumbs = terminal_breadcrumbs(root);
    for (agent_dir, profile) in agent_dirs(root) {
        for path in root_session_files(&agent_dir) {
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            let Some((session, offset)) = bootstrap_file(&path) else {
                continue;
            };
            let liveness = check_session_liveness(&session, &path, &inventory, &breadcrumbs);
            let live = matches!(liveness, SessionLiveness::Live | SessionLiveness::Ambiguous);
            let mut file = ObservedFile {
                offset,
                len: metadata.len(),
                modified: metadata.modified().ok(),
                session,
                profile: profile.clone(),
                published: should_publish_bootstrap(metadata.modified().ok(), live),
                ..ObservedFile::default()
            };
            update_children(&path, &mut file);
            if file.published {
                publish_bootstrap(state, app, &mut file);
            }
            observed.insert(path, file);
        }
    }
    observed
}

fn watch_sessions(
    root: &Path,
    state: &AppState,
    app: &AppHandle,
    observed: &mut HashMap<PathBuf, ObservedFile>,
    system: &mut System,
) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = sender.send(event);
    })
    .map_err(|error| error.to_string())?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|error| error.to_string())?;
    let mut last_liveness_check = Instant::now();
    loop {
        match receiver.recv_timeout(RECOVERY_INTERVAL) {
            Ok(Ok(event)) => {
                if last_liveness_check.elapsed() >= RECOVERY_INTERVAL {
                    poll_live(state, app, observed, system);
                    last_liveness_check = Instant::now();
                }
                for path in event.paths {
                    if path.extension().is_some_and(|ext| ext == "jsonl") {
                        let target = parent_session_path(&path).unwrap_or(path);
                        poll_path(root, &target, state, app, observed, system);
                    }
                }
            }
            Ok(Err(error)) => return Err(error.to_string()),
            Err(RecvTimeoutError::Timeout) => {
                poll_live(state, app, observed, system);
                last_liveness_check = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err("Oh My Pi session monitor disconnected".into())
            }
        }
    }
}

fn refresh_processes(system: &mut System) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::OnlyIfNotSet)
            .without_tasks(),
    );
}

fn poll_live(
    state: &AppState,
    app: &AppHandle,
    observed: &mut HashMap<PathBuf, ObservedFile>,
    system: &mut System,
) {
    refresh_processes(system);
    let Some(root) = config_root() else { return };
    let inventory = inventory_omp_processes(&root, system);
    let breadcrumbs = terminal_breadcrumbs(&root);
    let failed_paths = observed
        .iter()
        .filter_map(|(path, file)| {
            if is_terminal(&file.session)
                || file.session.status == Some(HookEventKind::WaitingForInput)
            {
                return None;
            }
            let liveness = check_session_liveness(&file.session, path, &inventory, &breadcrumbs);
            match liveness {
                SessionLiveness::Live | SessionLiveness::Ambiguous => None,
                SessionLiveness::NotLive => Some(path.clone()),
            }
        })
        .collect::<Vec<_>>();
    for path in failed_paths {
        if let Some(file) = observed.get_mut(&path) {
            file.session.status = Some(HookEventKind::Failed);
            file.session.status_label = Some("Sessão encerrada inesperadamente".into());
            file.session.stopped = true;
            publish_status(state, app, file);
        }
    }
}

fn poll_path(
    root: &Path,
    path: &Path,
    state: &AppState,
    app: &AppHandle,
    observed: &mut HashMap<PathBuf, ObservedFile>,
    system: &System,
) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    let len = metadata.len();
    let modified = metadata.modified().ok();
    if !observed.contains_key(path) {
        let Some((session, offset)) = bootstrap_file(path) else {
            return;
        };
        let inventory = inventory_omp_processes(root, system);
        let breadcrumbs = terminal_breadcrumbs(root);
        let liveness = check_session_liveness(&session, path, &inventory, &breadcrumbs);
        let live = matches!(liveness, SessionLiveness::Live | SessionLiveness::Ambiguous);
        let mut file = ObservedFile {
            offset,
            len,
            modified,
            session,
            profile: profile_for_path(root, path),
            published: should_publish_bootstrap(modified, live),
            ..ObservedFile::default()
        };
        update_children(path, &mut file);
        if file.published {
            publish_bootstrap(state, app, &mut file);
        }
        observed.insert(path.to_path_buf(), file);
        return;
    }
    let file = observed.get_mut(path).expect("checked above");
    let changed = len != file.len || modified != file.modified;
    if changed {
        if len < file.offset {
            let Some((session, offset)) = bootstrap_file(path) else {
                return;
            };
            file.session = session;
            file.offset = offset;
            file.published = false;
        } else if len > file.offset {
            if let Some((bytes, offset)) = read_appended(path, file.offset) {
                process_lines(&bytes, &mut file.session);
                file.offset = offset;
            }
        }
        file.len = len;
        file.modified = modified;
        if !file.published {
            let inventory = inventory_omp_processes(root, system);
            let breadcrumbs = terminal_breadcrumbs(root);
            let liveness = check_session_liveness(&file.session, path, &inventory, &breadcrumbs);
            let live = matches!(liveness, SessionLiveness::Live | SessionLiveness::Ambiguous);
            file.published = should_publish_bootstrap(modified, live);
            if file.published {
                publish_bootstrap(state, app, file);
            }
        } else {
            publish_status(state, app, file);
            publish_new_activities(state, app, file);
        }
    }
    if update_children(path, file) && file.published {
        publish_status(state, app, file);
    }
}

fn should_publish_bootstrap(modified: Option<SystemTime>, live: bool) -> bool {
    live || modified
        .and_then(|time| SystemTime::now().duration_since(time).ok())
        .is_some_and(|age| age <= BOOTSTRAP_LOOKBACK)
}

fn profile_for_path(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root.join("profiles"))
        .ok()?
        .components()
        .next()?
        .as_os_str()
        .to_str()
        .map(str::to_string)
}

fn bootstrap_file(path: &Path) -> Option<(ParsedSession, u64)> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut slot = Vec::new();
    let mut header_line = Vec::new();
    read_line_bytes(&mut file, &mut slot)?;
    read_line_bytes(&mut file, &mut header_line)?;
    let header: Value = serde_json::from_slice(&header_line).ok()?;
    if header["type"] != "session" {
        return None;
    }
    let mut session = ParsedSession {
        id: header["id"].as_str()?.to_string(),
        cwd: header["cwd"].as_str()?.to_string(),
        title: title_from_slot(&String::from_utf8_lossy(&slot))
            .or_else(|| header["title"].as_str().map(str::to_string)),
        started_at: header["timestamp"].as_str().map(str::to_string),
        ..ParsedSession::default()
    };
    let (tail, start) = read_tail(&mut file, len)?;
    let first = if start > 0 {
        tail.iter()
            .position(|byte| *byte == b'\n')
            .map_or(tail.len(), |index| index + 1)
    } else {
        0
    };
    let end = tail
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(first, |index| index + 1);
    process_lines(&tail[first..end], &mut session);
    Some((session, start + end as u64))
}

fn bootstrap_child(path: &Path) -> Option<(ParsedSession, u64)> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let (bytes, start) = read_tail(&mut file, len)?;
    let first = if start > 0 {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1)
    } else {
        0
    };
    let end = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(first, |index| index + 1);
    let mut session = ParsedSession::default();
    process_lines(&bytes[first..end], &mut session);
    Some((session, start + end as u64))
}

fn read_tail(file: &mut fs::File, len: u64) -> Option<(Vec<u8>, u64)> {
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some((bytes, start))
}

fn read_line_bytes(reader: &mut impl Read, line: &mut Vec<u8>) -> Option<()> {
    let mut byte = [0u8; 1];
    while reader.read(&mut byte).ok()? == 1 {
        line.push(byte[0]);
        if byte[0] == b'\n' {
            return Some(());
        }
    }
    Some(())
}

fn read_appended(path: &Path, offset: u64) -> Option<(Vec<u8>, u64)> {
    let mut file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(offset)).ok()?;
    file.read_to_end(&mut bytes).ok()?;
    let consumed = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    bytes.truncate(consumed);
    Some((bytes, offset + consumed as u64))
}

fn terminal_breadcrumbs(root: &Path) -> HashSet<PathBuf> {
    let mut paths = HashSet::new();
    for (agent_dir, _) in agent_dirs(root) {
        let Ok(entries) = fs::read_dir(agent_dir.join("terminal-sessions")) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(contents) = fs::read_to_string(entry.path()) else {
                continue;
            };
            for line in contents.lines() {
                if line.ends_with(".jsonl")
                    && (line.contains("/sessions/") || line.contains("\\sessions\\"))
                {
                    paths.insert(PathBuf::from(line));
                }
            }
        }
    }
    paths
}

fn title_from_slot(slot: &str) -> Option<String> {
    let value: Value = serde_json::from_str(slot.trim()).ok()?;
    value["title"]
        .as_str()
        .filter(|title| !title.trim().is_empty())
        .map(str::to_string)
}

fn process_lines(bytes: &[u8], session: &mut ParsedSession) {
    for line in bytes
        .split(|byte| *byte == b'\n')
        .take_while(|line| !line.is_empty())
    {
        let Ok(entry) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        for item in activities_from_entry(&entry) {
            session.activities.push(item);
            if session.activities.len() > 96 {
                session.activities.remove(0);
            }
        }
        match entry["type"].as_str().unwrap_or_default() {
            "message" => {
                let message = &entry["message"];
                match message["role"].as_str().unwrap_or_default() {
                    "user" | "toolResult" => {
                        session.status = Some(HookEventKind::Running);
                        session.stopped = false;
                    }
                    "assistant" => {
                        match message["stopReason"].as_str().unwrap_or_default() {
                            "error" => {
                                session.status = Some(HookEventKind::Failed);
                                session.status_label =
                                    message["errorMessage"].as_str().map(str::to_string);
                                session.stopped = true;
                            }
                            "aborted" => {
                                session.status = Some(HookEventKind::WaitingForInput);
                                session.status_label = Some("Tarefa interrompida".into());
                                session.stopped = true;
                            }
                            "stop" | "length" => {
                                session.status = Some(HookEventKind::Completed);
                                session.last_response = text_content(&message["content"]);
                                session.stopped = true;
                            }
                            _ => {}
                        }
                        if let Some(blocks) = message["content"].as_array() {
                            for block in blocks.iter().filter(|block| block["type"] == "toolCall") {
                                if let Some(id) = block["id"].as_str() {
                                    session.pending.insert(id.to_string());
                                }
                                session.status = Some(if block["name"] == "ask" {
                                    HookEventKind::WaitingForInput
                                } else {
                                    HookEventKind::Running
                                });
                                session.status_label =
                                    (block["name"] == "ask").then(|| "Aguardando resposta".into());
                                session.stopped = false;
                            }
                        }
                    }
                    _ => {}
                }
                if message["role"] == "toolResult" {
                    if let Some(id) = message["toolCallId"].as_str() {
                        session.pending.remove(id);
                    }
                }
            }
            "custom" if entry["customType"] == "session_exit" => {
                if entry["data"]["kind"]
                    .as_str()
                    .is_some_and(|kind| kind != "normal")
                {
                    session.status = Some(HookEventKind::Failed);
                    session.status_label = Some("Sessão encerrada inesperadamente".into());
                    session.stopped = true;
                } else {
                    session.stopped = true;
                    if session.pending.is_empty() {
                        session.status = Some(HookEventKind::Completed);
                        session.status_label = Some("Concluído".into());
                    }
                }
            }
            "custom" if entry["customType"] == "tool_execution_start" => {
                if let Some(id) = entry["data"]["toolCallId"].as_str() {
                    session.pending.insert(id.to_string());
                }
                if entry["data"]["toolName"] == "ask" {
                    session.status = Some(HookEventKind::WaitingForInput);
                    session.status_label = Some("Aguardando resposta".into());
                } else {
                    session.status = Some(HookEventKind::Running);
                    session.status_label = Some("Rodando".into());
                }
                session.stopped = false;
            }
            _ => {}
        }
        if !session.pending.is_empty()
            && !session.stopped
            && session.status != Some(HookEventKind::WaitingForInput)
        {
            session.status = Some(HookEventKind::Running);
            session.status_label = Some("Rodando".into());
        }
    }
    session.status.get_or_insert(HookEventKind::SessionStarted);
}

fn text_content(content: &Value) -> Option<String> {
    let text = content
        .as_array()?
        .iter()
        .filter_map(|part| {
            (part["type"] == "text")
                .then(|| part["text"].as_str())
                .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn activity_from_entry(entry: &Value) -> Option<SessionActivity> {
    let message = &entry["message"];
    let (kind, title, detail) = match entry["type"].as_str()? {
        "message" => match message["role"].as_str()? {
            "user" => (
                "prompt",
                "Prompt enviado",
                message["content"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| text_content(&message["content"]))?,
            ),
            "assistant" => (
                "message",
                "Resposta do agente",
                text_content(&message["content"])?,
            ),
            _ => return None,
        },
        _ => return None,
    };
    let timestamp = entry["timestamp"]
        .as_str()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|time| time.timestamp_millis())
        .unwrap_or_default();
    Some(SessionActivity {
        id: entry["id"].as_str().unwrap_or_default().to_string(),
        kind: kind.into(),
        title: title.into(),
        status: "completed".into(),
        detail: (!detail.is_empty()).then_some(detail),
        created_at: timestamp,
        files: vec![],
        attachments: vec![],
        append_detail: false,
    })
}

fn activities_from_entry(entry: &Value) -> Vec<SessionActivity> {
    let mut activities = activity_from_entry(entry).into_iter().collect::<Vec<_>>();
    if entry["type"] == "message" && entry["message"]["role"] == "assistant" {
        if let Some(blocks) = entry["message"]["content"].as_array() {
            for block in blocks.iter().filter(|block| block["type"] == "toolCall") {
                let timestamp = entry["timestamp"]
                    .as_str()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|time| time.timestamp_millis())
                    .unwrap_or_default();
                activities.push(tool_activity(
                    block["id"].as_str().unwrap_or_default(),
                    block["name"].as_str().unwrap_or("Ferramenta"),
                    &block["arguments"],
                    "completed",
                    timestamp,
                ));
            }
        }
    }
    activities
}

/// One transcript row per omp tool call, shared by the file watcher and the RPC bridge so a
/// conversation keeps the same rows when Lume takes control of it.
pub(crate) fn tool_activity(
    call_id: &str,
    name: &str,
    arguments: &Value,
    status: &str,
    created_at: i64,
) -> SessionActivity {
    // Arguments can carry whole files; keep only what the activity row shows.
    // `i` is omp's one-line intent; edit carries `[path#TAG]` headers in `input`.
    let intent = arguments["i"].as_str().map(str::to_string);
    let (kind, title, detail, files) = match name {
        "bash" => (
            "command",
            "Comando",
            arguments["command"].as_str().map(str::to_string),
            vec![],
        ),
        "eval" => (
            "command",
            "Comando",
            arguments["title"].as_str().map(str::to_string).or(intent),
            vec![],
        ),
        "write" => (
            "file",
            "Arquivos alterados",
            intent,
            arguments["path"]
                .as_str()
                .map(str::to_string)
                .into_iter()
                .collect(),
        ),
        "edit" => (
            "file",
            "Arquivos alterados",
            intent,
            arguments["input"]
                .as_str()
                .unwrap_or_default()
                .lines()
                .filter_map(|line| {
                    line.strip_prefix('[')?
                        .split_once('#')
                        .map(|(path, _)| path.to_string())
                })
                .collect(),
        ),
        "todo" => ("plan", "Plano atualizado", intent, vec![]),
        _ => (
            "tool",
            name,
            intent
                .or_else(|| arguments["path"].as_str().map(str::to_string))
                .map(|text| text.chars().take(2_000).collect()),
            vec![],
        ),
    };
    SessionActivity {
        id: format!("omp:tool:{call_id}"),
        kind: kind.into(),
        title: title.into(),
        status: status.into(),
        detail,
        created_at,
        files,
        attachments: vec![],
        append_detail: false,
    }
}

fn event_for(file: &ObservedFile, event: HookEventKind) -> HookEvent {
    let session = &file.session;
    HookEvent {
        event,
        session_id: format!("omp:{}", session.id),
        agent: AgentKind::Omp,
        agent_label: Some(
            file.profile
                .as_deref()
                .map(|name| format!("Oh My Pi · {name}"))
                .unwrap_or_else(|| "Oh My Pi".into()),
        ),
        session_name: session.title.clone(),
        project: Some(
            Path::new(&session.cwd)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Oh My Pi")
                .to_string(),
        ),
        source: Some(SessionSource::Cli),
        source_app: None,
        control_origin: SessionControlOrigin::External,
        status_label: session.status_label.clone(),
        started_at: session.started_at.clone(),
        process_id: None,
        native_session_id: Some(session.id.clone()),
        working_directory: Some(session.cwd.clone()),
        permission_profile: None,
        permission: None,
        question: None,
        last_response: session.last_response.clone(),
        activity: None,
        activities: file
            .children
            .values()
            .filter_map(|child| child.activity.clone())
            .collect(),
        wait_for_decision: false,
    }
}

fn publish_status(state: &AppState, app: &AppHandle, file: &ObservedFile) {
    if state.omp_native_owned_by_lume(&file.session.id) {
        return;
    }
    if let Some(status) = file.session.status {
        let _ = event_server::publish_event(state, app, event_for(file, status));
    }
}

fn publish_activity(
    state: &AppState,
    app: &AppHandle,
    file: &ObservedFile,
    activity: SessionActivity,
) {
    if state.omp_native_owned_by_lume(&file.session.id) {
        return;
    }
    let mut event = event_for(file, HookEventKind::Activity);
    event.activity = Some(activity);
    let _ = event_server::publish_event(state, app, event);
}

fn publish_bootstrap(state: &AppState, app: &AppHandle, file: &mut ObservedFile) {
    publish_status(state, app, file);
    let activities = std::mem::take(&mut file.session.activities);
    for activity in activities {
        publish_activity(state, app, file, activity);
    }
}

fn publish_new_activities(state: &AppState, app: &AppHandle, file: &mut ObservedFile) {
    let activities = std::mem::take(&mut file.session.activities);
    for activity in activities {
        publish_activity(state, app, file, activity);
    }
}

fn update_children(parent: &Path, file: &mut ObservedFile) -> bool {
    let directory = parent.with_extension("");
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    let mut changed = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let length = metadata.len();
        let modified = metadata.modified().ok();
        if file
            .children
            .get(&path)
            .is_some_and(|child| child.len == length && child.modified == modified)
        {
            continue;
        }
        changed = true;
        let agent_id = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let child = file.children.entry(path.clone()).or_default();
        if child.offset == 0 || length < child.offset {
            let Some((session, offset)) = bootstrap_child(&path) else {
                continue;
            };
            child.session = session;
            child.offset = offset;
        } else if length > child.offset {
            if let Some((bytes, offset)) = read_appended(&path, child.offset) {
                process_lines(&bytes, &mut child.session);
                child.offset = offset;
            }
        }
        child.len = length;
        child.modified = modified;
        let mut reader = fs::File::open(&path).ok();
        let mut slot = Vec::new();
        let mut header_line = Vec::new();
        let header = reader.as_mut().and_then(|reader| {
            read_line_bytes(reader, &mut slot)?;
            read_line_bytes(reader, &mut header_line)?;
            serde_json::from_slice::<Value>(&header_line).ok()
        });
        let name = header
            .as_ref()
            .and_then(|header| header["agent"].as_str().or_else(|| header["name"].as_str()))
            .unwrap_or(agent_id);
        let status = match child.session.status {
            Some(HookEventKind::Completed) => "completed",
            Some(HookEventKind::Failed) => "failed",
            Some(HookEventKind::WaitingForInput) if child.session.stopped => "interrupted",
            _ => "running",
        };
        let timestamp = modified
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|time| time.as_millis() as i64)
            .unwrap_or_default();
        child.activity = Some(SessionActivity {
            id: format!("omp:{}:subagent:{agent_id}", file.session.id),
            kind: "subagent".into(),
            title: format!("Subagente · {name}"),
            status: status.into(),
            detail: None,
            created_at: timestamp,
            files: vec![],
            attachments: vec![],
            append_detail: false,
        });
    }
    changed
}

fn is_terminal(session: &ParsedSession) -> bool {
    session.stopped
        || matches!(
            session.status,
            Some(HookEventKind::Completed | HookEventKind::Failed)
        )
}

fn parent_session_path(child: &Path) -> Option<PathBuf> {
    let child_dir = child.parent()?;
    let parent_name = child_dir.file_name()?.to_str()?;
    let parent = child_dir
        .parent()?
        .join(parent_name)
        .with_extension("jsonl");
    parent.is_file().then_some(parent)
}

#[derive(Default, Debug)]
struct OmpProcessInventory {
    bound_sessions: HashSet<String>,
    unbound_cwds: HashSet<PathBuf>,
}

fn inventory_omp_processes(root: &Path, system: &System) -> OmpProcessInventory {
    let mut bound_sessions = HashSet::new();
    let mut unbound_cwds = HashSet::new();

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
        if args
            .iter()
            .any(|arg| arg.contains("__omp_worker_") || arg.contains("daemon"))
        {
            continue;
        }

        let pid_u32 = pid.as_u32();
        let bound_id =
            crate::discovery::omp_native_session_ids_for_pid(pid_u32, process.cmd(), root);

        if let Some(id) = bound_id {
            bound_sessions.insert(id);
        } else if let Some(cwd) = process.cwd() {
            unbound_cwds.insert(cwd.to_path_buf());
            if let Ok(canonical) = fs::canonicalize(cwd) {
                unbound_cwds.insert(canonical);
            }
        }
    }

    OmpProcessInventory {
        bound_sessions,
        unbound_cwds,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionLiveness {
    Live,
    Ambiguous,
    NotLive,
}

fn check_session_liveness(
    session: &ParsedSession,
    session_path: &Path,
    inventory: &OmpProcessInventory,
    breadcrumbs: &HashSet<PathBuf>,
) -> SessionLiveness {
    if inventory.bound_sessions.contains(&session.id) {
        return SessionLiveness::Live;
    }
    if breadcrumbs.contains(session_path) {
        return SessionLiveness::Live;
    }
    let session_cwd = Path::new(&session.cwd);
    if inventory.unbound_cwds.contains(session_cwd) {
        return SessionLiveness::Ambiguous;
    }
    if let Ok(canonical) = fs::canonicalize(session_cwd) {
        if inventory.unbound_cwds.contains(&canonical) {
            return SessionLiveness::Ambiguous;
        }
    }
    SessionLiveness::NotLive
}

pub fn load_subagent_timeline(
    parent_id: &str,
    agent_id: &str,
) -> Result<Vec<SessionActivity>, String> {
    let root = config_root().ok_or_else(|| "Oh My Pi config root unavailable".to_string())?;
    for (agent_dir, _) in agent_dirs(&root) {
        for path in root_session_files(&agent_dir) {
            if session_header_id(&path).as_deref() == Some(parent_id) {
                let transcript = path.with_extension("").join(format!("{agent_id}.jsonl"));
                return timeline_from_jsonl(
                    &fs::read(transcript).map_err(|error| error.to_string())?,
                );
            }
        }
    }
    Err("Oh My Pi subagent transcript not found".into())
}

fn session_header_id(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut slot = Vec::new();
    let mut header = Vec::new();
    read_line_bytes(&mut file, &mut slot)?;
    read_line_bytes(&mut file, &mut header)?;
    let value: Value = serde_json::from_slice(&header).ok()?;
    (value["type"] == "session")
        .then(|| value["id"].as_str().map(str::to_string))
        .flatten()
}

fn timeline_from_jsonl(bytes: &[u8]) -> Result<Vec<SessionActivity>, String> {
    Ok(bytes
        .split(|byte| *byte == b'\n')
        .flat_map(|line| {
            if line.is_empty() {
                return Vec::new();
            }
            let Ok(entry) = serde_json::from_slice::<Value>(line) else {
                return Vec::new();
            };
            activities_from_entry(&entry)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_updates_status_incrementally() {
        let mut session = ParsedSession::default();
        process_lines(
            br#"{"type":"message","message":{"role":"user","content":"hello"}}"#.as_slice(),
            &mut session,
        );
        assert_eq!(session.status, Some(HookEventKind::Running));
        assert_eq!(session.activities.len(), 1);
        process_lines(br#"{"type":"message","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"Done"}]}}"#.as_slice(), &mut session);
        assert_eq!(session.status, Some(HookEventKind::Completed));
        assert_eq!(session.activities.len(), 2);
        assert_eq!(session.last_response.as_deref(), Some("Done"));
    }

    #[test]
    fn old_idle_sessions_are_not_visible_at_bootstrap() {
        let old = SystemTime::now() - Duration::from_secs(BOOTSTRAP_LOOKBACK.as_secs() + 1);
        assert!(!should_publish_bootstrap(Some(old), false));
        assert!(should_publish_bootstrap(Some(old), true));
    }

    #[test]
    fn transcript_parses_entries_from_real_jsonl_fixture() {
        let fixture = concat!(
            "{\"type\":\"title\",\"v\":1,\"title\":\"Example\",\"source\":\"auto\",\"pad\":\"   \"}\n",
            "{\"type\":\"session\",\"version\":3,\"id\":\"session-id\",\"timestamp\":\"2026-10-10T17:31:45.437Z\",\"cwd\":\"/tmp/project\"}\n",
            "{\"type\":\"message\",\"id\":\"entry-1\",\"parentId\":null,\"timestamp\":\"2026-10-10T17:31:46.000Z\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
            "{\"type\":\"message\",\"id\":\"entry-2\",\"parentId\":\"entry-1\",\"timestamp\":\"2026-10-10T17:31:47.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Hi\"},{\"type\":\"toolCall\",\"id\":\"call-1\",\"name\":\"bash\",\"arguments\":{\"command\":\"pwd\"}}],\"stopReason\":\"toolUse\"}}\n",
            "{\"type\":\"custom\",\"id\":\"entry-3\",\"parentId\":\"entry-2\",\"timestamp\":\"2026-10-10T17:31:48.000Z\",\"customType\":\"tool_execution_start\",\"data\":{\"toolCallId\":\"call-1\",\"toolName\":\"bash\",\"intent\":\"pwd\"}}\n",
            "{\"type\":\"message\",\"id\":\"entry-4\",\"parentId\":\"entry-3\",\"timestamp\":\"2026-10-10T17:31:49.000Z\",\"message\":{\"role\":\"toolResult\",\"toolCallId\":\"call-1\",\"toolName\":\"bash\",\"content\":\"/tmp/project\"}}\n",
        );
        let timeline = timeline_from_jsonl(fixture.as_bytes()).expect("transcript");
        let rows = timeline
            .iter()
            .map(|activity| (activity.kind.as_str(), activity.detail.as_deref()))
            .collect::<Vec<_>>();
        // Tool results and execution markers drive status only; they are not extra rows.
        assert_eq!(
            rows,
            vec![
                ("prompt", Some("hello")),
                ("message", Some("Hi")),
                ("command", Some("pwd")),
            ]
        );
    }
    #[test]
    fn two_sessions_in_same_project_preserve_truthful_status() {
        let sess1 = ParsedSession {
            id: "sess-1".into(),
            cwd: "/work/project".into(),
            status: Some(HookEventKind::Completed),
            status_label: Some("Concluído".into()),
            stopped: true,
            ..ParsedSession::default()
        };
        let sess2 = ParsedSession {
            id: "sess-2".into(),
            cwd: "/work/project".into(),
            status: Some(HookEventKind::Running),
            status_label: Some("Rodando".into()),
            stopped: false,
            ..ParsedSession::default()
        };

        let path1 = PathBuf::from("/sessions/sess1.jsonl");
        let path2 = PathBuf::from("/sessions/sess2.jsonl");

        // When a process is bound specifically to sess-2:
        let mut inventory = OmpProcessInventory::default();
        inventory.bound_sessions.insert("sess-2".into());
        let breadcrumbs = HashSet::new();

        // sess-1 must NOT be marked live just because a process is running in /work/project!
        assert_eq!(
            check_session_liveness(&sess1, &path1, &inventory, &breadcrumbs),
            SessionLiveness::NotLive
        );
        // sess-2 is correctly recognized as live
        assert_eq!(
            check_session_liveness(&sess2, &path2, &inventory, &breadcrumbs),
            SessionLiveness::Live
        );

        // Completed sess-1 is terminal: poll_live would never fail it
        assert!(is_terminal(&sess1));
        // Running sess-2 is not terminal
        assert!(!is_terminal(&sess2));
    }

    #[test]
    fn ambiguous_running_session_retains_truthful_status_while_unbound_process_exists() {
        let sess_running = ParsedSession {
            id: "sess-running".into(),
            cwd: "/work/project".into(),
            status: Some(HookEventKind::Running),
            status_label: Some("Rodando".into()),
            stopped: false,
            ..ParsedSession::default()
        };
        let sess_completed = ParsedSession {
            id: "sess-completed".into(),
            cwd: "/work/project".into(),
            status: Some(HookEventKind::Completed),
            status_label: Some("Concluído".into()),
            stopped: true,
            ..ParsedSession::default()
        };

        let path_running = PathBuf::from("/sessions/running.jsonl");

        // An unbound process is running in /work/project
        let mut inventory = OmpProcessInventory::default();
        inventory
            .unbound_cwds
            .insert(PathBuf::from("/work/project"));
        let breadcrumbs = HashSet::new();

        // Running session is Ambiguous: should retain status, avoid false failed
        assert_eq!(
            check_session_liveness(&sess_running, &path_running, &inventory, &breadcrumbs),
            SessionLiveness::Ambiguous
        );
        // Completed session remains terminal and truthful
        assert!(is_terminal(&sess_completed));

        // When the unbound process terminates (inventory becomes empty):
        let empty_inventory = OmpProcessInventory::default();
        assert_eq!(
            check_session_liveness(&sess_running, &path_running, &empty_inventory, &breadcrumbs),
            SessionLiveness::NotLive
        );
        // Completed session still terminal
        assert!(is_terminal(&sess_completed));
    }
}
