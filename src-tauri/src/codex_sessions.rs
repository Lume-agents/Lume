use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, File},
    hash::{DefaultHasher, Hash, Hasher},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, RecvTimeoutError},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use notify::{RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::Value;
use tauri::AppHandle;

use crate::{
    context_builder,
    domain::{
        AccessMode, AgentKind, HookEvent, HookEventKind, PermissionAction, PermissionProfile,
        SessionActivity, SessionControlOrigin, SessionSource,
    },
    event_server, protocol,
    state::{now_millis, AppState},
};

const RECOVERY_INTERVAL: Duration = Duration::from_secs(2);
const BOOTSTRAP_LOOKBACK: Duration = Duration::from_secs(5 * 60);
// Keep enough recent history to recover long-running GOAL/TODO metadata without
// ever loading an entire (potentially gigabyte-sized) rollout into memory.
const BOOTSTRAP_TAIL_BYTES: u64 = 16 * 1024 * 1024;
static SUBAGENT_PATH_CACHE: OnceLock<Mutex<HashMap<(PathBuf, String), PathBuf>>> = OnceLock::new();
static SUBAGENT_MISS_CACHE: OnceLock<Mutex<HashMap<(PathBuf, String), Instant>>> = OnceLock::new();

#[derive(Clone, Debug)]
struct SessionMetadata {
    id: String,
    cwd: Option<String>,
    started_at: Option<String>,
    source: SessionSource,
}

#[derive(Debug)]
struct ObservedFile {
    offset: u64,
    session: Option<SessionMetadata>,
    profile: Option<PermissionProfile>,
    pending_tools: HashMap<String, PendingTool>,
    token_totals: Option<TokenTotals>,
    prompt_token_start: Option<TokenTotals>,
    prompt_started_at: Option<i64>,
}

#[derive(Clone, Copy, Debug)]
struct TokenTotals {
    total: u64,
    input: u64,
    output: u64,
}

#[derive(Debug)]
struct PendingTool {
    name: String,
    activity_id: String,
    kind: String,
    title: String,
    detail: Option<String>,
    files: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct CodexRecord {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: RecordPayload,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct RecordPayload {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    originator: Option<String>,
    #[serde(default)]
    source: Option<Value>,
    #[serde(default)]
    parent_thread_id: Option<String>,
    #[serde(default)]
    thread_source: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    approval_policy: Option<String>,
    #[serde(default)]
    approvals_reviewer: Option<String>,
    #[serde(default)]
    sandbox_policy: Option<Value>,
    #[serde(default)]
    last_agent_message: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    output: Option<Value>,
    #[serde(default)]
    arguments: Option<String>,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default)]
    summary: Option<Value>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    command: Option<Value>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    aggregated_output: Option<String>,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    stderr: Option<String>,
    #[serde(default)]
    changes: Option<Value>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    agent_thread_id: Option<String>,
    #[serde(default)]
    agent_path: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    occurred_at_ms: Option<i64>,
    #[serde(default)]
    item: Option<Value>,
    #[serde(default)]
    info: Option<Value>,
}

pub fn start(state: AppState, app: AppHandle) -> Result<(), String> {
    thread::Builder::new()
        .name("lume-codex-session-monitor".into())
        .spawn(move || monitor(state, app))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn monitor(state: AppState, app: AppHandle) {
    let Some(root) = sessions_root() else {
        return;
    };
    let mut observed = initialize(&root, &state, &app);
    loop {
        if watch_session_files(&root, &state, &app, &mut observed).is_ok() {
            return;
        }
        poll(&root, &state, &app, &mut observed);
        thread::sleep(RECOVERY_INTERVAL);
    }
}

fn watch_session_files(
    root: &Path,
    state: &AppState,
    app: &AppHandle,
    observed: &mut HashMap<PathBuf, ObservedFile>,
) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = sender.send(event);
    })
    .map_err(|error| error.to_string())?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|error| error.to_string())?;

    loop {
        match receiver.recv_timeout(RECOVERY_INTERVAL) {
            Ok(Ok(event)) => {
                for path in event.paths {
                    if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
                        poll_path(&path, state, app, observed);
                    }
                }
            }
            Ok(Err(error)) => return Err(error.to_string()),
            Err(RecvTimeoutError::Timeout) => {
                poll(root, state, app, observed);
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err("Monitor de sessões do Codex desconectado".into());
            }
        }
    }
}

fn initialize(root: &Path, state: &AppState, app: &AppHandle) -> HashMap<PathBuf, ObservedFile> {
    let mut observed = HashMap::new();
    for path in session_files(root) {
        let Ok(file_metadata) = fs::metadata(&path) else {
            continue;
        };
        let mut file = ObservedFile {
            offset: file_metadata.len(),
            session: read_session_metadata(&path),
            profile: None,
            pending_tools: HashMap::new(),
            token_totals: None,
            prompt_token_start: None,
            prompt_started_at: None,
        };
        if was_modified_recently(&file_metadata) {
            let (running, recovered_events) = bootstrap_session(&path, &mut file);
            if running {
                if let Some(event) = event_for(&file, HookEventKind::Running, "Rodando", None) {
                    let _ = event_server::publish_event(state, app, event);
                }
            }
            for event in recovered_events {
                let _ = event_server::publish_event(state, app, event);
            }
        }
        observed.insert(path, file);
    }
    observed
}

fn was_modified_recently(metadata: &fs::Metadata) -> bool {
    metadata
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age <= BOOTSTRAP_LOOKBACK)
}

fn bootstrap_session(path: &Path, file: &mut ObservedFile) -> (bool, Vec<HookEvent>) {
    let Ok(records) = read_tail_records(path, BOOTSTRAP_TAIL_BYTES) else {
        return (false, Vec::new());
    };
    let mut running = false;
    for record in &records {
        if record.kind == "turn_context" {
            if let Some(session) = file.session.as_mut() {
                if record.payload.cwd.is_some() {
                    session.cwd = record.payload.cwd.clone();
                }
                file.profile = Some(profile_from_context(&record.payload));
            }
            continue;
        }
        if record.kind != "event_msg" {
            continue;
        }
        match record.payload.r#type.as_deref() {
            Some("task_started") => running = true,
            Some("task_complete" | "turn_aborted" | "stream_error" | "task_failed") => {
                running = false;
            }
            _ => {}
        }
    }
    let recovery_records = records
        .into_iter()
        .filter(|record| {
            (record.kind == "event_msg"
                && (matches!(
                    record.payload.r#type.as_deref(),
                    Some(
                        "sub_agent_activity"
                            | "agent_message"
                            | "token_count"
                            | "task_started"
                            | "task_complete"
                            | "turn_aborted"
                            | "stream_error"
                            | "task_failed"
                    )
                ) && (record.payload.r#type.as_deref() != Some("agent_message")
                    || record
                        .payload
                        .message
                        .as_deref()
                        .is_some_and(protocol::is_work_tracking_message))
                    || matches!(
                        record.payload.r#type.as_deref(),
                        Some("item_started" | "item_completed")
                    ) && record.payload.item.as_ref().is_some_and(|item| {
                        matches!(
                            item.get("type").and_then(Value::as_str),
                            Some("SubAgentActivity" | "subAgentActivity")
                        )
                    })))
                || (record.kind == "response_item"
                    && (matches!(
                        record.payload.r#type.as_deref(),
                        Some("function_call_output" | "custom_tool_call_output")
                    ) || matches!(
                        record.payload.r#type.as_deref(),
                        Some("function_call" | "custom_tool_call")
                    ) && is_work_tracking_tool(&record.payload)))
        })
        .collect();
    let mut recovered = events_from_records(recovery_records, file);
    recovered.retain(|event| {
        event.activity.as_ref().is_some_and(|activity| {
            activity.kind == "subagent"
                || activity.kind == "token_usage"
                || activity.kind == "plan"
                || (activity.kind == "message"
                    && activity
                        .detail
                        .as_deref()
                        .is_some_and(protocol::is_work_tracking_message))
                || (activity.kind == "tool"
                    && ["create_goal", "get_goal", "update_goal", "todo_write"]
                        .iter()
                        .any(|tool| activity.title.contains(tool)))
        })
    });
    let mut seen = HashSet::new();
    let recent_ids = recovered
        .iter()
        .rev()
        .filter_map(|event| event.activity.as_ref())
        .filter(|activity| seen.insert(activity.id.clone()))
        .take(96)
        .map(|activity| activity.id.clone())
        .collect::<HashSet<_>>();
    recovered.retain(|event| {
        event
            .activity
            .as_ref()
            .is_some_and(|activity| recent_ids.contains(&activity.id))
    });
    (running, recovered)
}

fn read_tail_records(path: &Path, max_bytes: u64) -> Result<Vec<CodexRecord>, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    let start = length.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::with_capacity(max_bytes.min(8 * 1024 * 1024) as usize);
    file.take(max_bytes)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let records_start = if start > 0 {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |index| index + 1)
    } else {
        0
    };
    let mut records = Vec::new();
    for line in bytes[records_start..].split(|byte| *byte == b'\n') {
        if let Ok(record) = serde_json::from_slice(line) {
            records.push(record);
        }
    }
    Ok(records)
}

fn poll(
    root: &Path,
    state: &AppState,
    app: &AppHandle,
    observed: &mut HashMap<PathBuf, ObservedFile>,
) {
    for path in session_files(root) {
        poll_path(&path, state, app, observed);
    }
}

fn poll_path(
    path: &Path,
    state: &AppState,
    app: &AppHandle,
    observed: &mut HashMap<PathBuf, ObservedFile>,
) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    let length = metadata.len();
    if !observed.contains_key(path) {
        let mut file = ObservedFile {
            offset: 0,
            session: read_session_metadata(path),
            profile: None,
            pending_tools: HashMap::new(),
            token_totals: None,
            prompt_token_start: None,
            prompt_started_at: None,
        };
        if let Some(event) = session_started_event(&file) {
            let _ = event_server::publish_event(state, app, event);
        }
        publish_appended_events(path, state, app, &mut file);
        if file.offset == 0 {
            file.offset = length;
        }
        observed.insert(path.to_path_buf(), file);
        return;
    }

    let file = observed.get_mut(path).expect("verificado acima");
    if length < file.offset {
        file.offset = 0;
        file.profile = None;
        file.pending_tools.clear();
        file.token_totals = None;
        file.prompt_token_start = None;
        file.prompt_started_at = None;
        file.session = read_session_metadata(path);
        if let Some(event) = session_started_event(file) {
            let _ = event_server::publish_event(state, app, event);
        }
    }
    if file.session.is_none() && length > file.offset {
        file.session = read_session_metadata(path);
        if file.session.is_some() {
            file.offset = 0;
            if let Some(event) = session_started_event(file) {
                let _ = event_server::publish_event(state, app, event);
            }
        }
    }
    if length > file.offset {
        publish_appended_events(path, state, app, file);
    }
}

fn publish_appended_events(
    path: &Path,
    state: &AppState,
    app: &AppHandle,
    file: &mut ObservedFile,
) {
    if file.session.is_none() {
        file.offset = fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(file.offset);
        return;
    }
    let Ok((records, offset)) = read_records(path, file.offset) else {
        return;
    };
    for event in events_from_records(records, file) {
        let _ = event_server::publish_event(state, app, event);
    }
    file.offset = offset;
}

fn events_from_records(records: Vec<CodexRecord>, file: &mut ObservedFile) -> Vec<HookEvent> {
    let mut events = Vec::new();
    for record in records {
        if record.kind == "response_item" {
            match record.payload.r#type.as_deref() {
                Some("function_call" | "custom_tool_call") => {
                    if let Some(event) = remember_tool(&record.payload, file) {
                        events.push(event);
                    }
                }
                Some("function_call_output" | "custom_tool_call_output") => {
                    if let Some(event) = tool_output_event(&record.payload, file) {
                        events.push(event);
                    }
                }
                Some("reasoning") => {
                    if let Some(detail) = reasoning_summary(&record.payload) {
                        if let Some(event) = activity_event_for(
                            file,
                            "analysis",
                            "Análise",
                            &detail,
                            record.timestamp.as_deref(),
                        ) {
                            events.push(event);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if record.kind == "turn_context" {
            if let Some(session) = file.session.as_mut() {
                if record.payload.cwd.is_some() {
                    session.cwd = record.payload.cwd.clone();
                }
                file.profile = Some(profile_from_context(&record.payload));
            }
            continue;
        }
        if record.kind != "event_msg" {
            continue;
        }
        if record.payload.r#type.as_deref() == Some("token_count") {
            if let Some(totals) = token_totals(&record.payload) {
                file.token_totals = Some(totals);
            }
            continue;
        }
        if record.payload.r#type.as_deref() == Some("task_started") {
            file.prompt_token_start = file.token_totals;
            file.prompt_started_at = Some(record_timestamp_millis(&record));
        } else if record.payload.r#type.as_deref() == Some("task_complete") {
            if let Some(event) =
                completed_prompt_token_usage_event(file, record_timestamp_millis(&record))
            {
                events.push(event);
            }
            file.prompt_token_start = None;
            file.prompt_started_at = None;
        } else if matches!(
            record.payload.r#type.as_deref(),
            Some("turn_aborted" | "stream_error" | "task_failed")
        ) {
            file.prompt_token_start = None;
            file.prompt_started_at = None;
        }
        if record.payload.r#type.as_deref() == Some("sub_agent_activity") {
            if let Some(event) = subagent_activity_event(
                file,
                record.payload.agent_thread_id.as_deref(),
                record.payload.agent_path.as_deref(),
                record.payload.kind.as_deref(),
                record.payload.occurred_at_ms,
            ) {
                events.push(event);
            }
            continue;
        }
        if matches!(
            record.payload.r#type.as_deref(),
            Some("item_started" | "item_completed")
        ) {
            if let Some(item) = record.payload.item.as_ref().filter(|item| {
                matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("SubAgentActivity" | "subAgentActivity")
                )
            }) {
                if let Some(event) = subagent_activity_event(
                    file,
                    item.get("agent_thread_id")
                        .or_else(|| item.get("agentThreadId"))
                        .and_then(Value::as_str),
                    item.get("agent_path")
                        .or_else(|| item.get("agentPath"))
                        .and_then(Value::as_str),
                    item.get("kind").and_then(Value::as_str),
                    record.payload.occurred_at_ms,
                ) {
                    events.push(event);
                }
                continue;
            }
        }
        if record.payload.r#type.as_deref() == Some("exec_command_end") {
            if let Some(event) = command_finished_event(&record.payload, file) {
                events.push(event);
            }
            continue;
        }
        if record.payload.r#type.as_deref() == Some("patch_apply_end") {
            if let Some(event) = patch_finished_event(&record.payload, file) {
                events.push(event);
            }
            continue;
        }
        if matches!(
            record.payload.r#type.as_deref(),
            Some("user_message" | "agent_message")
        ) {
            if let Some(message) = record
                .payload
                .message
                .as_deref()
                .map(str::trim)
                .filter(|message| !message.is_empty())
            {
                let (kind, title) = if record.payload.r#type.as_deref() == Some("user_message") {
                    ("prompt", "Prompt enviado")
                } else {
                    ("message", "Resposta do agente")
                };
                if let Some(event) =
                    activity_event_for(file, kind, title, message, record.timestamp.as_deref())
                {
                    events.push(event);
                }
            }
            continue;
        }
        let (kind, label, last_response) = match record.payload.r#type.as_deref() {
            Some("task_started") => (HookEventKind::Running, "Rodando", None),
            Some("task_complete") => (
                HookEventKind::Completed,
                "Tarefa finalizada",
                record.payload.last_agent_message.as_deref(),
            ),
            Some("turn_aborted") => (HookEventKind::WaitingForInput, "Tarefa interrompida", None),
            Some("stream_error" | "task_failed") => {
                (HookEventKind::Failed, "Tarefa encerrada com erro", None)
            }
            _ => continue,
        };
        if let Some(event) = event_for(file, kind, label, last_response) {
            events.push(event);
        }
    }
    events
}

fn token_totals(payload: &RecordPayload) -> Option<TokenTotals> {
    let usage = payload.info.as_ref()?.get("total_token_usage")?;
    Some(TokenTotals {
        total: usage.get("total_tokens")?.as_u64()?,
        input: usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        output: usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
    })
}

fn record_timestamp_millis(record: &CodexRecord) -> i64 {
    record
        .timestamp
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis())
        .unwrap_or_else(now_millis)
}

fn completed_prompt_token_usage_event(file: &ObservedFile, completed_at: i64) -> Option<HookEvent> {
    let start = file.prompt_token_start?;
    let end = file.token_totals?;
    let total = end.total.saturating_sub(start.total);
    if total == 0 {
        return None;
    }
    let started_at = file.prompt_started_at.unwrap_or(completed_at);
    let turn_id = format!("rollout-{started_at}");
    let detail = serde_json::json!({
        "turnId": turn_id,
        "totalTokens": total,
        "inputTokens": end.input.saturating_sub(start.input),
        "outputTokens": end.output.saturating_sub(start.output),
        "createdAt": completed_at,
    })
    .to_string();
    let mut event = event_for(file, HookEventKind::Activity, "Uso de tokens", None)?;
    event.activity = Some(SessionActivity {
        id: format!("codex-token-usage:{turn_id}"),
        kind: "token_usage".into(),
        title: "Uso de tokens".into(),
        detail: Some(detail),
        status: "completed".into(),
        created_at: completed_at,
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    });
    Some(event)
}

fn subagent_activity_event(
    file: &ObservedFile,
    agent_thread_id: Option<&str>,
    agent_path: Option<&str>,
    kind: Option<&str>,
    occurred_at_ms: Option<i64>,
) -> Option<HookEvent> {
    let session = file.session.as_ref()?;
    let agent_thread_id = agent_thread_id.filter(|id| !id.is_empty())?;
    let status = match kind? {
        "started" | "interacted" => "running",
        "completed" => "completed",
        "failed" | "errored" => "failed",
        "interrupted" | "shutdown" | "closed" => "interrupted",
        _ => return None,
    };
    let label = agent_path
        .filter(|path| !path.is_empty())
        .unwrap_or(agent_thread_id);
    let mut event = event_for(file, HookEventKind::Activity, "Subagente", None)?;
    event.activity = Some(SessionActivity {
        id: format!("codex:{}:subagent:{agent_thread_id}", session.id),
        kind: "subagent".into(),
        title: format!("Subagente · {label}"),
        detail: None,
        status: status.into(),
        created_at: occurred_at_ms.unwrap_or_else(now_millis),
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    });
    Some(event)
}

fn remember_tool(payload: &RecordPayload, file: &mut ObservedFile) -> Option<HookEvent> {
    let original_name = payload.name.as_deref()?;
    let call_id = payload.call_id.as_ref()?;
    let original_detail = tool_input_text(payload);
    let nested = (normalized_tool_name(original_name) == "exec")
        .then(|| {
            original_detail
                .as_deref()
                .and_then(nested_work_tracking_tool)
        })
        .flatten();
    let (name, detail) = nested
        .map(|(name, arguments)| (name.to_string(), response_text(&arguments)))
        .unwrap_or_else(|| (original_name.to_string(), original_detail));
    let kind = tool_kind(&name);
    let title = tool_title(&name);
    let files = detail
        .as_deref()
        .map(files_from_patch_text)
        .unwrap_or_default();
    let activity_id = payload
        .id
        .as_ref()
        .and_then(|id| {
            file.session
                .as_ref()
                .map(|session| format!("codex:{}:{id}", session.id))
        })
        .unwrap_or_else(|| format!("codex-rollout-tool:{call_id}"));
    file.pending_tools.insert(
        call_id.clone(),
        PendingTool {
            name: name.clone(),
            activity_id: activity_id.clone(),
            kind: kind.into(),
            title: title.clone(),
            detail: detail.clone(),
            files: files.clone(),
        },
    );
    if is_goal_tool(&name) {
        return None;
    }
    let mut event = event_for(file, HookEventKind::Activity, &title, None)?;
    event.activity = Some(SessionActivity {
        id: activity_id,
        kind: kind.into(),
        title,
        detail,
        status: "running".into(),
        created_at: now_millis(),
        files,
        attachments: Vec::new(),
        append_detail: false,
    });
    Some(event)
}

fn tool_output_event(payload: &RecordPayload, file: &mut ObservedFile) -> Option<HookEvent> {
    let tool = file.pending_tools.remove(payload.call_id.as_deref()?)?;
    let output = payload.output.as_ref().and_then(|value| {
        if is_goal_tool(&tool.name) {
            structured_goal_output_text(value)
        } else {
            record_value_text(value)
        }
    });
    let detail = if is_goal_tool(&tool.name) {
        output.or(tool.detail)
    } else if matches!(
        normalized_tool_name(&tool.name),
        "update_plan" | "todo_write"
    ) {
        tool.detail.or(output)
    } else {
        combine_activity_detail(tool.detail.as_deref(), output.as_deref())
    };
    let label = if is_goal_tool(&tool.name) {
        "GOAL atualizada"
    } else {
        &tool.title
    };
    let mut event = event_for(file, HookEventKind::Activity, label, None)?;
    event.activity = Some(SessionActivity {
        id: tool.activity_id,
        kind: tool.kind,
        title: if is_goal_tool(&tool.name) {
            format!("functions · {}", normalized_tool_name(&tool.name))
        } else {
            tool.title
        },
        detail,
        status: "completed".into(),
        created_at: now_millis(),
        files: tool.files,
        attachments: Vec::new(),
        append_detail: false,
    });
    Some(event)
}

fn command_finished_event(payload: &RecordPayload, file: &mut ObservedFile) -> Option<HookEvent> {
    let call_id = payload.call_id.as_deref()?;
    let tool = file.pending_tools.remove(call_id)?;
    let command = payload
        .command
        .as_ref()
        .and_then(command_value_text)
        .or(tool.detail);
    let output = payload
        .aggregated_output
        .as_deref()
        .and_then(response_text)
        .or_else(|| command_output_text(payload));
    let detail = combine_activity_detail(command.as_deref(), output.as_deref());
    let failed = payload.status.as_deref() == Some("failed")
        || payload.exit_code.is_some_and(|exit_code| exit_code != 0);
    let mut event = event_for(file, HookEventKind::Activity, "Comando", None)?;
    event.activity = Some(SessionActivity {
        id: tool.activity_id,
        kind: "command".into(),
        title: "Comando".into(),
        detail,
        status: if failed { "failed" } else { "completed" }.into(),
        created_at: now_millis(),
        files: tool.files,
        attachments: Vec::new(),
        append_detail: false,
    });
    Some(event)
}

fn patch_finished_event(payload: &RecordPayload, file: &mut ObservedFile) -> Option<HookEvent> {
    let changes = payload.changes.as_ref()?.as_object()?;
    if changes.is_empty() {
        return None;
    }
    let files = changes.keys().cloned().collect::<Vec<_>>();
    let mut diffs = Vec::new();
    for (path, change) in changes {
        if let Some(diff) = change.get("unified_diff").and_then(Value::as_str) {
            diffs.push(format!("*** Update File: {path}\n{diff}"));
        }
    }
    let detail = (!diffs.is_empty()).then(|| diffs.join("\n"));
    let session_id = file.session.as_ref()?.id.clone();
    let call_id = payload.call_id.as_deref().unwrap_or("patch");
    let pending = file.pending_tools.remove(call_id);
    let activity_id = pending
        .map(|tool| tool.activity_id)
        .unwrap_or_else(|| format!("codex:{session_id}:patch:{call_id}"));
    let failed = payload.success == Some(false) || payload.status.as_deref() == Some("failed");
    let mut event = event_for(file, HookEventKind::Activity, "Arquivos alterados", None)?;
    event.activity = Some(SessionActivity {
        id: activity_id,
        kind: "file".into(),
        title: "Arquivos alterados".into(),
        detail,
        status: if failed { "failed" } else { "completed" }.into(),
        created_at: now_millis(),
        files,
        attachments: Vec::new(),
        append_detail: false,
    });
    Some(event)
}

fn tool_kind(name: &str) -> &'static str {
    let name = normalized_tool_name(name);
    if matches!(name, "exec" | "exec_command" | "shell" | "terminal") {
        "command"
    } else if name == "apply_patch" {
        "file"
    } else if name == "update_plan" {
        "plan"
    } else {
        "tool"
    }
}

fn tool_title(name: &str) -> String {
    match normalized_tool_name(name) {
        "exec" | "exec_command" | "shell" | "terminal" => "Comando".into(),
        "apply_patch" => "Alteração de arquivo".into(),
        "update_plan" => "Plano atualizado".into(),
        "view_image" => "Imagem inspecionada".into(),
        "wait" => "Aguardando comando".into(),
        name => format!("functions · {name}"),
    }
}

fn normalized_tool_name(name: &str) -> &str {
    name.rsplit(['.', ':', '/']).next().unwrap_or(name)
}

fn tool_input_text(payload: &RecordPayload) -> Option<String> {
    payload
        .arguments
        .as_deref()
        .and_then(|arguments| {
            serde_json::from_str::<Value>(arguments)
                .ok()
                .as_ref()
                .and_then(tool_input_value_text)
                .or_else(|| response_text(arguments))
        })
        .or_else(|| payload.input.as_ref().and_then(tool_input_value_text))
}

fn tool_input_value_text(value: &Value) -> Option<String> {
    value
        .get("cmd")
        .and_then(Value::as_str)
        .and_then(response_text)
        .or_else(|| {
            value
                .get("patch")
                .and_then(Value::as_str)
                .and_then(response_text)
        })
        .or_else(|| {
            value
                .get("source")
                .and_then(Value::as_str)
                .and_then(response_text)
        })
        .or_else(|| value.as_str().and_then(response_text))
        .or_else(|| record_value_text(value))
}

fn command_value_text(value: &Value) -> Option<String> {
    if let Some(parts) = value.as_array() {
        let command = parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        return response_text(&command);
    }
    record_value_text(value)
}

fn command_output_text(payload: &RecordPayload) -> Option<String> {
    let output = [payload.stdout.as_deref(), payload.stderr.as_deref()]
        .into_iter()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    response_text(&output)
}

fn combine_activity_detail(input: Option<&str>, output: Option<&str>) -> Option<String> {
    match (input, output) {
        (Some(input), Some(output)) if input.trim() != output.trim() => {
            response_text(&format!("{input}\n\n{output}"))
        }
        (Some(input), _) => response_text(input),
        (_, Some(output)) => response_text(output),
        _ => None,
    }
}

fn reasoning_summary(payload: &RecordPayload) -> Option<String> {
    payload
        .summary
        .as_ref()
        .and_then(value_text)
        .or_else(|| payload.content.as_ref().and_then(value_text))
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => response_text(value),
        Value::Array(values) => {
            let text = values
                .iter()
                .filter_map(value_text)
                .collect::<Vec<_>>()
                .join("\n");
            response_text(&text)
        }
        Value::Object(object) => object
            .get("text")
            .and_then(value_text)
            .or_else(|| object.get("summary_text").and_then(value_text)),
        _ => None,
    }
}

fn files_from_patch_text(value: &str) -> Vec<String> {
    let mut files = Vec::new();
    // Code-mode tool calls often carry a patch inside a JS string using literal
    // `\\n` escapes; normalize those without inspecting unrelated tool output.
    let normalized = value.replace("\\\\n", "\n").replace("\\n", "\n");
    for line in normalized.lines() {
        let line = line.trim_start();
        let path = ["*** Add File: ", "*** Update File: ", "*** Delete File: "]
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix));
        if let Some(path) = path.map(str::trim).filter(|path| !path.is_empty()) {
            if !files.iter().any(|existing| existing == path) {
                files.push(path.into());
            }
        }
    }
    files
}

fn is_goal_tool(name: &str) -> bool {
    matches!(
        normalized_tool_name(name),
        "create_goal" | "get_goal" | "update_goal"
    )
}

fn is_work_tracking_tool(payload: &RecordPayload) -> bool {
    let Some(name) = payload.name.as_deref() else {
        return false;
    };
    let normalized = normalized_tool_name(name);
    matches!(normalized, "update_plan" | "todo_write")
        || is_goal_tool(normalized)
        || (normalized == "exec"
            && tool_input_text(payload)
                .as_deref()
                .and_then(nested_work_tracking_tool)
                .is_some())
}

pub(crate) fn nested_work_tracking_tool(source: &str) -> Option<(&'static str, String)> {
    const TOOLS: [&str; 5] = [
        "create_goal",
        "get_goal",
        "update_goal",
        "update_plan",
        "todo_write",
    ];
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;

    while index < bytes.len() {
        let current = bytes[index];
        let next = bytes.get(index + 1).copied();
        if line_comment {
            if current == b'\n' {
                line_comment = false;
            }
            index += 1;
            continue;
        }
        if block_comment {
            if current == b'*' && next == Some(b'/') {
                block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if current == b'\\' {
                escaped = true;
            } else if current == active_quote {
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(current, b'\'' | b'"' | b'`') {
            quote = Some(current);
            index += 1;
            continue;
        }
        if current == b'/' && next == Some(b'/') {
            line_comment = true;
            index += 2;
            continue;
        }
        if current == b'/' && next == Some(b'*') {
            block_comment = true;
            index += 2;
            continue;
        }
        if source[index..].starts_with("tools.") {
            let name_start = index + "tools.".len();
            for tool in TOOLS {
                let name_end = name_start + tool.len();
                if source.get(name_start..name_end) != Some(tool) {
                    continue;
                }
                let mut open = name_end;
                while bytes.get(open).is_some_and(u8::is_ascii_whitespace) {
                    open += 1;
                }
                if bytes.get(open) == Some(&b'(') {
                    return extract_javascript_call_argument(source, open)
                        .map(|arguments| (tool, arguments));
                }
            }
        }
        index += 1;
    }
    None
}

fn extract_javascript_call_argument(source: &str, open: usize) -> Option<String> {
    let bytes = source.as_bytes();
    let mut depth = 1usize;
    let mut index = open + 1;
    let mut quote = None;
    let mut escaped = false;

    while index < bytes.len() {
        let current = bytes[index];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if current == b'\\' {
                escaped = true;
            } else if current == active_quote {
                quote = None;
            }
            index += 1;
            continue;
        }
        match current {
            b'\'' | b'"' | b'`' => quote = Some(current),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(source[open + 1..index].trim().to_string());
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn record_value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => response_text(value),
        Value::Null => None,
        value => response_text(&value.to_string()),
    }
}

fn structured_goal_output_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|_| response_text(text)),
        Value::Array(values) => values.iter().find_map(structured_goal_output_text),
        Value::Object(object) => {
            if let Some(text) = object.get("text") {
                structured_goal_output_text(text)
            } else {
                response_text(&value.to_string())
            }
        }
        _ => None,
    }
}

fn activity_event_for(
    file: &ObservedFile,
    kind: &str,
    title: &str,
    detail: &str,
    timestamp: Option<&str>,
) -> Option<HookEvent> {
    let session = file.session.as_ref()?;
    let mut hasher = DefaultHasher::new();
    session.id.hash(&mut hasher);
    kind.hash(&mut hasher);
    detail.hash(&mut hasher);
    timestamp.hash(&mut hasher);
    let mut event = event_for(file, HookEventKind::Activity, title, None)?;
    event.activity = Some(SessionActivity {
        id: format!("codex-rollout:{:x}", hasher.finish()),
        kind: kind.into(),
        title: title.into(),
        detail: response_text(detail),
        status: "completed".into(),
        created_at: now_millis(),
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    });
    Some(event)
}

fn session_started_event(file: &ObservedFile) -> Option<HookEvent> {
    event_for(file, HookEventKind::SessionStarted, "Esperando ação", None)
}

fn event_for(
    file: &ObservedFile,
    event: HookEventKind,
    label: &str,
    last_response: Option<&str>,
) -> Option<HookEvent> {
    let session = file.session.as_ref()?;
    let project = session
        .cwd
        .as_deref()
        .and_then(|cwd| Path::new(cwd).file_name())
        .and_then(|name| name.to_str())
        .map(str::to_string);
    Some(HookEvent {
        event,
        session_id: format!("codex-app-server:{}", session.id),
        agent: AgentKind::Codex,
        agent_label: Some("Codex".into()),
        session_name: None,
        project,
        source: Some(session.source.clone()),
        source_app: None,
        control_origin: SessionControlOrigin::External,
        status_label: Some(label.into()),
        started_at: session.started_at.clone(),
        process_id: None,
        native_session_id: Some(session.id.clone()),
        working_directory: session.cwd.clone(),
        permission_profile: file.profile.clone(),
        permission: None,
        question: None,
        last_response: last_response.and_then(response_text),
        activity: None,
        activities: Vec::new(),
        wait_for_decision: false,
    })
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

fn read_session_metadata(path: &Path) -> Option<SessionMetadata> {
    let file = File::open(path).ok()?;
    let record = serde_json::Deserializer::from_reader(BufReader::new(file))
        .into_iter::<CodexRecord>()
        .next()?
        .ok()?;
    session_metadata(&record)
}

fn session_metadata(record: &CodexRecord) -> Option<SessionMetadata> {
    if record.kind != "session_meta"
        || record.payload.originator.as_deref() == Some("lume-diagnostic")
        || record.payload.parent_thread_id.is_some()
        || record.payload.thread_source.as_deref() == Some("subagent")
        || record
            .payload
            .cwd
            .as_deref()
            .is_some_and(crate::session_filters::is_codex_internal_workspace)
        || record
            .payload
            .source
            .as_ref()
            .and_then(|source| source.get("subagent"))
            .is_some()
    {
        return None;
    }
    let source = match (
        record.payload.originator.as_deref(),
        record.payload.source.as_ref().and_then(Value::as_str),
    ) {
        (Some("codex_vscode"), _) | (_, Some("vscode")) => SessionSource::Vscode,
        (Some("codex-tui" | "codex_cli_rs"), _) | (_, Some("cli")) => SessionSource::Cli,
        _ => return None,
    };
    Some(SessionMetadata {
        id: record.payload.id.clone()?,
        cwd: record.payload.cwd.clone(),
        started_at: record.payload.timestamp.clone(),
        source,
    })
}

fn read_records(path: &Path, start: u64) -> Result<(Vec<CodexRecord>, u64), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(start))
        .map_err(|error| error.to_string())?;
    let mut stream =
        serde_json::Deserializer::from_reader(BufReader::new(file)).into_iter::<CodexRecord>();
    let mut records = Vec::new();
    while let Some(record) = stream.next() {
        match record {
            Ok(record) => records.push(record),
            Err(error) if error.is_eof() => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok((records, start + stream.byte_offset() as u64))
}

fn profile_from_context(payload: &RecordPayload) -> PermissionProfile {
    let sandbox = payload
        .sandbox_policy
        .as_ref()
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("custom");
    let (mode, label) = match sandbox {
        "danger-full-access" => (AccessMode::FullAccess, "Acesso total"),
        "workspace-write" => (AccessMode::WorkspaceWrite, "Edições no projeto"),
        "read-only" => (AccessMode::ReadOnly, "Somente leitura"),
        "plan" => (AccessMode::Plan, "Modo de planejamento"),
        _ => (AccessMode::Custom, "Permissões da sessão"),
    };
    PermissionProfile {
        mode,
        label: label.into(),
        approval_policy: payload
            .approval_policy
            .clone()
            .unwrap_or_else(|| "Gerenciada na origem".into()),
        approvals_reviewer: payload.approvals_reviewer.clone(),
        can_respond_from_lume: false,
        available_actions: vec![PermissionAction::OpenSource],
    }
}

fn sessions_root() -> Option<PathBuf> {
    let codex_home = env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| {
        env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .map(|home| home.join(".codex"))
    })?;
    Some(codex_home.join("sessions"))
}

pub fn load_subagent_timeline(
    parent_thread_id: &str,
    child_thread_id: &str,
) -> Result<Vec<SessionActivity>, String> {
    let root =
        sessions_root().ok_or_else(|| "Codex sessions directory is unavailable".to_string())?;
    load_subagent_timeline_from_root(&root, parent_thread_id, child_thread_id)
}

fn load_subagent_timeline_from_root(
    root: &Path,
    parent_thread_id: &str,
    child_thread_id: &str,
) -> Result<Vec<SessionActivity>, String> {
    let bytes = child_thread_id.as_bytes();
    if bytes.len() != 36
        || !bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return Err("Invalid subagent thread ID".into());
    }
    let cache = SUBAGENT_PATH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let miss_cache = SUBAGENT_MISS_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache_key = (root.to_path_buf(), child_thread_id.to_string());
    let cached = cache
        .lock()
        .ok()
        .and_then(|paths| paths.get(&cache_key).cloned())
        .filter(|path| path.is_file());
    let path = if let Some(path) = cached {
        path
    } else {
        if miss_cache
            .lock()
            .ok()
            .and_then(|misses| misses.get(&cache_key).copied())
            .is_some_and(|checked| checked.elapsed() < Duration::from_secs(12))
        {
            return Err("Subagent timeline is not available yet".into());
        }
        let suffix = format!("-{child_thread_id}.jsonl");
        let Some(path) = session_files(root).into_iter().find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(&suffix))
        }) else {
            if let Ok(mut misses) = miss_cache.lock() {
                if misses.len() >= 128 {
                    misses.clear();
                }
                misses.insert(cache_key, Instant::now());
            }
            return Err("Subagent timeline is not available yet".into());
        };
        if let Ok(mut misses) = miss_cache.lock() {
            misses.remove(&cache_key);
        }
        if let Ok(mut paths) = cache.lock() {
            if paths.len() >= 128 {
                paths.clear();
            }
            paths.insert(cache_key, path.clone());
        }
        path
    };
    let first = BufReader::new(File::open(&path).map_err(|error| error.to_string())?)
        .lines()
        .next()
        .transpose()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Subagent session is empty".to_string())?;
    let metadata: CodexRecord = serde_json::from_str(&first).map_err(|error| error.to_string())?;
    if metadata.kind != "session_meta"
        || metadata.payload.id.as_deref() != Some(child_thread_id)
        || metadata.payload.parent_thread_id.as_deref() != Some(parent_thread_id)
    {
        return Err("Subagent does not belong to this thread".into());
    }

    // Subagent rollouts may inherit a very large parent history. Read only the
    // recent tail, then skip any inherited records before the branch marker.
    let records = read_tail_records(&path, 384 * 1024)?;
    let start = records
        .iter()
        .rposition(|record| record.kind == "inter_agent_communication_metadata")
        .map_or(0, |index| index + 1);
    let working_directory = metadata.payload.cwd.clone();
    let mut observed = ObservedFile {
        offset: 0,
        session: Some(SessionMetadata {
            id: child_thread_id.to_string(),
            cwd: working_directory.clone(),
            started_at: None,
            source: SessionSource::Cli,
        }),
        profile: None,
        pending_tools: HashMap::new(),
        token_totals: None,
        prompt_token_start: None,
        prompt_started_at: None,
    };
    let mut activities: Vec<SessionActivity> = Vec::new();
    let mut indices: HashMap<String, usize> = HashMap::new();
    let mut latest_turn_started = i64::MIN;
    let mut latest_public_message: Option<String> = None;
    for record in records.into_iter().skip(start) {
        let created_at = record
            .timestamp
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.timestamp_millis())
            .unwrap_or_else(now_millis);
        let event_type = record.payload.r#type.as_deref();
        if record.kind == "event_msg" && event_type == Some("task_started") {
            latest_turn_started = created_at;
            latest_public_message = None;
        }
        if record.kind == "event_msg" && event_type == Some("agent_message") {
            latest_public_message = record.payload.message.clone();
        }
        let completed_response =
            if record.kind == "event_msg" && event_type == Some("task_complete") {
                record
                    .payload
                    .last_agent_message
                    .clone()
                    .or_else(|| latest_public_message.take())
            } else {
                None
            };
        for event in events_from_records(vec![record], &mut observed) {
            let Some(mut activity) = event.activity else {
                continue;
            };
            activity.created_at = created_at;
            activity.detail = match activity.kind.as_str() {
                "message" => activity
                    .detail
                    .take()
                    .and_then(|detail| response_text(&detail)),
                "analysis" | "plan" | "prompt" => activity
                    .detail
                    .take()
                    .map(|detail| detail.chars().take(2_000).collect()),
                "file" if activity.status == "completed" => {
                    activity.detail.take().and_then(|detail| {
                        context_builder::sanitize_subagent_patch_detail(
                            &detail,
                            working_directory.as_deref(),
                        )
                    })
                }
                _ => None,
            };
            activity.files = context_builder::sanitize_subagent_file_paths(
                activity.files,
                working_directory.as_deref(),
            );
            activity.files.truncate(64);
            activity.attachments.clear();
            if let Some(index) = indices.get(&activity.id).copied() {
                activity.created_at = activities[index].created_at;
                activities[index] = activity;
            } else {
                indices.insert(activity.id.clone(), activities.len());
                activities.push(activity);
            }
        }
        if let Some(response) = completed_response
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            if let Some(activity) = activities.iter_mut().rev().find(|activity| {
                activity.kind == "message" && activity.created_at >= latest_turn_started
            }) {
                activity.title = "Resposta final".into();
                activity.detail = Some(response);
                activity.status = "completed".into();
            } else {
                activities.push(SessionActivity {
                    id: format!("codex:{child_thread_id}:final:{created_at}"),
                    kind: "message".into(),
                    title: "Resposta final".into(),
                    detail: Some(response),
                    status: "completed".into(),
                    created_at,
                    files: Vec::new(),
                    attachments: Vec::new(),
                    append_detail: false,
                });
            }
        }
    }
    let mut all_changed_files = Vec::new();
    for file in activities
        .iter()
        .filter(|activity| activity.status == "completed")
        .flat_map(|activity| &activity.files)
    {
        if all_changed_files.len() >= 64 {
            break;
        }
        if !all_changed_files.contains(file) {
            all_changed_files.push(file.clone());
        }
    }
    let excess = activities.len().saturating_sub(48);
    activities.drain(..excess);
    let visible_files = activities
        .iter()
        .flat_map(|activity| activity.files.iter())
        .collect::<HashSet<_>>();
    let older_files = all_changed_files
        .into_iter()
        .filter(|file| !visible_files.contains(file))
        .collect::<Vec<_>>();
    if !older_files.is_empty() {
        activities.insert(
            0,
            SessionActivity {
                id: format!("codex:{child_thread_id}:older-file-changes"),
                kind: "file".into(),
                title: "Arquivos alterados anteriormente".into(),
                detail: None,
                status: "completed".into(),
                created_at: activities
                    .first()
                    .map_or_else(now_millis, |activity| activity.created_at),
                files: older_files,
                attachments: Vec::new(),
                append_detail: false,
            },
        );
    }
    Ok(activities)
}

fn session_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_session_files(root, &mut files);
    files
}

fn collect_session_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_session_files(&path, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(value: &str) -> CodexRecord {
        serde_json::from_str(value).expect("registro")
    }

    fn observed_file(source: SessionSource) -> ObservedFile {
        ObservedFile {
            offset: 0,
            session: Some(SessionMetadata {
                id: "chat-1".into(),
                cwd: Some("/work/lume".into()),
                started_at: None,
                source,
            }),
            profile: None,
            pending_tools: HashMap::new(),
            token_totals: None,
            prompt_token_start: None,
            prompt_started_at: None,
        }
    }

    fn temporary_rollout(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lume-codex-session-{name}-{}-{}.jsonl",
            std::process::id(),
            now_millis()
        ))
    }

    #[test]
    fn identifies_root_codex_sessions_without_creating_subagent_duplicates() {
        let vscode = record(
            r#"{"type":"session_meta","payload":{"id":"chat-1","originator":"codex_vscode","source":"vscode","cwd":"/work/lume"}}"#,
        );
        let cli = record(
            r#"{"type":"session_meta","payload":{"id":"chat-2","originator":"codex-tui","source":"cli","cwd":"/work/lume"}}"#,
        );
        let subagent = record(
            r#"{"type":"session_meta","payload":{"id":"chat-3","originator":"codex-tui","source":{"subagent":{"other":"guardian"}},"parent_thread_id":"chat-2","thread_source":"subagent","cwd":"/work/lume"}}"#,
        );
        let memories = record(
            r#"{"type":"session_meta","payload":{"id":"chat-4","originator":"codex-tui","source":"cli","cwd":"/home/user/.codex/memories"}}"#,
        );
        let diagnostic = record(
            r#"{"type":"session_meta","payload":{"id":"chat-5","originator":"lume-diagnostic","source":"vscode","cwd":"/tmp"}}"#,
        );

        assert_eq!(
            session_metadata(&vscode).expect("VS Code").source,
            SessionSource::Vscode
        );
        assert_eq!(
            session_metadata(&cli).expect("CLI").source,
            SessionSource::Cli
        );
        assert!(session_metadata(&subagent).is_none());
        assert!(session_metadata(&memories).is_none());
        assert!(session_metadata(&diagnostic).is_none());
    }

    #[test]
    fn lifecycle_records_become_realtime_vscode_events() {
        let mut file = observed_file(SessionSource::Vscode);
        let records = vec![
            record(r#"{"type":"event_msg","payload":{"type":"task_started"}}"#),
            record(
                r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"Resposta pronta"}}"#,
            ),
        ];

        let events = events_from_records(records, &mut file);

        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0].event, HookEventKind::Running));
        assert!(matches!(&events[1].event, HookEventKind::Completed));
        assert_eq!(events[0].source, Some(SessionSource::Vscode));
        assert_eq!(events[0].native_session_id.as_deref(), Some("chat-1"));
        assert_eq!(events[1].last_response.as_deref(), Some("Resposta pronta"));
    }

    #[test]
    fn cumulative_rollout_tokens_become_per_prompt_usage() {
        let mut file = observed_file(SessionSource::Cli);
        let events = events_from_records(
            vec![
                record(
                    r#"{"timestamp":"2026-09-21T12:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":100,"input_tokens":80,"output_tokens":20}}}}"#,
                ),
                record(
                    r#"{"timestamp":"2026-09-21T12:01:00Z","type":"event_msg","payload":{"type":"task_started"}}"#,
                ),
                record(
                    r#"{"timestamp":"2026-09-21T12:01:10Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":160,"input_tokens":125,"output_tokens":35}}}}"#,
                ),
                record(
                    r#"{"timestamp":"2026-09-21T12:01:12Z","type":"event_msg","payload":{"type":"task_complete"}}"#,
                ),
            ],
            &mut file,
        );

        let activity = events
            .iter()
            .find_map(|event| event.activity.as_ref())
            .expect("token usage activity");
        assert_eq!(activity.kind, "token_usage");
        let usage: crate::domain::PromptTokenUsage =
            serde_json::from_str(activity.detail.as_deref().expect("token usage detail"))
                .expect("valid token usage");
        assert_eq!(usage.total_tokens, 60);
        assert_eq!(usage.input_tokens, 45);
        assert_eq!(usage.output_tokens, 15);
    }

    #[test]
    fn an_interrupted_turn_returns_to_waiting_instead_of_completing() {
        let mut file = observed_file(SessionSource::Vscode);
        let events = events_from_records(
            vec![record(
                r#"{"type":"event_msg","payload":{"type":"turn_aborted"}}"#,
            )],
            &mut file,
        );

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event, HookEventKind::WaitingForInput));
        assert_eq!(
            events[0].status_label.as_deref(),
            Some("Tarefa interrompida")
        );
    }

    #[test]
    fn rollout_messages_become_chat_entries() {
        let mut file = observed_file(SessionSource::Vscode);
        let records = vec![
            record(
                r#"{"timestamp":"2026-07-24T10:00:00Z","type":"event_msg","payload":{"type":"user_message","message":"Mostre os arquivos"}}"#,
            ),
            record(
                r#"{"timestamp":"2026-07-24T10:00:01Z","type":"event_msg","payload":{"type":"agent_message","message":"Alterei src/lib/TerminalWindow.svelte"}}"#,
            ),
        ];

        let events = events_from_records(records, &mut file);

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]
                .activity
                .as_ref()
                .map(|activity| activity.kind.as_str()),
            Some("prompt")
        );
        assert_eq!(
            events[0]
                .activity
                .as_ref()
                .and_then(|activity| activity.detail.as_deref()),
            Some("Mostre os arquivos")
        );
        assert_eq!(
            events[1]
                .activity
                .as_ref()
                .map(|activity| activity.kind.as_str()),
            Some("message")
        );
    }

    #[test]
    fn goal_tool_output_becomes_realtime_work_activity() {
        let mut file = observed_file(SessionSource::Cli);
        let records = vec![
            record(
                r#"{"type":"response_item","payload":{"type":"function_call","id":"fc-goal","name":"get_goal","arguments":"{}","call_id":"call-goal"}}"#,
            ),
            record(
                r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"call-goal","output":"{\"goal\":{\"objective\":\"Test goal\",\"status\":\"active\",\"createdAt\":1785190621}}"}}"#,
            ),
        ];

        let events = events_from_records(records, &mut file);

        assert_eq!(events.len(), 1);
        let activity = events[0].activity.as_ref().expect("goal activity");
        assert_eq!(activity.id, "codex:chat-1:fc-goal");
        assert_eq!(activity.kind, "tool");
        assert_eq!(activity.title, "functions · get_goal");
        assert!(activity
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("\"objective\":\"Test goal\"")));
    }

    #[test]
    fn nested_goal_tool_inside_exec_becomes_work_activity() {
        let mut file = observed_file(SessionSource::Cli);
        let records = vec![
            record(
                r#"{"type":"response_item","payload":{"type":"custom_tool_call","id":"fc-exec-goal","name":"exec","input":"const result = await tools.create_goal({objective:\"Harden takeover\"}); text(JSON.stringify(result));","call_id":"call-exec-goal"}}"#,
            ),
            record(
                r#"{"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"call-exec-goal","output":[{"type":"input_text","text":"Script completed\n"},{"type":"input_text","text":"{\"goal\":{\"objective\":\"Harden takeover\",\"status\":\"active\",\"createdAt\":1785190621}}"}]}}"#,
            ),
        ];

        let events = events_from_records(records, &mut file);

        assert_eq!(events.len(), 1);
        let activity = events[0].activity.as_ref().expect("nested goal activity");
        assert_eq!(activity.kind, "tool");
        assert_eq!(activity.title, "functions · create_goal");
        let detail = activity.detail.as_deref().expect("structured goal detail");
        let parsed: Value = serde_json::from_str(detail).expect("valid goal JSON");
        assert_eq!(
            parsed
                .get("goal")
                .and_then(|goal| goal.get("objective"))
                .and_then(Value::as_str),
            Some("Harden takeover")
        );
    }

    #[test]
    fn quoted_goal_tool_name_does_not_reclassify_an_exec_command() {
        let mut file = observed_file(SessionSource::Cli);
        let records = vec![record(
            r#"{"type":"response_item","payload":{"type":"custom_tool_call","id":"fc-search","name":"exec","input":"const result = await tools.exec_command({cmd:\"rg 'tools.create_goal(' src\"}); text(result.output);","call_id":"call-search"}}"#,
        )];

        let events = events_from_records(records, &mut file);

        let activity = events[0].activity.as_ref().expect("command activity");
        assert_eq!(activity.kind, "command");
        assert_eq!(activity.title, "Comando");
    }

    #[test]
    fn nested_todo_tool_inside_exec_becomes_work_activity() {
        let mut file = observed_file(SessionSource::Cli);
        let records = vec![
            record(
                r#"{"type":"response_item","payload":{"type":"custom_tool_call","id":"fc-exec-todo","name":"exec","input":"const result = await tools.todo_write({todos:[{content:\"Inspect\",status:\"in_progress\"}]}); text(JSON.stringify(result));","call_id":"call-exec-todo"}}"#,
            ),
            record(
                r#"{"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"call-exec-todo","output":"saved"}}"#,
            ),
        ];

        let events = events_from_records(records, &mut file);

        let activity = events
            .last()
            .and_then(|event| event.activity.as_ref())
            .expect("todo activity");
        assert_eq!(activity.kind, "tool");
        assert_eq!(activity.title, "functions · todo_write");
        assert!(activity
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("Inspect")));
    }

    #[test]
    fn command_and_patch_records_become_detailed_activities() {
        let mut file = observed_file(SessionSource::Vscode);
        let records = vec![
            record(
                r#"{"type":"response_item","payload":{"type":"function_call","id":"fc-command","name":"exec_command","arguments":"{\"cmd\":\"cargo test\"}","call_id":"call-command"}}"#,
            ),
            record(
                r#"{"type":"event_msg","payload":{"type":"exec_command_end","call_id":"call-command","command":["/bin/bash","-lc","cargo test"],"status":"completed","exit_code":0,"aggregated_output":"4 tests passed"}}"#,
            ),
            record(
                r#"{"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","call_id":"call-patch","input":"*** Begin Patch\n*** Update File: /work/lume/src/main.rs\n@@\n-old\n+new\n*** End Patch"}}"#,
            ),
            record(
                r#"{"type":"event_msg","payload":{"type":"patch_apply_end","call_id":"call-patch","status":"completed","success":true,"changes":{"/work/lume/src/main.rs":{"type":"update","unified_diff":"@@ -1 +1 @@\n-old\n+new\n"}}}}"#,
            ),
        ];

        let events = events_from_records(records, &mut file);

        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0]
                .activity
                .as_ref()
                .map(|activity| activity.kind.as_str()),
            Some("command")
        );
        assert_eq!(
            events[0]
                .activity
                .as_ref()
                .map(|activity| activity.status.as_str()),
            Some("running")
        );
        assert_eq!(
            events[1]
                .activity
                .as_ref()
                .map(|activity| activity.status.as_str()),
            Some("completed")
        );
        assert_eq!(
            events[0]
                .activity
                .as_ref()
                .map(|activity| activity.id.as_str()),
            events[1]
                .activity
                .as_ref()
                .map(|activity| activity.id.as_str())
        );
        let patch_start = events[2].activity.as_ref().expect("patch start activity");
        let patch = events[3].activity.as_ref().expect("patch activity");
        assert_eq!(patch.kind, "file");
        assert_eq!(patch_start.id, patch.id);
        assert_eq!(patch.files, vec!["/work/lume/src/main.rs"]);
        assert!(patch
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("+new")));
    }

    #[test]
    fn external_cli_subagent_lifecycle_uses_one_sidebar_identity() {
        let mut file = observed_file(SessionSource::Cli);
        let events = events_from_records(
            vec![
                record(
                    r#"{"type":"event_msg","payload":{"type":"sub_agent_activity","agent_thread_id":"child-1","agent_path":"/root/prompt_index_frontend_test","kind":"started","occurred_at_ms":1234}}"#,
                ),
                record(
                    r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"SubAgentActivity","agent_thread_id":"child-1","agent_path":"/root/prompt_index_frontend_test","kind":"completed"}}}"#,
                ),
                record(
                    r#"{"type":"event_msg","payload":{"type":"sub_agent_activity","agent_thread_id":"child-2","agent_path":"/root/prompt_index_backend_test","kind":"started"}}"#,
                ),
            ],
            &mut file,
        );
        let started = events[0].activity.as_ref().expect("subagent started");
        let completed = events[1].activity.as_ref().expect("subagent completed");
        assert_eq!(started.kind, "subagent");
        assert_eq!(
            started.title,
            "Subagente · /root/prompt_index_frontend_test"
        );
        assert_eq!(started.status, "running");
        assert_eq!(started.created_at, 1234);
        assert_eq!(completed.id, started.id);
        assert_eq!(completed.status, "completed");
        assert_ne!(
            events[2].activity.as_ref().expect("second subagent").id,
            started.id
        );
    }

    #[test]
    fn subagent_timeline_reads_only_its_branch_and_hides_tool_payloads() {
        let child_id = "01a0b0f7-8139-7403-b5f4-ab1b273ed7ba";
        let root = std::env::temp_dir().join(format!(
            "lume-subagent-timeline-{}-{}",
            std::process::id(),
            now_millis()
        ));
        fs::create_dir_all(&root).expect("test directory");
        let path = root.join(format!("rollout-test-{child_id}.jsonl"));
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"01a0b0f7-8139-7403-b5f4-ab1b273ed7ba\",\"parent_thread_id\":\"parent-1\",\"thread_source\":\"subagent\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"Inherited parent message\"}}\n",
                "{\"type\":\"inter_agent_communication_metadata\",\"payload\":{}}\n",
                "{\"timestamp\":\"2026-09-17T20:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"Checking the frontend\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"reasoning\",\"encrypted_content\":\"private data\",\"summary\":[]}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"id\":\"command-1\",\"name\":\"exec_command\",\"arguments\":\"{\\\"cmd\\\":\\\"echo secret\\\"}\",\"call_id\":\"call-1\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"call-1\",\"output\":\"secret-output\"}}\n"
            ),
        )
        .expect("test rollout");

        let timeline = load_subagent_timeline_from_root(&root, "parent-1", child_id)
            .expect("subagent timeline");
        assert_eq!(timeline.len(), 2);
        assert_eq!(timeline[0].detail.as_deref(), Some("Checking the frontend"));
        assert_eq!(timeline[1].kind, "command");
        assert_eq!(timeline[1].status, "completed");
        assert!(timeline[1].detail.is_none());
        assert!(load_subagent_timeline_from_root(&root, "another-parent", child_id).is_err());
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn subagent_timeline_keeps_full_final_response_and_changed_files() {
        let child_id = "01a0b0f7-8139-7403-b5f4-ab1b273ed7bb";
        let root = std::env::temp_dir().join(format!(
            "lume-subagent-final-{}-{}",
            std::process::id(),
            now_millis()
        ));
        fs::create_dir_all(&root).expect("test directory");
        let path = root.join(format!("rollout-test-{child_id}.jsonl"));
        let final_response = "Complete response. ".repeat(2_000).trim().to_string();
        let patch = "*** Add File: /work/child-created.rs\n+fn main() {}\n";
        let mut records = vec![
            serde_json::json!({"type":"session_meta","payload":{"id":child_id,"parent_thread_id":"parent-1","thread_source":"subagent","cwd":"/work"}}),
            serde_json::json!({"type":"inter_agent_communication_metadata","payload":{}}),
            serde_json::json!({"timestamp":"2026-09-17T20:00:00Z","type":"event_msg","payload":{"type":"task_started"}}),
            serde_json::json!({"timestamp":"2026-09-17T20:00:01Z","type":"response_item","payload":{"type":"function_call","name":"apply_patch","arguments":serde_json::json!({"patch":patch}).to_string(),"call_id":"patch-1"}}),
            serde_json::json!({"timestamp":"2026-09-17T20:00:02Z","type":"response_item","payload":{"type":"function_call_output","call_id":"patch-1","output":"Success"}}),
            serde_json::json!({"timestamp":"2026-09-17T20:00:02Z","type":"event_msg","payload":{"type":"agent_message","message":"Mid-turn update. ".repeat(160)}}),
            serde_json::json!({"timestamp":"2026-09-17T20:00:03Z","type":"event_msg","payload":{"type":"agent_message","message":final_response.clone()}}),
            serde_json::json!({"timestamp":"2026-09-17T20:00:04Z","type":"event_msg","payload":{"type":"task_complete","last_agent_message":final_response}}),
        ];
        records.splice(
            5..5,
            (0..60).map(|index| serde_json::json!({
                "timestamp":"2026-09-17T20:00:02Z",
                "type":"response_item",
                "payload":{"type":"reasoning","summary":[{"type":"summary_text","text":format!("Step {index}")}]}
            })),
        );
        fs::write(
            &path,
            format!(
                "{}\n",
                records
                    .into_iter()
                    .map(|record| record.to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        )
        .expect("test rollout");

        let timeline = load_subagent_timeline_from_root(&root, "parent-1", child_id)
            .expect("subagent timeline");
        assert_eq!(timeline[0].title, "Arquivos alterados anteriormente");
        let change = timeline
            .iter()
            .find(|activity| activity.kind == "file")
            .expect("file change");
        assert_eq!(change.files, vec!["child-created.rs"]);
        assert!(timeline.iter().any(|activity| activity.kind == "message"
            && activity.title != "Resposta final"
            && activity
                .detail
                .as_ref()
                .is_some_and(|detail| detail.len() > 960)));
        let final_message = timeline.last().expect("final response");
        assert_eq!(final_message.title, "Resposta final");
        assert_eq!(
            final_message.detail.as_deref(),
            Some(final_response.as_str())
        );
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn subagent_timeline_exposes_only_safe_recorded_diff() {
        let child_id = "01a0b0f7-8139-7403-b5f4-ab1b273ed7bc";
        let root = std::env::temp_dir().join(format!(
            "lume-subagent-diff-{}-{}",
            std::process::id(),
            now_millis()
        ));
        fs::create_dir_all(&root).expect("test directory");
        let path = root.join(format!("rollout-test-{child_id}.jsonl"));
        let patch = "*** Begin Patch\n*** Add File: /work/src/main.rs\n+fn main() {}\n*** Add File: /work/.env\n+TOKEN=private-value\n*** End Patch";
        let records = [
            serde_json::json!({"type":"session_meta","payload":{"id":child_id,"parent_thread_id":"parent-1","thread_source":"subagent","cwd":"/work"}}),
            serde_json::json!({"type":"inter_agent_communication_metadata","payload":{}}),
            serde_json::json!({"type":"response_item","payload":{"type":"function_call","name":"apply_patch","arguments":serde_json::json!({"patch":patch}).to_string(),"call_id":"patch-1"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"patch-1","output":"Success"}}),
        ];
        fs::write(
            &path,
            format!("{}\n", records.map(|record| record.to_string()).join("\n")),
        )
        .expect("test rollout");

        let timeline = load_subagent_timeline_from_root(&root, "parent-1", child_id)
            .expect("subagent timeline");
        let change = timeline
            .iter()
            .find(|activity| activity.kind == "file")
            .expect("file change");
        assert_eq!(change.files, vec!["src/main.rs"]);
        let diff = change.detail.as_deref().expect("recorded diff");
        assert!(diff.contains("diff --git a/src/main.rs b/src/main.rs"));
        assert!(diff.contains("+fn main() {}"));
        assert!(!diff.contains(".env"));
        assert!(!diff.contains("private-value"));
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn namespaced_exec_and_plan_tools_get_semantic_activity_kinds() {
        assert_eq!(tool_kind("functions.exec"), "command");
        assert_eq!(tool_title("functions.exec"), "Comando");
        assert_eq!(tool_kind("functions.update_plan"), "plan");
        assert_eq!(tool_title("functions.update_plan"), "Plano atualizado");
        assert!(is_work_tracking_tool(&RecordPayload {
            name: Some("functions.todo_write".into()),
            ..RecordPayload::default()
        }));
        assert!(is_goal_tool("functions.get_goal"));
        assert_eq!(
            files_from_patch_text(
                "const patch = \\\"\\n*** Add File: /work/child.rs\\n+new\\n\\\";"
            ),
            vec!["/work/child.rs"]
        );
        let wrapped = RecordPayload {
            arguments: Some(
                serde_json::json!({"source":"const patch = \"\\n*** Add File: /work/wrapped.rs\\n+new\\n\";"}).to_string(),
            ),
            ..RecordPayload::default()
        };
        assert_eq!(
            tool_input_text(&wrapped)
                .as_deref()
                .map(files_from_patch_text),
            Some(vec!["/work/wrapped.rs".to_string()])
        );
    }

    #[test]
    fn reasoning_summaries_are_visible_without_encrypted_reasoning() {
        let mut file = observed_file(SessionSource::Vscode);
        let events = events_from_records(
            vec![record(
                r#"{"timestamp":"2026-07-29T10:00:00Z","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"Vou validar os eventos."}],"encrypted_content":"nao exibir"}}"#,
            )],
            &mut file,
        );

        assert_eq!(events.len(), 1);
        let activity = events[0].activity.as_ref().expect("analysis activity");
        assert_eq!(activity.kind, "analysis");
        assert_eq!(activity.detail.as_deref(), Some("Vou validar os eventos."));
    }

    #[test]
    fn startup_only_restores_a_rollout_with_an_active_turn() {
        let path = temporary_rollout("bootstrap");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"chat-1\",\"originator\":\"codex_vscode\",\"source\":\"vscode\",\"cwd\":\"/work/lume\"}}\n",
                "{\"type\":\"turn_context\",\"payload\":{\"cwd\":\"/work/lume\",\"approval_policy\":\"on-request\",\"sandbox_policy\":{\"type\":\"workspace-write\"}}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"sub_agent_activity\",\"agent_thread_id\":\"child-1\",\"agent_path\":\"/root/reviewer\",\"kind\":\"started\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"sub_agent_activity\",\"agent_thread_id\":\"child-1\",\"agent_path\":\"/root/reviewer\",\"kind\":\"completed\"}}\n"
            ),
        )
        .expect("write active rollout");
        let mut file = observed_file(SessionSource::Vscode);
        let (running, subagents) = bootstrap_session(&path, &mut file);
        assert!(running);
        assert_eq!(subagents.len(), 2);
        assert_eq!(
            subagents[0]
                .activity
                .as_ref()
                .map(|activity| activity.status.as_str()),
            Some("running")
        );
        assert_eq!(
            subagents[1]
                .activity
                .as_ref()
                .map(|activity| activity.status.as_str()),
            Some("completed")
        );
        assert_eq!(
            file.profile.as_ref().map(|profile| &profile.mode),
            Some(&AccessMode::WorkspaceWrite)
        );

        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"chat-1\",\"originator\":\"codex_vscode\",\"source\":\"vscode\",\"cwd\":\"/work/lume\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"}}\n"
            ),
        )
        .expect("write completed rollout");
        assert!(!bootstrap_session(&path, &mut file).0);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn startup_recovers_nested_goal_and_explicit_todo() {
        let path = temporary_rollout("bootstrap-work");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"chat-1\",\"originator\":\"codex_cli_rs\",\"source\":\"cli\",\"cwd\":\"/work/lume\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call\",\"id\":\"fc-goal\",\"name\":\"exec\",\"input\":\"const result = await tools.create_goal({objective:\\\"Fix bookmarks\\\"}); text(JSON.stringify(result));\",\"call_id\":\"call-goal\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call_output\",\"call_id\":\"call-goal\",\"output\":[{\"type\":\"input_text\",\"text\":\"{\\\"goal\\\":{\\\"objective\\\":\\\"Fix bookmarks\\\",\\\"status\\\":\\\"active\\\"}}\"}]}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"Goal criada. TO DO ativo:\\n\\n1. Corrigir GOAL.\\n2. Corrigir TODO.\"}}\n"
            ),
        )
        .expect("write work rollout");
        let mut file = observed_file(SessionSource::Cli);

        let (_, work) = bootstrap_session(&path, &mut file);

        assert_eq!(work.len(), 2);
        assert_eq!(
            work[0]
                .activity
                .as_ref()
                .map(|activity| activity.title.as_str()),
            Some("functions · create_goal")
        );
        assert_eq!(
            work[1]
                .activity
                .as_ref()
                .map(|activity| activity.kind.as_str()),
            Some("message")
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn incremental_reader_retries_a_partial_json_record() {
        let path = temporary_rollout("partial");
        let complete = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n";
        let partial = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"";
        fs::write(&path, format!("{complete}{partial}")).expect("write partial rollout");

        let (records, offset) = read_records(&path, 0).expect("first read");
        assert_eq!(records.len(), 1);
        assert_eq!(offset, complete.len() as u64);

        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open rollout");
        file.write_all(b"}}\n").expect("finish record");

        let (records, final_offset) = read_records(&path, offset).expect("second read");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].payload.r#type.as_deref(), Some("task_complete"));
        assert_eq!(final_offset, fs::metadata(&path).expect("metadata").len());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_the_permission_profile_without_prompt_content() {
        let context = record(
            r#"{"type":"turn_context","payload":{"cwd":"/work/lume","approval_policy":"on-request","approvals_reviewer":"auto_review","sandbox_policy":{"type":"workspace-write"},"user_message":"nao deve ser guardada"}}"#,
        );
        let profile = profile_from_context(&context.payload);

        assert_eq!(profile.mode, AccessMode::WorkspaceWrite);
        assert_eq!(profile.approval_policy, "on-request");
        assert_eq!(profile.approvals_reviewer.as_deref(), Some("auto_review"));
        assert!(!profile.can_respond_from_lume);
    }

    #[test]
    fn events_without_a_turn_context_do_not_publish_a_fallback_permission_profile() {
        let file = observed_file(SessionSource::Cli);
        let event =
            event_for(&file, HookEventKind::Running, "Rodando", None).expect("session event");

        assert!(event.permission_profile.is_none());
    }
}
