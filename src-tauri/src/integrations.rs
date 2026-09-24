use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::UNIX_EPOCH,
};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::domain::SessionActivity;

const ANTIGRAVITY_HOOK_NAME: &str = "lume-session-monitor";
const ANTIGRAVITY_LEGACY_HOOK_NAME: &str = "lume";

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationKind {
    Codex,
    Claude,
    Antigravity,
    #[serde(rename = "deepseek")]
    DeepSeek,
    Gemini,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationStatus {
    pub kind: IntegrationKind,
    pub label: String,
    pub installed: bool,
    pub configured: bool,
    pub can_configure: bool,
    pub can_launch: bool,
    pub direct_permissions: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumableSession {
    pub id: String,
    pub agent: IntegrationKind,
    pub name: String,
    pub project: String,
    pub working_directory: String,
    pub source: String,
    pub updated_at: i64,
}

#[derive(Clone, Debug)]
pub struct ResumePreview {
    pub response: String,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionStatus {
    pub installed: bool,
    pub configured: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCheck {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationDiagnostic {
    pub kind: IntegrationKind,
    pub label: String,
    pub healthy: bool,
    pub checks: Vec<DiagnosticCheck>,
    pub last_event_at: Option<i64>,
}

pub fn lume_executable() -> Result<PathBuf, String> {
    if let Some(app_image) = env::var_os("APPIMAGE").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(app_image));
    }
    std::env::current_exe().map_err(|error| error.to_string())
}

pub fn statuses(executable: &str) -> Vec<IntegrationStatus> {
    crate::agent_plugins::catalog()
        .into_iter()
        .map(|plugin| {
            let kind = plugin.kind();
            let installed = crate::executables::available(plugin.executable());
            let configured = kind != IntegrationKind::Gemini
                && config_path(&kind)
                    .and_then(|path| fs::read_to_string(path).ok())
                    .is_some_and(|content| configured_content(&content, &kind, executable));
            let can_configure = kind != IntegrationKind::Gemini && config_path(&kind).is_some();
            let can_launch = kind != IntegrationKind::Gemini;
            let antigravity_hook_warning = (kind == IntegrationKind::Antigravity)
                .then(antigravity_legacy_hook_warning)
                .flatten();
            let gemini_hook_warning = (kind == IntegrationKind::Gemini)
                .then(gemini_legacy_hook_warning)
                .flatten();
            let detail = if let Some(warning) = antigravity_hook_warning {
                warning
            } else if let Some(warning) = gemini_hook_warning {
                warning
            } else if !installed {
                "CLI não encontrada".into()
            } else if kind == IntegrationKind::Gemini {
                "Somente monitoramento por processo; o Lume não altera as configurações compartilhadas do Gemini".into()
            } else if kind == IntegrationKind::DeepSeek {
                "CLI detectada; requer o perfil TUI do DeepSeek Harness".into()
            } else if plugin.hook_events().is_empty() {
                "Detecção local de processos disponível".into()
            } else if configured {
                if kind == IntegrationKind::Codex {
                    "Hook conectado; /hooks está disponível no Codex CLI".into()
                } else if kind == IntegrationKind::Antigravity {
                    "Hook conectado; ferramentas seguem permitidas se o Lume ficar indisponível; desative o hook na CLI para restaurar confirmações nativas".into()
                } else if plugin.direct_permissions() {
                    "Monitoramento e decisões conectados".into()
                } else {
                    "Monitoramento conectado".into()
                }
            } else if kind == IntegrationKind::Codex {
                "Decisões diretas ao abrir uma sessão pelo Lume".into()
            } else if kind == IntegrationKind::Antigravity {
                "Somente CLI; conectar permite todas as ferramentas mesmo se o Lume fechar; desative o hook na CLI para restaurar confirmações nativas".into()
            } else {
                "Pronto para conectar".into()
            };
            IntegrationStatus {
                kind,
                label: plugin.label().into(),
                installed,
                configured,
                can_configure,
                can_launch,
                direct_permissions: plugin.direct_permissions(),
                detail,
            }
        })
        .collect()
}

pub fn resumable_sessions(kind: &IntegrationKind) -> Result<Vec<ResumableSession>, String> {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "Could not find the user home directory".to_string())?;
    let mut sessions = match kind {
        IntegrationKind::Codex => {
            let root = env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"))
                .join("sessions");
            codex_resumable_sessions(&root)
        }
        IntegrationKind::Claude => claude_resumable_sessions(&home.join(".claude/projects")),
        IntegrationKind::Antigravity => {
            antigravity_resumable_sessions(&home.join(".gemini/antigravity-cli"))
        }
        IntegrationKind::DeepSeek | IntegrationKind::Gemini => Vec::new(),
    };
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    sessions.truncate(250);
    Ok(sessions)
}

/// Returns names that the provider already indexed for live native sessions.
///
/// Process discovery calls this frequently, so this path must stay bounded: it
/// intentionally does not walk or parse the rollout history. The complete
/// history scan remains exclusive to the user-triggered resume picker.
pub(crate) fn indexed_session_names(
    kind: &IntegrationKind,
) -> Result<HashMap<String, String>, String> {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "Could not find the user home directory".to_string())?;
    Ok(match kind {
        IntegrationKind::Codex => {
            let root = env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"))
                .join("sessions");
            cached_codex_session_names(&root)
        }
        // Claude does not currently maintain a compact thread-name index.
        // Its hook events remain the authoritative source for live names.
        IntegrationKind::Claude
        | IntegrationKind::Antigravity
        | IntegrationKind::DeepSeek
        | IntegrationKind::Gemini => HashMap::new(),
    })
}

/// Returns a small, recent slice of Codex rollouts for process discovery.
///
/// Unlike the resume picker, this deliberately inspects only the newest date
/// directories and files. Windows process arguments do not expose the thread
/// id for a newly-opened TUI, so discovery correlates these records with the
/// process start time without repeatedly walking the complete history.
#[cfg(target_os = "windows")]
pub(crate) fn recent_codex_sessions_for_discovery(limit: usize) -> Vec<ResumableSession> {
    let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) else {
        return Vec::new();
    };
    let root = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".codex"))
        .join("sessions");
    let names = cached_codex_session_names(&root);
    recent_codex_resume_files(&root, limit)
        .into_iter()
        .filter_map(|path| {
            let file = fs::File::open(&path).ok()?;
            let first_line = BufReader::new(file).lines().next()?.ok()?;
            let value = serde_json::from_str::<Value>(&first_line).ok()?;
            let mut session = codex_resume_metadata(&value, file_updated_at(&path))?;
            if let Some(name) = names.get(&session.id) {
                session.name = name.clone();
            }
            Some(session)
        })
        .collect()
}

pub(crate) fn native_session_title(kind: &IntegrationKind, session_id: &str) -> Option<String> {
    let path = resume_path(kind, session_id)?;
    let file = fs::File::open(path).ok()?;
    match kind {
        IntegrationKind::Codex => codex_session_title(BufReader::new(file)),
        IntegrationKind::Claude
        | IntegrationKind::Antigravity
        | IntegrationKind::DeepSeek
        | IntegrationKind::Gemini => None,
    }
}

pub fn resume_preview(kind: &IntegrationKind, session_id: &str) -> Option<ResumePreview> {
    let path = resume_path(kind, session_id)?;
    let recent = read_file_tail(&path, 8 * 1024 * 1024)?;
    let response = match kind {
        IntegrationKind::Codex => codex_last_response(std::io::Cursor::new(recent)),
        IntegrationKind::Claude => claude_last_response(std::io::Cursor::new(recent)),
        IntegrationKind::Antigravity | IntegrationKind::DeepSeek | IntegrationKind::Gemini => None,
    }?;
    Some(ResumePreview {
        response,
        updated_at: file_updated_at(&path),
    })
}

pub fn resume_work_activities(kind: &IntegrationKind, session_id: &str) -> Vec<SessionActivity> {
    let Some(path) = resume_path(kind, session_id) else {
        return Vec::new();
    };
    let Some(recent) = read_file_tail(&path, 8 * 1024 * 1024) else {
        return Vec::new();
    };
    match kind {
        IntegrationKind::Codex => codex_work_activities(std::io::Cursor::new(recent), session_id),
        IntegrationKind::Claude => claude_work_activities(std::io::Cursor::new(recent), session_id),
        IntegrationKind::Antigravity | IntegrationKind::DeepSeek | IntegrationKind::Gemini => {
            Vec::new()
        }
    }
}

fn read_file_tail(path: &Path, max_bytes: u64) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((length - start).min(max_bytes) as usize);
    file.read_to_end(&mut bytes).ok()?;
    if start > 0 {
        let first_complete_line = bytes.iter().position(|byte| *byte == b'\n')? + 1;
        bytes.drain(..first_complete_line);
    }
    Some(bytes)
}

fn resume_path(kind: &IntegrationKind, session_id: &str) -> Option<PathBuf> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return None;
    }
    let filename_suffix = format!("-{session_id}");
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    let root = match kind {
        IntegrationKind::Codex => env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"))
            .join("sessions"),
        IntegrationKind::Claude => home.join(".claude/projects"),
        IntegrationKind::Antigravity => {
            return antigravity_transcript_path(&home.join(".gemini/antigravity-cli"), session_id)
        }
        IntegrationKind::DeepSeek | IntegrationKind::Gemini => return None,
    };
    resume_files(&root).into_iter().find(|path| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(|stem| stem == session_id || stem.ends_with(&filename_suffix))
    })
}

fn codex_resumable_sessions(root: &Path) -> Vec<ResumableSession> {
    let names = codex_session_names(root);
    resume_files(root)
        .into_iter()
        .filter_map(|path| {
            let file = fs::File::open(&path).ok()?;
            let first_line = BufReader::new(file).lines().next()?.ok()?;
            let value = serde_json::from_str::<Value>(&first_line).ok()?;
            let mut session = codex_resume_metadata(&value, file_updated_at(&path))?;
            if let Some(name) = names.get(&session.id) {
                session.name = name.clone();
            } else if let Ok(file) = fs::File::open(&path) {
                if let Some(name) = codex_session_title(BufReader::new(file)) {
                    session.name = name;
                }
            }
            Some(session)
        })
        .collect()
}

fn codex_session_title(reader: impl BufRead) -> Option<String> {
    const MAX_SCAN_BYTES: usize = 512 * 1024;
    let mut scanned = 0usize;
    for line in reader.lines().take(160).map_while(Result::ok) {
        scanned = scanned.saturating_add(line.len());
        if scanned > MAX_SCAN_BYTES {
            break;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        for message in codex_user_messages(&value) {
            if let Some(title) = meaningful_session_title(message) {
                return Some(title);
            }
        }
    }
    None
}

fn codex_user_messages(value: &Value) -> Vec<&str> {
    match (
        value.get("type").and_then(Value::as_str),
        value.pointer("/payload/type").and_then(Value::as_str),
        value.pointer("/payload/role").and_then(Value::as_str),
    ) {
        (Some("event_msg"), Some("user_message"), _) => value
            .pointer("/payload/message")
            .and_then(Value::as_str)
            .into_iter()
            .collect(),
        (Some("response_item"), Some("message"), Some("user")) => value
            .pointer("/payload/content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("input_text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect(),
        _ => Vec::new(),
    }
}

fn meaningful_session_title(message: &str) -> Option<String> {
    let compact = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = compact.trim();
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.starts_with("# AGENTS.md")
        || normalized.starts_with("<environment_context")
        || normalized.starts_with("# Lume workflow")
        || normalized.starts_with("## Context handoff from Codex")
    {
        return None;
    }
    let mut chars = normalized.chars();
    let title = chars.by_ref().take(72).collect::<String>();
    Some(if chars.next().is_some() {
        format!("{title}…")
    } else {
        title
    })
}

fn codex_session_names(root: &Path) -> HashMap<String, String> {
    let Some(index_path) = root.parent().map(|home| home.join("session_index.jsonl")) else {
        return HashMap::new();
    };
    let Ok(file) = fs::File::open(index_path) else {
        return HashMap::new();
    };
    codex_session_name_entries(BufReader::new(file))
}

fn codex_session_name_entries(reader: impl BufRead) -> HashMap<String, String> {
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .filter_map(|value| codex_session_name_entry(&value))
        .collect()
}

struct CodexSessionNameCache {
    path: PathBuf,
    length: u64,
    updated_at: i64,
    names: HashMap<String, String>,
}

static CODEX_SESSION_NAME_CACHE: OnceLock<Mutex<Option<CodexSessionNameCache>>> = OnceLock::new();

fn cached_codex_session_names(root: &Path) -> HashMap<String, String> {
    let Some(index_path) = root.parent().map(|home| home.join("session_index.jsonl")) else {
        return HashMap::new();
    };
    let Ok(metadata) = fs::metadata(&index_path) else {
        return HashMap::new();
    };
    let updated_at = file_updated_at(&index_path);
    let cache = CODEX_SESSION_NAME_CACHE.get_or_init(|| Mutex::new(None));
    let Ok(mut cache) = cache.lock() else {
        return codex_session_names(root);
    };
    if let Some(current) = cache.as_ref() {
        if current.path == index_path
            && current.length == metadata.len()
            && current.updated_at == updated_at
        {
            return current.names.clone();
        }
    }

    // Re-read only when the compact index changes. Starting in the middle of a
    // concurrently appended JSONL record can permanently lose that name, so a
    // changed index is parsed from the beginning instead of from a byte offset.
    let names = codex_session_names(root);
    *cache = Some(CodexSessionNameCache {
        path: index_path,
        length: metadata.len(),
        updated_at,
        names: names.clone(),
    });
    names
}

fn codex_session_name_entry(value: &Value) -> Option<(String, String)> {
    let id = value.get("id")?.as_str()?.trim();
    let name = value.get("thread_name")?.as_str()?.trim();
    (!id.is_empty() && !name.is_empty()).then(|| (id.to_string(), name.to_string()))
}

fn codex_resume_metadata(value: &Value, updated_at: i64) -> Option<ResumableSession> {
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = value.get("payload")?;
    if payload
        .get("parent_thread_id")
        .is_some_and(|value| !value.is_null())
        || payload.get("thread_source").and_then(Value::as_str) == Some("subagent")
        || payload.pointer("/source/subagent").is_some()
    {
        return None;
    }
    let id = payload.get("id")?.as_str()?.to_string();
    let working_directory = payload.get("cwd")?.as_str()?.to_string();
    if crate::session_filters::is_codex_internal_workspace(&working_directory) {
        return None;
    }
    let source = match (
        payload.get("originator").and_then(Value::as_str),
        payload.get("source").and_then(Value::as_str),
    ) {
        (Some("codex_vscode"), _) | (_, Some("vscode")) => "VS Code",
        _ => "CLI",
    }
    .to_string();
    let project = resume_project_name(&working_directory);
    Some(ResumableSession {
        id,
        agent: IntegrationKind::Codex,
        name: project.clone(),
        project,
        working_directory,
        source,
        updated_at,
    })
}

fn claude_resumable_sessions(root: &Path) -> Vec<ResumableSession> {
    let mut seen = HashSet::new();
    resume_files(root)
        .into_iter()
        .filter_map(|path| {
            let file = fs::File::open(&path).ok()?;
            let mut bytes_read = 0usize;
            let mut identity: Option<(String, String)> = None;
            let mut name = None;
            for line in BufReader::new(file).lines().take(128) {
                let line = line.ok()?;
                bytes_read = bytes_read.saturating_add(line.len());
                if bytes_read > 256 * 1024 {
                    break;
                }
                let value = serde_json::from_str::<Value>(&line).ok()?;
                if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                if identity.is_none() {
                    if let (Some(id), Some(working_directory)) = (
                        value.get("sessionId").and_then(Value::as_str),
                        value.get("cwd").and_then(Value::as_str),
                    ) {
                        identity = Some((id.to_string(), working_directory.to_string()));
                    }
                }
                name = name.or_else(|| claude_session_name(&value));
                if identity.is_some() && name.is_some() {
                    break;
                }
            }
            let (id, working_directory) = identity?;
            if !seen.insert(id.clone()) {
                return None;
            }
            let project = resume_project_name(&working_directory);
            Some(ResumableSession {
                id,
                agent: IntegrationKind::Claude,
                name: name.unwrap_or_else(|| project.clone()),
                project,
                working_directory,
                source: "CLI".into(),
                updated_at: file_updated_at(&path),
            })
        })
        .collect()
}

/// Antigravity CLI documents a workspace-to-last-conversation cache. Use that
/// bounded index instead of walking or parsing every conversation transcript.
/// This intentionally exposes at most one recent conversation per workspace.
fn antigravity_resumable_sessions(root: &Path) -> Vec<ResumableSession> {
    let cache = root.join("cache/last_conversations.json");
    if fs::metadata(&cache)
        .ok()
        .is_none_or(|metadata| metadata.len() > 1024 * 1024)
    {
        return Vec::new();
    }
    let Ok(content) = fs::read_to_string(cache) else {
        return Vec::new();
    };
    let Ok(workspaces) = serde_json::from_str::<HashMap<String, String>>(&content) else {
        return Vec::new();
    };

    let mut workspaces = workspaces.into_iter().collect::<Vec<_>>();
    workspaces.sort_by(|left, right| left.0.cmp(&right.0));

    let mut sessions_by_id = HashMap::new();
    for (working_directory, id) in workspaces.into_iter().take(1_000) {
        let directory = Path::new(&working_directory);
        if !directory.is_absolute() || !directory.is_dir() || !is_safe_conversation_id(&id) {
            continue;
        }
        let Some(transcript) = antigravity_transcript_path(root, &id) else {
            continue;
        };
        let project = resume_project_name(&working_directory);
        let session = ResumableSession {
            id: id.clone(),
            agent: IntegrationKind::Antigravity,
            name: project.clone(),
            project,
            working_directory,
            source: "CLI".into(),
            updated_at: file_updated_at(&transcript),
        };
        sessions_by_id.entry(id).or_insert(session);
    }
    sessions_by_id.into_values().collect()
}

fn antigravity_transcript_path(root: &Path, conversation_id: &str) -> Option<PathBuf> {
    if !is_safe_conversation_id(conversation_id) {
        return None;
    }
    let logs = root
        .join("brain")
        .join(conversation_id)
        .join(".system_generated/logs");
    ["transcript.jsonl", "transcript_full.jsonl"]
        .into_iter()
        .map(|name| logs.join(name))
        .find(|path| path.is_file())
}

fn is_safe_conversation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn claude_session_name(value: &Value) -> Option<String> {
    let content = value.pointer("/message/content")?.as_str()?.trim();
    if content.is_empty() || content.starts_with('/') || content.starts_with("<command-") {
        return None;
    }
    let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let name = chars.by_ref().take(72).collect::<String>();
    Some(if chars.next().is_some() {
        format!("{name}…")
    } else {
        name
    })
}

fn codex_work_activities(reader: impl BufRead, session_id: &str) -> Vec<SessionActivity> {
    let mut activities = Vec::new();
    let mut pending_goals = HashMap::<String, (String, Option<String>, i64)>::new();
    for (index, line) in reader.lines().map_while(Result::ok).enumerate() {
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if record.get("type").and_then(Value::as_str) != Some("response_item") {
            continue;
        }
        let Some(payload) = record.get("payload") else {
            continue;
        };
        let created_at = record_timestamp(&record).unwrap_or(index as i64);
        match payload.get("type").and_then(Value::as_str) {
            Some("function_call" | "custom_tool_call") => {
                let Some(name) = payload.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let call_id = payload
                    .get("call_id")
                    .or_else(|| payload.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or(name);
                let input = payload
                    .get("arguments")
                    .and_then(json_text)
                    .or_else(|| payload.get("input").and_then(json_text));
                if name == "update_plan" {
                    activities.push(work_activity(
                        format!("codex:{session_id}:plan:{call_id}"),
                        "plan",
                        "Plan updated",
                        input,
                        created_at,
                    ));
                } else if matches!(name, "create_goal" | "get_goal" | "update_goal") {
                    pending_goals
                        .insert(call_id.to_string(), (name.to_string(), input, created_at));
                }
            }
            Some("function_call_output" | "custom_tool_call_output") => {
                let Some(call_id) = payload.get("call_id").and_then(Value::as_str) else {
                    continue;
                };
                let Some((name, input, started_at)) = pending_goals.remove(call_id) else {
                    continue;
                };
                let output = payload.get("output").and_then(json_text);
                let detail = if name == "get_goal" {
                    output.or(input)
                } else {
                    input.or(output)
                };
                activities.push(work_activity(
                    format!("codex:{session_id}:goal:{call_id}"),
                    "tool",
                    format!("functions · {name}"),
                    detail,
                    created_at.max(started_at),
                ));
            }
            _ => {}
        }
    }
    activities
}

fn claude_work_activities(reader: impl BufRead, session_id: &str) -> Vec<SessionActivity> {
    let mut activities = Vec::new();
    for (line_index, line) in reader.lines().map_while(Result::ok).enumerate() {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let created_at = record_timestamp(&entry).unwrap_or(line_index as i64);
        let Some(blocks) = entry.pointer("/message/content").and_then(Value::as_array) else {
            continue;
        };
        for (block_index, block) in blocks.iter().enumerate() {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let Some(name) = block.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !name.to_ascii_lowercase().contains("todo") {
                continue;
            }
            let detail = block.get("input").and_then(json_text);
            let id = block
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{line_index}:{block_index}"));
            activities.push(work_activity(
                format!("claude:{session_id}:todo:{id}"),
                "tool",
                name,
                detail,
                created_at,
            ));
        }
    }
    activities
}

fn work_activity(
    id: String,
    kind: &str,
    title: impl Into<String>,
    detail: Option<String>,
    created_at: i64,
) -> SessionActivity {
    SessionActivity {
        id,
        kind: kind.into(),
        title: title.into(),
        detail,
        status: "completed".into(),
        created_at,
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    }
}

fn json_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Null => None,
        value => Some(value.to_string()),
    }
}

fn record_timestamp(value: &Value) -> Option<i64> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.timestamp_millis())
}

fn codex_last_response(reader: impl BufRead) -> Option<String> {
    let mut last_message = None;
    let mut last_final = None;
    for line in reader.lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some((message, is_final)) = codex_agent_message(&value) else {
            continue;
        };
        last_message = Some(message.clone());
        if is_final {
            last_final = Some(message);
        }
    }
    last_final.or(last_message)
}

fn codex_agent_message(value: &Value) -> Option<(String, bool)> {
    let payload = value.get("payload")?;
    let phase = payload.get("phase").and_then(Value::as_str);
    let is_final = matches!(phase, Some("final" | "final_answer"));
    let message = match (
        value.get("type").and_then(Value::as_str),
        payload.get("type").and_then(Value::as_str),
    ) {
        (Some("event_msg"), Some("agent_message")) => {
            payload.get("message").and_then(Value::as_str)?.to_string()
        }
        (Some("response_item"), Some("message"))
            if payload.get("role").and_then(Value::as_str) == Some("assistant") =>
        {
            content_text(payload.get("content")?, "output_text")?
        }
        _ => return None,
    };
    non_empty_response(message).map(|message| (message, is_final))
}

fn claude_last_response(reader: impl BufRead) -> Option<String> {
    let mut last_message = None;
    let mut last_final = None;
    for line in reader.lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if value.get("type").and_then(Value::as_str) == Some("result") {
            if let Some(message) = value
                .get("result")
                .and_then(Value::as_str)
                .map(str::to_string)
                .and_then(non_empty_response)
            {
                last_final = Some(message);
            }
            continue;
        }
        if value.get("type").and_then(Value::as_str) != Some("assistant")
            || value.pointer("/message/role").and_then(Value::as_str) != Some("assistant")
        {
            continue;
        }
        if let Some(message) = value
            .pointer("/message/content")
            .and_then(|content| content_text(content, "text"))
            .and_then(non_empty_response)
        {
            last_message = Some(message);
        }
    }
    last_final.or(last_message)
}

fn content_text(content: &Value, block_type: &str) -> Option<String> {
    let text = content
        .as_array()?
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some(block_type))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    non_empty_response(text)
}

fn non_empty_response(response: String) -> Option<String> {
    let response = response.trim().to_string();
    (!response.is_empty()).then_some(response)
}

fn resume_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_resume_files(root, &mut files);
    files
}

#[cfg(target_os = "windows")]
fn recent_codex_resume_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    if limit == 0 {
        return Vec::new();
    }
    let mut directories = vec![root.to_path_buf()];
    for (depth, per_directory_limit) in [2usize, 2, 7].into_iter().enumerate() {
        let mut next = directories
            .iter()
            .flat_map(|directory| sorted_child_directories(directory, per_directory_limit))
            .collect::<Vec<_>>();
        if next.is_empty() {
            break;
        }
        next.sort_by(|left, right| right.cmp(left));
        next.truncate(if depth == 2 { 14 } else { 4 });
        directories = next;
    }
    let mut files = directories
        .iter()
        .flat_map(|directory| {
            fs::read_dir(directory)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file()
                        && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|path| std::cmp::Reverse(file_updated_at(path)));
    files.truncate(limit);
    files
}

#[cfg(target_os = "windows")]
fn sorted_child_directories(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut directories = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_type().ok()?.is_dir().then(|| entry.path()))
        .collect::<Vec<_>>();
    directories.sort_by(|left, right| right.cmp(left));
    directories.truncate(limit);
    directories
}

fn collect_resume_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_resume_files(&path, files);
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            files.push(path);
        }
    }
}

fn file_updated_at(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|updated| updated.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

fn resume_project_name(working_directory: &str) -> String {
    working_directory
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(working_directory)
        .to_string()
}

pub fn diagnose(
    kind: &IntegrationKind,
    executable: &str,
    last_event_at: Option<i64>,
) -> Result<IntegrationDiagnostic, String> {
    let plugin =
        crate::agent_plugins::find(kind).ok_or_else(|| "Integração não reconhecida".to_string())?;
    let mut checks = Vec::new();
    let executable_path = crate::executables::path(plugin.executable());
    checks.push(DiagnosticCheck {
        id: "cli".into(),
        label: "CLI".into(),
        status: if executable_path.is_some() {
            "ok"
        } else {
            "error"
        }
        .into(),
        detail: executable_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{} não encontrado", plugin.executable())),
    });

    if executable_path.is_some() {
        let version = crate::executables::command(plugin.executable())
            .and_then(|mut command| {
                command
                    .arg("--version")
                    .output()
                    .map_err(|error| error.to_string())
            })
            .ok()
            .and_then(|output| {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                (!stdout.is_empty())
                    .then_some(stdout)
                    .or((!stderr.is_empty()).then_some(stderr))
            });
        checks.push(DiagnosticCheck {
            id: "version".into(),
            label: "Versão".into(),
            status: if version.is_some() { "ok" } else { "warning" }.into(),
            detail: version.unwrap_or_else(|| "Não foi possível consultar a versão".into()),
        });
    }

    if plugin.hook_events().is_empty() {
        checks.push(DiagnosticCheck {
            id: "monitoring".into(),
            label: "Monitoramento".into(),
            status: if executable_path.is_some() {
                "ok"
            } else {
                "warning"
            }
            .into(),
            detail: "Detecção baseada no processo local da CLI".into(),
        });
    } else {
        let hook_path = config_path(kind);
        let configured = hook_path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .is_some_and(|content| configured_content(&content, kind, executable));
        checks.push(DiagnosticCheck {
            id: "hooks".into(),
            label: "Monitoramento".into(),
            status: if configured { "ok" } else { "warning" }.into(),
            detail: if configured {
                format!("{} eventos configurados", plugin.hook_events().len())
            } else {
                "Hook do Lume ainda não conectado".into()
            },
        });
    }
    if kind == &IntegrationKind::Gemini {
        if let Some(warning) = gemini_legacy_hook_warning() {
            checks.push(DiagnosticCheck {
                id: "legacy-hooks".into(),
                label: "Hooks legados".into(),
                status: "warning".into(),
                detail: warning,
            });
        }
    }
    checks.push(DiagnosticCheck {
        id: "activity".into(),
        label: "Último evento".into(),
        status: if last_event_at.is_some() {
            "ok"
        } else {
            "warning"
        }
        .into(),
        detail: last_event_at
            .map(|timestamp| timestamp.to_string())
            .unwrap_or_else(|| "Nenhum evento recebido nesta execução".into()),
    });
    let healthy = checks.iter().all(|check| check.status != "error");
    Ok(IntegrationDiagnostic {
        kind: plugin.kind(),
        label: plugin.label().into(),
        healthy,
        checks,
        last_event_at,
    })
}

pub fn configure(kind: &IntegrationKind, executable: &str, enabled: bool) -> Result<(), String> {
    if *kind == IntegrationKind::Antigravity {
        return configure_antigravity(executable, enabled);
    }
    if *kind == IntegrationKind::Gemini {
        return configure_legacy_gemini_at_path(
            &config_path(kind).ok_or_else(|| "Diretório do usuário não encontrado".to_string())?,
            executable,
            enabled,
        );
    }
    if enabled && *kind == IntegrationKind::Codex {
        ensure_codex_hooks_enabled()?;
    }
    let path =
        config_path(kind).ok_or_else(|| "Diretório do usuário não encontrado".to_string())?;
    let mut root = read_config(&path)?;
    if !root.is_object() {
        return Err(format!(
            "A configuração {} não contém um objeto JSON",
            path.display()
        ));
    }
    let hooks = root
        .as_object_mut()
        .expect("validado acima")
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        return Err("A chave hooks existente não contém um objeto".into());
    }

    for event in events(kind) {
        remove_lume_handlers(hooks, event, kind, executable);
        if enabled {
            add_handler(hooks, event, kind, executable)?;
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if path.exists() {
        let backup = path.with_extension("lume-backup.json");
        if !backup.exists() {
            fs::copy(&path, &backup).map_err(|error| error.to_string())?;
        }
    }
    let payload = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    fs::write(&path, format!("{payload}\n")).map_err(|error| error.to_string())
}

fn configure_legacy_gemini_at_path(
    path: &Path,
    executable: &str,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        return Err(
            "Gemini CLI legado é somente monitoramento; o Lume não instala hooks nas configurações compartilhadas do Gemini".into(),
        );
    }
    cleanup_legacy_gemini_hooks_at_path(path, executable)
}

fn cleanup_legacy_gemini_hooks_at_path(path: &Path, executable: &str) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut root = read_config(&path.to_path_buf())?;
    if !root.is_object() {
        return Err(format!(
            "A configuração compartilhada {} não contém um objeto JSON; nada foi alterado",
            path.display()
        ));
    }
    let original = root.clone();
    let Some(hooks) = root.get_mut("hooks") else {
        return Ok(());
    };
    if !hooks.is_object() {
        return Err(format!(
            "A chave hooks da configuração compartilhada {} não é um objeto; nada foi alterado",
            path.display()
        ));
    }
    // Older Lume versions could have registered handlers under event names
    // that are no longer known to this build. Sweep every event in the shared
    // Gemini settings, but remove only commands that identify themselves as a
    // Lume Gemini hook; unrelated hooks and tool/MCP settings stay untouched.
    let events = hooks
        .as_object()
        .expect("hooks validado como objeto")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for event in events {
        remove_lume_handlers(hooks, &event, &IntegrationKind::Gemini, executable);
        if hooks
            .get(&event)
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            hooks
                .as_object_mut()
                .expect("hooks validado como objeto")
                .remove(&event);
        }
    }
    if root != original {
        write_json_config(path, &root)?;
    }
    Ok(())
}

fn configure_antigravity(executable: &str, enabled: bool) -> Result<(), String> {
    let cli_settings_path = antigravity_cli_settings_path()
        .ok_or_else(|| "Diretório do usuário não encontrado".to_string())?;
    let legacy_hooks_path = antigravity_legacy_hooks_path()
        .ok_or_else(|| "Diretório do usuário não encontrado".to_string())?;
    configure_antigravity_at_paths(&cli_settings_path, &legacy_hooks_path, executable, enabled)
}

fn configure_antigravity_at_paths(
    cli_settings_path: &PathBuf,
    legacy_hooks_path: &PathBuf,
    executable: &str,
    enabled: bool,
) -> Result<(), String> {
    let legacy_hooks = if legacy_hooks_path.exists() {
        let content = fs::read_to_string(legacy_hooks_path).map_err(|error| error.to_string())?;
        let root = serde_json::from_str::<Value>(&content).map_err(|error| {
            format!(
                "A configuração compartilhada {} contém JSON inválido: {error}",
                legacy_hooks_path.display()
            )
        })?;
        if !root.is_object() {
            return Err(format!(
                "A configuração compartilhada {} não contém um objeto JSON; nada foi alterado",
                legacy_hooks_path.display()
            ));
        }
        Some(root)
    } else {
        None
    };
    let legacy_hooks = legacy_hooks.map(|mut root| {
        let original = root.clone();
        apply_named_antigravity_hooks(&mut root, ANTIGRAVITY_LEGACY_HOOK_NAME, executable, false)
            .map(|()| (root, original))
    });
    let legacy_hooks = legacy_hooks.transpose()?;

    if let Some((legacy_hooks, original_legacy_hooks)) = legacy_hooks {
        if legacy_hooks != original_legacy_hooks {
            write_json_config(legacy_hooks_path, &legacy_hooks)?;
        }
    }

    // Remove the shared IDE/CLI hook before touching CLI settings. If the CLI
    // profile is malformed or unwritable, the old hook must not remain active
    // in IDE surfaces as a side effect of a failed migration.
    let mut cli_settings = read_config(cli_settings_path)?;
    if !cli_settings.is_object() {
        return Err(format!(
            "A configuração {} não contém um objeto JSON",
            cli_settings_path.display()
        ));
    }
    let original_cli_settings = cli_settings.clone();
    apply_antigravity_cli_settings(&mut cli_settings, executable, enabled)?;
    if (enabled || cli_settings_path.exists()) && cli_settings != original_cli_settings {
        write_json_config(cli_settings_path, &cli_settings)?;
    }

    Ok(())
}

fn apply_antigravity_cli_settings(
    settings: &mut Value,
    executable: &str,
    enabled: bool,
) -> Result<(), String> {
    let Some(root) = settings.as_object_mut() else {
        return Err("As configurações do Antigravity CLI devem ser um objeto JSON".into());
    };
    if enabled {
        let hooks = root
            .entry("hooks")
            .or_insert_with(|| Value::Object(Map::new()));
        if !hooks.is_object() {
            return Err(
                "A chave hooks existente nas configurações da CLI não contém um objeto".into(),
            );
        }
        apply_named_antigravity_hooks(hooks, ANTIGRAVITY_HOOK_NAME, executable, true)
    } else if let Some(hooks) = root.get_mut("hooks") {
        if hooks.is_object() {
            apply_named_antigravity_hooks(hooks, ANTIGRAVITY_HOOK_NAME, executable, false)
        } else {
            Ok(())
        }
    } else {
        Ok(())
    }
}

fn antigravity_cli_settings_path() -> Option<PathBuf> {
    Some(antigravity_cli_settings_path_for(&antigravity_user_home()?))
}

fn antigravity_cli_settings_path_for(home: &Path) -> PathBuf {
    home.join(".gemini/antigravity-cli/settings.json")
}

fn antigravity_legacy_hooks_path() -> Option<PathBuf> {
    Some(antigravity_user_home()?.join(".gemini/config/hooks.json"))
}

fn antigravity_user_home() -> Option<PathBuf> {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn write_json_config(path: &Path, root: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if path.exists() {
        let backup = path.with_extension("lume-backup.json");
        if !backup.exists() {
            fs::copy(path, &backup).map_err(|error| error.to_string())?;
        }
    }
    let payload = serde_json::to_string_pretty(root).map_err(|error| error.to_string())?;
    fs::write(path, format!("{payload}\n")).map_err(|error| error.to_string())
}

fn apply_named_antigravity_hooks(
    root: &mut Value,
    registry_name: &str,
    executable: &str,
    enabled: bool,
) -> Result<(), String> {
    let root = root
        .as_object_mut()
        .ok_or_else(|| "A configuração do Antigravity não contém um objeto JSON".to_string())?;
    let mut lume = match root.remove(registry_name) {
        Some(Value::Object(lume)) => lume,
        Some(_) => {
            return Err(format!(
                "A entrada de hooks `{registry_name}` existente não contém um objeto"
            ));
        }
        None => Map::new(),
    };
    if enabled {
        lume.insert("enabled".into(), Value::Bool(true));
    }
    let supported_events = events(&IntegrationKind::Antigravity);
    // Replace only Lume-owned handlers, retaining third-party entries even in
    // the same registry. The shared registry is cleaned separately below.
    for event in supported_events.iter().copied() {
        if let Some(existing) = lume.get_mut(event) {
            remove_antigravity_lume_handlers(existing);
        }
        if lume
            .get(event)
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            lume.remove(event);
        }
        if !enabled || !supported_events.contains(&event) {
            continue;
        }
        // The versioned command marker represents explicit consent to the
        // wildcard allow policy without adding unsupported keys to Antigravity
        // settings. Older handler commands do not carry this marker.
        let provider = if event == "PreToolUse" {
            "antigravity:PreToolUseAllow".to_string()
        } else {
            format!("antigravity:{event}")
        };
        let handler = json!({
            "type": "command",
            "command": fail_open_hook_command(
                executable,
                &provider,
                antigravity_fallback_output(&provider),
            ),
            "timeout": 5
        });
        let handlers = if matches!(event, "PreToolUse" | "PostToolUse") {
            json!([{ "matcher": "*", "hooks": [handler] }])
        } else {
            Value::Array(vec![handler])
        };
        match (
            event,
            lume.entry(event)
                .or_insert_with(|| Value::Array(Vec::new())),
        ) {
            ("PreToolUse" | "PostToolUse", Value::Array(groups)) => {
                groups.extend(handlers.as_array().unwrap().clone())
            }
            (_, Value::Array(existing)) => existing.extend(handlers.as_array().unwrap().clone()),
            (_, _) => {
                return Err(format!(
                    "A configuração do evento Antigravity {event} não contém uma lista"
                ));
            }
        }
    }
    if !lume.is_empty() {
        root.insert(registry_name.into(), Value::Object(lume));
    }
    Ok(())
}

fn remove_antigravity_lume_handlers(value: &mut Value) {
    let Some(groups) = value.as_array_mut() else {
        return;
    };
    groups.retain_mut(|group| {
        if is_antigravity_lume_handler(group) {
            return false;
        }
        if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            handlers.retain(|handler| !is_antigravity_lume_handler(handler));
            if handlers.is_empty() {
                return false;
            }
        }
        true
    });
}

fn is_antigravity_lume_handler(value: &Value) -> bool {
    ["command", "commandWindows"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_str))
        .any(|command| command_mentions_lume_hook(command, "antigravity:"))
}

pub fn refresh_connected(executable: &str) {
    for plugin in crate::agent_plugins::catalog() {
        let kind = plugin.kind();
        if kind == IntegrationKind::Antigravity {
            let cli_content = config_path(&kind).and_then(|path| fs::read_to_string(path).ok());
            let legacy_content =
                antigravity_legacy_hooks_path().and_then(|path| fs::read_to_string(path).ok());
            if let Some(enabled) =
                antigravity_refresh_enabled(cli_content.as_deref(), legacy_content.as_deref())
            {
                // A user's explicit disabled flag must survive migration. The
                // CLI registry is authoritative when both locations exist.
                let _ = configure(&kind, executable, enabled);
            }
            continue;
        }
        if kind == IntegrationKind::Gemini {
            if let Some(path) = config_path(&kind) {
                if let Err(error) = configure_legacy_gemini_at_path(&path, executable, false) {
                    eprintln!("Could not remove legacy Lume Gemini hooks: {error}");
                }
            }
            continue;
        }
        let Some(path) = config_path(&kind) else {
            continue;
        };
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        if has_lume_handler(&content, &kind) {
            let _ = configure(&kind, executable, true);
        }
    }
}

pub fn vscode_status() -> CompanionStatus {
    let installed = command_available("code");
    let configured = installed
        && code_command()
            .arg("--list-extensions")
            .output()
            .ok()
            .is_some_and(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|extension| extension.eq_ignore_ascii_case("tulerws.lume"))
            });
    CompanionStatus {
        installed,
        configured,
        detail: if !installed {
            "VS Code não encontrado".into()
        } else if configured {
            "Extensão do Lume instalada; conexão ativa não verificada".into()
        } else {
            "Extensão do Lume não instalada; chats do Gemini Code Assist não são controlados".into()
        },
    }
}

pub fn configure_vscode(enabled: bool, vsix_path: &std::path::Path) -> Result<(), String> {
    let mut command = code_command();
    if enabled {
        if !vsix_path.exists() {
            return Err("O companion do VS Code não foi incluído no aplicativo".into());
        }
        command
            .arg("--install-extension")
            .arg(vsix_path)
            .arg("--force");
    } else {
        command.arg("--uninstall-extension").arg("tulerws.lume");
    }
    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn add_handler(
    hooks: &mut Value,
    event: &str,
    kind: &IntegrationKind,
    executable: &str,
) -> Result<(), String> {
    let groups = hooks
        .as_object_mut()
        .expect("hooks validado")
        .entry(event)
        .or_insert_with(|| Value::Array(Vec::new()));
    let groups = groups
        .as_array_mut()
        .ok_or_else(|| format!("A configuração do evento {event} não contém uma lista"))?;
    let provider = provider(kind);
    let interactive_claude_hook =
        *kind == IntegrationKind::Claude && matches!(event, "PermissionRequest" | "PreToolUse");
    let timeout = if interactive_claude_hook { 900 } else { 10 };
    let status_message = if event == "PermissionRequest" {
        "Aguardando decisão no Lume"
    } else {
        "Sincronizando com o Lume"
    };
    let handler = match kind {
        IntegrationKind::Claude => json!({
            "type": "command",
            "name": "Lume",
            "command": executable,
            "args": ["hook", provider],
            "timeout": timeout,
            "statusMessage": status_message
        }),
        IntegrationKind::Antigravity => json!({
            "type": "command",
            "name": "Lume",
            "command": fail_open_hook_command(
                executable,
                provider,
                antigravity_fallback_output(provider),
            ),
            "timeout": timeout * 1_000,
            "description": "Envia o estado da sessão ao Lume"
        }),
        IntegrationKind::Gemini => json!({
            "type": "command",
            "name": "Lume",
            "command": fail_open_hook_command(executable, provider, "{}"),
            "timeout": timeout * 1_000,
            "description": "Envia o estado da sessão ao Lume"
        }),
        IntegrationKind::DeepSeek => json!({
            "type": "command",
            "name": "Lume",
            "command": shell_command(executable, provider),
            "timeout": timeout * 1_000,
            "description": "Envia o estado da sessão ao Lume"
        }),
        IntegrationKind::Codex => json!({
            "type": "command",
            "command": shell_command(executable, provider),
            "commandWindows": powershell_command(executable, provider),
            "timeout": timeout,
            "statusMessage": "Lume monitor"
        }),
    };
    let matcher = if matches!(event, "SessionStart" | "PermissionRequest" | "Notification") {
        json!("*")
    } else {
        Value::Null
    };
    let mut group = Map::new();
    if !matcher.is_null() {
        group.insert("matcher".into(), matcher);
    }
    group.insert("hooks".into(), Value::Array(vec![handler]));
    groups.push(Value::Object(group));
    Ok(())
}

fn remove_lume_handlers(hooks: &mut Value, event: &str, kind: &IntegrationKind, executable: &str) {
    let Some(groups) = hooks
        .as_object_mut()
        .and_then(|hooks| hooks.get_mut(event))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for group in groups.iter_mut() {
        let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        handlers.retain(|handler| !is_lume_hook_handler(handler, kind, executable));
    }
    groups.retain(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .is_none_or(|handlers| !handlers.is_empty())
    });
}

fn read_config(path: &PathBuf) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(content) if !content.trim().is_empty() => {
            serde_json::from_str(&content).map_err(|error| {
                format!(
                    "A configuração {} contém JSON inválido: {error}",
                    path.display()
                )
            })
        }
        Ok(_) => Ok(Value::Object(Map::new())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(error) => Err(error.to_string()),
    }
}

fn events(kind: &IntegrationKind) -> &'static [&'static str] {
    crate::agent_plugins::find(kind)
        .map(|plugin| plugin.hook_events())
        .unwrap_or_default()
}

fn config_path(kind: &IntegrationKind) -> Option<PathBuf> {
    let user_home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    let directory = match kind {
        IntegrationKind::Codex => ".codex/hooks.json",
        IntegrationKind::Claude => ".claude/settings.json",
        IntegrationKind::Antigravity => {
            return Some(antigravity_cli_settings_path_for(&PathBuf::from(user_home)))
        }
        IntegrationKind::DeepSeek => return None,
        IntegrationKind::Gemini => ".gemini/settings.json",
    };
    Some(PathBuf::from(user_home).join(directory))
}

fn codex_user_config_path() -> Option<PathBuf> {
    let user_home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    Some(PathBuf::from(user_home).join(".codex/config.toml"))
}

fn ensure_codex_hooks_enabled() -> Result<(), String> {
    let path = codex_user_config_path()
        .ok_or_else(|| "Diretório de configuração do Codex não encontrado".to_string())?;
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.to_string()),
    };
    let Some(updated) = config_with_hooks_enabled(&content)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if path.exists() {
        let backup = path.with_extension("lume-backup.toml");
        if !backup.exists() {
            fs::copy(&path, &backup).map_err(|error| error.to_string())?;
        }
    }
    fs::write(path, updated).map_err(|error| error.to_string())
}

fn config_with_hooks_enabled(content: &str) -> Result<Option<String>, String> {
    let mut lines = content.lines().map(str::to_string).collect::<Vec<_>>();
    if let Some(features_index) = lines.iter().position(|line| line.trim() == "[features]") {
        let section_end = lines
            .iter()
            .enumerate()
            .skip(features_index + 1)
            .find(|(_, line)| line.trim().starts_with('['))
            .map(|(index, _)| index)
            .unwrap_or(lines.len());
        if let Some(line) = lines[features_index + 1..section_end].iter().find(|line| {
            line.split_once('=')
                .is_some_and(|(key, _)| key.trim() == "hooks")
        }) {
            let value = line
                .split_once('=')
                .map(|(_, value)| value.split('#').next().unwrap_or_default().trim())
                .unwrap_or_default();
            return match value {
                "true" => Ok(None),
                "false" => Err(
                    "Os hooks estão desativados em ~/.codex/config.toml; ative features.hooks para conectar o Lume"
                        .into(),
                ),
                _ => Err("A opção features.hooks do Codex não é válida".into()),
            };
        }
        lines.insert(features_index + 1, "hooks = true".into());
    } else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.extend(["[features]".into(), "hooks = true".into()]);
    }
    Ok(Some(format!("{}\n", lines.join("\n"))))
}

fn provider(kind: &IntegrationKind) -> &'static str {
    match kind {
        IntegrationKind::Codex => "codex",
        IntegrationKind::Claude => "claude",
        IntegrationKind::Antigravity => "antigravity",
        IntegrationKind::DeepSeek => "deepseek",
        IntegrationKind::Gemini => "gemini",
    }
}

fn is_lume_hook_handler(handler: &Value, kind: &IntegrationKind, executable: &str) -> bool {
    if ["command", "commandWindows"]
        .into_iter()
        .filter_map(|key| handler.get(key).and_then(Value::as_str))
        .any(|command| {
            command_invokes_lume_hook(command, kind)
                || decode_powershell_command(command)
                    .as_deref()
                    .is_some_and(|decoded| command_invokes_lume_hook(decoded, kind))
        })
    {
        return true;
    }

    let command = handler.get("command").and_then(Value::as_str).unwrap_or("");
    let executable_name = command
        .trim_matches(['\"', '\''])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let invokes_lume = command == executable
        || matches!(executable_name.as_str(), "lume" | "lume.exe" | "lume-cli");
    invokes_lume
        && handler
            .get("args")
            .and_then(Value::as_array)
            .is_some_and(|args| {
                args.first().and_then(Value::as_str) == Some("hook")
                    && args.get(1).and_then(Value::as_str) == Some(provider(kind))
            })
}

fn command_invokes_lume_hook(command: &str, kind: &IntegrationKind) -> bool {
    let normalized = command.to_ascii_lowercase();
    let tokens = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.contains(&"lume")
        && tokens
            .windows(2)
            .any(|pair| pair[0] == "hook" && pair[1] == provider(kind))
}

fn configured_content(content: &str, kind: &IntegrationKind, executable: &str) -> bool {
    let Ok(root) = serde_json::from_str::<Value>(content) else {
        return false;
    };
    if *kind == IntegrationKind::Antigravity {
        return antigravity_hook_registry(&root).is_some_and(|value| {
            antigravity_auto_approval_enabled(value)
                && value_contains_command(value, executable, "antigravity:")
        });
    }
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    hooks.values().any(|groups| {
        groups.as_array().is_some_and(|groups| {
            groups.iter().any(|group| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_some_and(|handlers| {
                        handlers.iter().any(|handler| {
                            let command =
                                handler.get("command").and_then(Value::as_str).unwrap_or("");
                            let exec_form = command == executable
                                && handler.get("args").and_then(Value::as_array).is_some_and(
                                    |args| {
                                        args.first().and_then(Value::as_str) == Some("hook")
                                            && args.get(1).and_then(Value::as_str)
                                                == Some(provider(kind))
                                    },
                                );
                            let shell_form = command.contains(executable)
                                && command.contains(&format!(" hook {}", provider(kind)));
                            let windows_form = handler
                                .get("commandWindows")
                                .and_then(Value::as_str)
                                .is_some_and(|command| {
                                    command.contains(executable)
                                        && command.contains(&format!(" hook {}", provider(kind)))
                                });
                            exec_form || shell_form || windows_form
                        })
                    })
            })
        })
    })
}

fn has_lume_handler(content: &str, kind: &IntegrationKind) -> bool {
    let Ok(root) = serde_json::from_str::<Value>(content) else {
        return false;
    };
    if *kind == IntegrationKind::Antigravity {
        return antigravity_hook_registry(&root)
            .is_some_and(|value| value_contains_lume_hook(value, "antigravity:"));
    }
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    hooks.values().any(|groups| {
        groups.as_array().is_some_and(|groups| {
            groups.iter().any(|group| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_some_and(|handlers| {
                        handlers
                            .iter()
                            .any(|handler| is_lume_hook_handler(handler, kind, ""))
                    })
            })
        })
    })
}

fn antigravity_hook_registry(root: &Value) -> Option<&Value> {
    [
        root.get("hooks")
            .and_then(|hooks| hooks.get(ANTIGRAVITY_HOOK_NAME)),
        root.get(ANTIGRAVITY_HOOK_NAME),
        root.get(ANTIGRAVITY_LEGACY_HOOK_NAME),
    ]
    .into_iter()
    .flatten()
    .find(|registry| value_contains_lume_hook(registry, "antigravity:"))
}

fn antigravity_registry_enabled(registry: &Value) -> bool {
    registry.get("enabled").and_then(Value::as_bool) != Some(false)
}

fn antigravity_auto_approval_enabled(registry: &Value) -> bool {
    antigravity_registry_enabled(registry)
        && value_contains_lume_hook(registry, "antigravity:PreToolUseAllow")
}

fn antigravity_cli_hook_enabled(content: &str) -> Option<bool> {
    let root = serde_json::from_str::<Value>(content).ok()?;
    antigravity_hook_registry(&root).map(antigravity_auto_approval_enabled)
}

fn legacy_antigravity_hook_enabled(content: &str) -> Option<bool> {
    let root = serde_json::from_str::<Value>(content).ok()?;
    root.get(ANTIGRAVITY_LEGACY_HOOK_NAME)
        .filter(|registry| value_contains_lume_hook(registry, "antigravity:"))
        .map(antigravity_registry_enabled)
}

fn gemini_legacy_hook_warning() -> Option<String> {
    let path = config_path(&IntegrationKind::Gemini)?;
    gemini_legacy_hook_warning_at_path(&path)
}

fn gemini_legacy_hook_warning_at_path(path: &Path) -> Option<String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(format!(
                "Não foi possível verificar os hooks compartilhados do Gemini ({error}); o Lume preservou {}",
                path.display()
            ));
        }
    };
    if !serde_json::from_str::<Value>(&content).is_ok_and(|root| root.is_object()) {
        return Some(format!(
            "As configurações compartilhadas do Gemini estão inválidas e foram preservadas: {}",
            path.display()
        ));
    }
    if has_lume_handler(&content, &IntegrationKind::Gemini) {
        Some(format!(
            "Um hook antigo do Lume ainda está nas configurações compartilhadas do Gemini: {}",
            path.display()
        ))
    } else {
        None
    }
}

fn antigravity_legacy_hook_warning() -> Option<String> {
    let path = antigravity_legacy_hooks_path()?;
    antigravity_legacy_hook_warning_at_path(&path)
}

fn antigravity_legacy_hook_warning_at_path(path: &Path) -> Option<String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(format!(
                "Não foi possível verificar os hooks compartilhados ({error}); o Lume não alterou {}",
                path.display()
            ));
        }
    };
    let root = match serde_json::from_str::<Value>(&content) {
        Ok(root) if root.is_object() => root,
        Ok(_) => {
            return Some(format!(
                "A configuração compartilhada não é um objeto JSON e foi preservada: {}",
                path.display()
            ));
        }
        Err(_) => {
            return Some(format!(
                "A configuração compartilhada está com JSON inválido e foi preservada: {}",
                path.display()
            ));
        }
    };
    root.get(ANTIGRAVITY_LEGACY_HOOK_NAME)
        .filter(|registry| {
            value_contains_lume_hook(registry, "antigravity:")
                && antigravity_registry_enabled(registry)
        })
        .map(|_| {
            format!(
                "O hook antigo do Lume ainda está ativo na configuração compartilhada: {}",
                path.display()
            )
        })
}

fn antigravity_refresh_enabled(
    cli_content: Option<&str>,
    legacy_content: Option<&str>,
) -> Option<bool> {
    cli_content
        .and_then(antigravity_cli_hook_enabled)
        .or_else(|| {
            legacy_content
                .and_then(legacy_antigravity_hook_enabled)
                .map(|_| false)
        })
}

fn value_contains_command(value: &Value, executable: &str, marker: &str) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            (key == "command"
                && value.as_str().is_some_and(|command| {
                    command_contains_executable_marker(command, executable, marker)
                }))
                || value_contains_command(value, executable, marker)
        }),
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_command(value, executable, marker)),
        _ => false,
    }
}

fn value_contains_lume_hook(value: &Value, marker: &str) -> bool {
    match value {
        Value::String(text) => command_mentions_lume_hook(text, marker),
        Value::Object(object) => object
            .values()
            .any(|value| value_contains_lume_hook(value, marker)),
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_lume_hook(value, marker)),
        _ => false,
    }
}

fn command_mentions_lume_hook(command: &str, marker: &str) -> bool {
    let mentions = |value: &str| {
        let normalized = value.to_ascii_lowercase();
        mentions_lume_executable(value)
            && normalized.contains("hook")
            && normalized.contains(&marker.to_ascii_lowercase())
    };
    mentions(command)
        || decode_powershell_command(command)
            .as_deref()
            .is_some_and(mentions)
}

fn mentions_lume_executable(command: &str) -> bool {
    command.split_whitespace().any(|part| {
        let executable = part.trim_matches(['\"', '\'']);
        let name = executable
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        matches!(name.as_str(), "lume" | "lume.exe" | "lume-cli")
            || (name.starts_with("lume") && name.ends_with(".appimage"))
    })
}

fn command_contains_executable_marker(command: &str, executable: &str, marker: &str) -> bool {
    let matches = |value: &str| value.contains(executable) && value.contains(marker);
    matches(command)
        || decode_powershell_command(command)
            .as_deref()
            .is_some_and(matches)
}

fn decode_powershell_command(command: &str) -> Option<String> {
    let mut parts = command.split_whitespace();
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("-EncodedCommand") {
            let bytes = BASE64_STANDARD
                .decode(parts.next()?.trim_matches(['\"', '\'']))
                .ok()?;
            let words = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            return String::from_utf16(&words).ok();
        }
    }
    None
}

fn shell_command(executable: &str, provider: &str) -> String {
    format!("\"{}\" hook {provider}", executable.replace('"', "\\\""))
}

fn antigravity_fallback_output(provider: &str) -> &'static str {
    match provider.rsplit_once(':').map(|(_, event)| event) {
        Some("PreToolUse" | "PreToolUseAllow") => r#"{"decision":"allow"}"#,
        Some("Stop") => r#"{"decision":"allow"}"#,
        _ => "{}",
    }
}

fn fail_open_hook_command(executable: &str, provider: &str, fallback_json: &str) -> String {
    if cfg!(windows) {
        powershell_fail_open_hook_command(executable, provider, fallback_json)
    } else {
        posix_fail_open_hook_command(executable, provider, fallback_json)
    }
}

fn posix_fail_open_hook_command(executable: &str, provider: &str, fallback_json: &str) -> String {
    format!(
        "output=$({} hook {} 2>/dev/null); status=$?; if [ \"$status\" -eq 0 ] && [ -n \"$output\" ]; then printf '%s\\n' \"$output\"; else printf '%s\\n' {}; fi",
        posix_quote(executable),
        provider,
        posix_quote(fallback_json)
    )
}

fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn powershell_fail_open_hook_command(
    executable: &str,
    provider: &str,
    fallback_json: &str,
) -> String {
    let quote = |value: &str| format!("'{}'", value.replace('\'', "''"));
    let fallback = quote(fallback_json);
    let script = format!(
        "$ErrorActionPreference = 'Stop'; try {{ $output = @(& {} {} {}); $status = $LASTEXITCODE; $payload = ($output -join [Environment]::NewLine).Trim(); if ($status -eq 0 -and $payload.Length -gt 0) {{ [Console]::Out.WriteLine($payload) }} else {{ [Console]::Out.WriteLine({fallback}) }} }} catch {{ [Console]::Out.WriteLine({fallback}) }}; exit 0",
        quote(executable),
        quote("hook"),
        quote(provider)
    );
    let encoded = BASE64_STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    format!("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {encoded}")
}

fn powershell_command(executable: &str, provider: &str) -> String {
    format!("& '{}' hook {provider}", executable.replace('\'', "''"))
}

#[cfg(not(target_os = "windows"))]
fn command_available(command: &str) -> bool {
    Command::new(command).arg("--version").output().is_ok()
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

#[cfg(not(target_os = "windows"))]
pub(crate) fn code_command() -> Command {
    Command::new("code")
}

#[cfg(target_os = "windows")]
pub(crate) fn code_command() -> Command {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new("cmd.exe");
    command
        .args(["/D", "/S", "/C", "code"])
        .creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_resume_metadata_excludes_subagents_and_keeps_project_context() {
        let session = codex_resume_metadata(
            &json!({
                "type": "session_meta",
                "payload": {
                    "id": "thread-1",
                    "cwd": "/work/lume",
                    "originator": "codex_vscode",
                    "source": "vscode"
                }
            }),
            42,
        )
        .expect("sessão retomável");
        assert_eq!(session.name, "lume");
        assert_eq!(session.project, "lume");
        assert_eq!(session.source, "VS Code");
        assert_eq!(session.updated_at, 42);

        assert!(codex_resume_metadata(
            &json!({
                "type": "session_meta",
                "payload": {
                    "id": "thread-child",
                    "cwd": "/work/lume",
                    "source": { "subagent": { "other": "worker" } }
                }
            }),
            43,
        )
        .is_none());
        assert!(codex_resume_metadata(
            &json!({
                "type": "session_meta",
                "payload": {
                    "id": "memory-thread",
                    "cwd": "/home/user/.codex/memories",
                    "originator": "codex-tui",
                    "source": "cli"
                }
            }),
            44,
        )
        .is_none());
    }

    #[test]
    fn codex_session_index_exposes_the_saved_thread_name() {
        let entry = codex_session_name_entry(&json!({
            "id": "thread-1",
            "thread_name": "Lume principal"
        }))
        .expect("nome salvo");

        assert_eq!(entry, ("thread-1".into(), "Lume principal".into()));
    }

    #[test]
    fn codex_resume_name_falls_back_to_the_first_real_user_message() {
        let records = [
            json!({
                "type": "event_msg",
                "payload": { "type": "user_message", "message": "# Lume workflow context" }
            }),
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "user_message",
                    "message": "Corrija a retomada das sessões sem perder o contexto"
                }
            }),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");

        assert_eq!(
            codex_session_title(BufReader::new(records.as_bytes())).as_deref(),
            Some("Corrija a retomada das sessões sem perder o contexto")
        );
    }

    #[test]
    fn codex_resume_name_reads_modern_response_item_prompts() {
        let records = [
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "# AGENTS.md instructions" },
                        { "type": "input_text", "text": "<environment_context>ignored</environment_context>" }
                    ]
                }
            }),
            json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": "Prepare o jogo Invicto para lançamento"
                    }]
                }
            }),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");

        assert_eq!(
            codex_session_title(BufReader::new(records.as_bytes())).as_deref(),
            Some("Prepare o jogo Invicto para lançamento")
        );
    }

    #[test]
    fn claude_resume_name_uses_the_first_meaningful_prompt() {
        assert!(claude_session_name(&json!({
            "message": { "content": "/plan" }
        }))
        .is_none());
        assert_eq!(
            claude_session_name(&json!({
                "message": { "content": "  Revise   o fluxo de autenticação  " }
            }))
            .as_deref(),
            Some("Revise o fluxo de autenticação")
        );
    }

    #[test]
    fn antigravity_resume_uses_only_workspace_indexed_cli_conversations() {
        let root = std::env::temp_dir().join(format!(
            "lume-antigravity-resume-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let workspace_a = root.join("workspace-a");
        let workspace_z = root.join("workspace-z");
        let id = "019f8061-7032-7521-b333-84f84c744fa8";
        let transcript = root
            .join("antigravity-cli/brain")
            .join(id)
            .join(".system_generated/logs/transcript.jsonl");
        fs::create_dir_all(&workspace_a).expect("workspace A");
        fs::create_dir_all(&workspace_z).expect("workspace Z");
        fs::create_dir_all(transcript.parent().expect("logs directory")).expect("logs");
        fs::write(&transcript, "{}\n").expect("transcript");
        fs::create_dir_all(root.join("antigravity-cli/brain/unindexed/.system_generated/logs"))
            .expect("unindexed logs");
        fs::write(
            root.join("antigravity-cli/brain/unindexed/.system_generated/logs/transcript.jsonl"),
            "{}\n",
        )
        .expect("unindexed transcript");
        fs::create_dir_all(root.join("antigravity-cli/cache")).expect("cache directory");
        fs::write(
            root.join("antigravity-cli/cache/last_conversations.json"),
            serde_json::to_vec(&json!({
                workspace_z.to_string_lossy().to_string(): id,
                workspace_a.to_string_lossy().to_string(): id
            }))
            .expect("cache JSON"),
        )
        .expect("cache");

        let sessions = antigravity_resumable_sessions(&root.join("antigravity-cli"));

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, id);
        assert_eq!(sessions[0].agent, IntegrationKind::Antigravity);
        assert_eq!(sessions[0].name, "workspace-a");
        assert_eq!(sessions[0].working_directory, workspace_a.to_string_lossy());
        assert_eq!(sessions[0].source, "CLI");
        assert!(sessions[0].updated_at > 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn antigravity_resume_rejects_untrusted_ids_and_relative_workspaces() {
        assert!(antigravity_transcript_path(Path::new("/tmp/agy"), "../outside").is_none());
        assert!(!is_safe_conversation_id("nested/session"));
        assert!(!is_safe_conversation_id("."));
    }

    #[test]
    fn codex_resume_preview_prefers_the_latest_final_answer() {
        let transcript = concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"phase\":\"commentary\",\"message\":\"Analisando\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"Tudo pronto.\"}]}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"phase\":\"commentary\",\"message\":\"Novo trabalho iniciado\"}}\n",
        );

        assert_eq!(
            codex_last_response(std::io::Cursor::new(transcript)).as_deref(),
            Some("Tudo pronto.")
        );
    }

    #[test]
    fn resume_tail_discards_a_partial_record_and_keeps_recent_records() {
        let path = std::env::temp_dir().join(format!(
            "lume-resume-tail-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::write(&path, b"old-record\nrecent-one\nrecent-two\n").expect("arquivo");

        let recent = read_file_tail(&path, 24).expect("cauda recente");

        assert_eq!(
            String::from_utf8(recent).unwrap(),
            "recent-one\nrecent-two\n"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn claude_resume_preview_reads_the_last_agent_text() {
        let transcript = concat!(
            "{\"type\":\"assistant\",\"isSidechain\":false,\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"internal\"}]}}\n",
            "{\"type\":\"assistant\",\"isSidechain\":false,\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Resposta anterior do Claude.\"}]}}\n",
        );

        assert_eq!(
            claude_last_response(std::io::Cursor::new(transcript)).as_deref(),
            Some("Resposta anterior do Claude.")
        );
    }

    #[test]
    fn codex_work_state_is_rebuilt_from_plan_and_goal_records() {
        let transcript = concat!(
            "{\"timestamp\":\"2026-07-30T12:00:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"name\":\"update_plan\",\"call_id\":\"plan-1\",\"arguments\":\"{\\\"explanation\\\":\\\"Phase 1\\\",\\\"plan\\\":[{\\\"step\\\":\\\"Test handoff\\\",\\\"status\\\":\\\"completed\\\"}]}\"}}\n",
            "{\"timestamp\":\"2026-07-30T12:01:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"name\":\"get_goal\",\"call_id\":\"goal-1\",\"arguments\":\"{}\"}}\n",
            "{\"timestamp\":\"2026-07-30T12:01:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"goal-1\",\"output\":\"{\\\"goal\\\":{\\\"objective\\\":\\\"Workflow\\\",\\\"status\\\":\\\"active\\\",\\\"createdAt\\\":1785400000}}\"}}\n",
        );
        let activities = codex_work_activities(std::io::Cursor::new(transcript), "thread-1");
        assert_eq!(activities.len(), 2);
        assert_eq!(activities[0].kind, "plan");
        assert!(activities[0]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("Test handoff")));
        assert_eq!(activities[1].title, "functions · get_goal");
        assert!(activities[1]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("Workflow")));
    }

    #[test]
    fn claude_work_state_is_rebuilt_from_todo_write() {
        let transcript = "{\"timestamp\":\"2026-07-30T12:00:00Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"todo-1\",\"name\":\"TodoWrite\",\"input\":{\"todos\":[{\"content\":\"Inspect workflow\",\"status\":\"in_progress\"}]}}]}}\n";
        let activities = claude_work_activities(std::io::Cursor::new(transcript), "session-1");
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].title, "TodoWrite");
        assert!(activities[0]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("Inspect workflow")));
    }

    #[test]
    fn adding_and_removing_lume_keeps_existing_hooks() {
        let mut hooks = json!({
            "Stop": [{
                "hooks": [
                    { "type": "command", "command": "notify-existing" },
                    { "type": "command", "name": "Lume", "command": "third-party-lume-tool" }
                ]
            }]
        });
        add_handler(
            &mut hooks,
            "Stop",
            &IntegrationKind::Claude,
            "/opt/Lume/lume",
        )
        .expect("adiciona o hook");
        assert_eq!(
            hooks["Stop"].as_array().expect("grupos").len(),
            2,
            "o hook existente deve ser preservado"
        );

        remove_lume_handlers(
            &mut hooks,
            "Stop",
            &IntegrationKind::Claude,
            "/opt/Lume/lume",
        );
        assert_eq!(hooks["Stop"].as_array().expect("grupos").len(), 1);
        assert_eq!(hooks["Stop"][0]["hooks"][0]["command"], "notify-existing");
        assert_eq!(
            hooks["Stop"][0]["hooks"][1]["command"],
            "third-party-lume-tool"
        );
    }

    #[test]
    fn hook_commands_keep_executable_paths_as_single_arguments() {
        let mut hooks = json!({});
        add_handler(
            &mut hooks,
            "PermissionRequest",
            &IntegrationKind::Claude,
            "/opt/Lume App/lume",
        )
        .expect("adiciona o hook");
        let handler = &hooks["PermissionRequest"][0]["hooks"][0];
        assert_eq!(handler["command"], "/opt/Lume App/lume");
        assert_eq!(handler["args"], json!(["hook", "claude"]));
        let root = json!({ "hooks": hooks });
        assert!(configured_content(
            &root.to_string(),
            &IntegrationKind::Claude,
            "/opt/Lume App/lume"
        ));
    }

    #[test]
    fn claude_question_hook_waits_for_the_lume_response() {
        let mut hooks = json!({});
        add_handler(
            &mut hooks,
            "PreToolUse",
            &IntegrationKind::Claude,
            "/opt/Lume/lume",
        )
        .expect("adiciona o hook");
        assert_eq!(hooks["PreToolUse"][0]["hooks"][0]["timeout"], 900);
    }

    #[test]
    fn recognizes_connected_lume_hooks_from_an_older_executable() {
        let root = json!({
            "hooks": {
                "PermissionRequest": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "name": "Lume",
                        "command": "/old/Lume/lume",
                        "args": ["hook", "claude"]
                    }]
                }]
            }
        });

        assert!(has_lume_handler(
            &root.to_string(),
            &IntegrationKind::Claude
        ));
    }

    #[test]
    fn quoted_codex_command_is_recognized_as_connected() {
        let root = json!({
            "hooks": {
                "SessionStart": [{
                    "hooks": [{
                        "type": "command",
                        "command": "\"/usr/bin/lume\" hook codex",
                        "statusMessage": "Lume monitor"
                    }]
                }]
            }
        });

        assert!(configured_content(
            &root.to_string(),
            &IntegrationKind::Codex,
            "/usr/bin/lume"
        ));
    }

    #[test]
    fn legacy_gemini_hook_cleanup_preserves_shared_tools_and_other_hooks() {
        let root = std::env::temp_dir().join(format!(
            "lume-gemini-hook-cleanup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let settings_path = root.join(".gemini/settings.json");
        fs::create_dir_all(settings_path.parent().expect("Gemini settings dir"))
            .expect("create Gemini settings dir");
        let original = json!({
            "coreTools": ["run_shell_command"],
            "mcpServers": { "private-server": { "command": "trusted-tool" } },
            "hooks": {
                "BeforeAgent": [{
                    "hooks": [
                        {
                            "type": "command",
                            "name": "Lume",
                            "command": "'/old/Lume/lume' hook gemini || printf '{}'"
                        },
                        {
                            "type": "command",
                            "name": "user-hook",
                            "command": "user-before-agent-hook"
                        }
                    ]
                }],
                "BeforeSkillDownload": [{
                    "hooks": [
                        {
                            "type": "command",
                            "name": "Lume",
                            "command": "'/old/Lume/lume' hook gemini || printf '{}'"
                        },
                        {
                            "type": "command",
                            "name": "skill-policy",
                            "command": "user-skill-policy-hook"
                        }
                    ]
                }],
                "AfterTool": [{
                    "hooks": [{ "type": "command", "command": "user-after-tool-hook" }]
                }]
            }
        });
        fs::write(
            &settings_path,
            serde_json::to_vec(&original).expect("settings JSON"),
        )
        .expect("write Gemini settings");

        cleanup_legacy_gemini_hooks_at_path(&settings_path, "/opt/Lume/lume")
            .expect("remove only legacy Lume hooks");

        let updated: Value =
            serde_json::from_slice(&fs::read(&settings_path).expect("read updated settings"))
                .expect("updated settings JSON");
        assert_eq!(updated["coreTools"], original["coreTools"]);
        assert_eq!(updated["mcpServers"], original["mcpServers"]);
        assert_eq!(
            updated["hooks"]["BeforeAgent"][0]["hooks"][0]["command"],
            "user-before-agent-hook"
        );
        assert_eq!(
            updated["hooks"]["AfterTool"][0]["hooks"][0]["command"],
            "user-after-tool-hook"
        );
        assert_eq!(
            updated["hooks"]["BeforeSkillDownload"][0]["hooks"][0]["command"],
            "user-skill-policy-hook"
        );
        assert_eq!(
            updated["hooks"]["BeforeSkillDownload"][0]["hooks"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "unrecognized Gemini events still lose only the stale Lume hook"
        );
        assert!(!has_lume_handler(
            &updated.to_string(),
            &IntegrationKind::Gemini
        ));
        assert_eq!(
            serde_json::from_slice::<Value>(
                &fs::read(settings_path.with_extension("lume-backup.json"))
                    .expect("settings backup"),
            )
            .expect("backup JSON"),
            original
        );

        let before_enable_attempt = fs::read(&settings_path).expect("settings after cleanup");
        assert!(configure_legacy_gemini_at_path(&settings_path, "/opt/Lume/lume", true).is_err());
        assert_eq!(
            fs::read(&settings_path).expect("settings after rejected enable"),
            before_enable_attempt
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_gemini_hook_cleanup_recognizes_encoded_windows_commands() {
        let command =
            powershell_fail_open_hook_command(r"C:\Program Files\Lume\lume.exe", "gemini", "{}");
        assert!(is_lume_hook_handler(
            &json!({ "command": command }),
            &IntegrationKind::Gemini,
            ""
        ));
    }

    #[test]
    fn malformed_legacy_gemini_settings_are_preserved() {
        let root = std::env::temp_dir().join(format!(
            "lume-gemini-invalid-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let settings_path = root.join("settings.json");
        fs::create_dir_all(&root).expect("create temporary settings directory");
        let invalid = b"{invalid json";
        fs::write(&settings_path, invalid).expect("write malformed settings");

        assert!(cleanup_legacy_gemini_hooks_at_path(&settings_path, "/opt/Lume/lume").is_err());
        assert_eq!(
            fs::read(&settings_path).expect("read preserved settings"),
            invalid
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn antigravity_hooks_use_the_named_registry_and_preserve_other_entries() {
        let mut root = json!({
            "existing": {
                "Stop": [{ "type": "command", "command": "notify-existing" }]
            },
            "lume": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{ "type": "command", "command": "/old/lume hook antigravity:PreToolUse" }]
                }],
                "PostInvocation": [{
                    "hooks": [{ "type": "command", "command": "custom-tool hook antigravity:PostInvocation" }]
                }],
                "custom-event": [{ "type": "command", "command": "user-owned-hook" }]
            }
        });
        apply_named_antigravity_hooks(
            &mut root,
            ANTIGRAVITY_LEGACY_HOOK_NAME,
            "/opt/Lume App/lume",
            true,
        )
        .expect("configura hooks Antigravity");

        assert_eq!(root["existing"]["Stop"][0]["command"], "notify-existing");
        assert_eq!(root["lume"]["enabled"], true);
        assert_eq!(
            root["lume"]["PostInvocation"][0]["hooks"][0]["command"],
            "custom-tool hook antigravity:PostInvocation"
        );
        assert_eq!(
            root["lume"]["custom-event"][0]["command"],
            "user-owned-hook"
        );
        assert_eq!(root["lume"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(root["lume"]["PreToolUse"][0]["hooks"][0]["timeout"], 5);
        assert!(root["lume"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .is_some_and(|command| command.contains("hook antigravity:PreToolUse")));
        assert_eq!(root["lume"]["PostToolUse"][0]["matcher"], "*");
        assert_eq!(root["lume"]["PostToolUse"][0]["hooks"][0]["timeout"], 5);
        assert!(root["lume"]["PostToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .is_some_and(|command| command.contains("hook antigravity:PostToolUse")));
        assert!(root["lume"]["PreInvocation"][0]["command"]
            .as_str()
            .is_some_and(|command| command.contains("hook antigravity:PreInvocation")));
        assert!(configured_content(
            &root.to_string(),
            &IntegrationKind::Antigravity,
            "/opt/Lume App/lume"
        ));

        apply_named_antigravity_hooks(
            &mut root,
            ANTIGRAVITY_LEGACY_HOOK_NAME,
            "/opt/Lume App/lume",
            false,
        )
        .expect("remove hooks Antigravity");
        assert_eq!(
            root["lume"]["custom-event"][0]["command"],
            "user-owned-hook"
        );
        assert_eq!(
            root["lume"]["PostInvocation"][0]["hooks"][0]["command"],
            "custom-tool hook antigravity:PostInvocation"
        );
        assert!(root["lume"].get("PreToolUse").is_none());
        assert!(root["lume"].get("PostToolUse").is_none());
        assert!(root.get("existing").is_some());
    }

    #[test]
    fn antigravity_hooks_are_scoped_to_cli_settings_and_migrate_legacy_hooks() {
        let home = Path::new("/home/test-user");
        assert_eq!(
            antigravity_cli_settings_path_for(home),
            home.join(".gemini/antigravity-cli/settings.json")
        );

        let mut cli_settings = json!({
            "model": "existing-model",
            "hooks": {
                "other-hook": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "other-cli-hook" }] }]
                },
                "lume-session-monitor": {
                    "custom-event": [{ "hooks": [{ "type": "command", "command": "custom-cli-hook" }] }]
                }
            }
        });
        apply_antigravity_cli_settings(&mut cli_settings, "/opt/Lume/lume", true)
            .expect("configura hooks exclusivos da CLI");
        assert!(configured_content(
            &cli_settings.to_string(),
            &IntegrationKind::Antigravity,
            "/opt/Lume/lume"
        ));
        assert_eq!(cli_settings["model"], "existing-model");
        assert_eq!(
            cli_settings["hooks"]["other-hook"]["Stop"][0]["hooks"][0]["command"],
            "other-cli-hook"
        );
        assert_eq!(
            cli_settings["hooks"][ANTIGRAVITY_HOOK_NAME]["custom-event"][0]["hooks"][0]["command"],
            "custom-cli-hook"
        );

        apply_antigravity_cli_settings(&mut cli_settings, "/opt/Lume/lume", false)
            .expect("desconecta hooks exclusivos da CLI");
        assert!(!has_lume_handler(
            &cli_settings.to_string(),
            &IntegrationKind::Antigravity
        ));
        assert_eq!(
            cli_settings["hooks"][ANTIGRAVITY_HOOK_NAME]["custom-event"][0]["hooks"][0]["command"],
            "custom-cli-hook"
        );
        assert_eq!(
            cli_settings["hooks"]["other-hook"]["Stop"][0]["hooks"][0]["command"],
            "other-cli-hook"
        );

        let mut malformed_settings = json!({ "hooks": false, "model": "keep" });
        let original = malformed_settings.clone();
        assert!(
            apply_antigravity_cli_settings(&mut malformed_settings, "/opt/Lume/lume", true)
                .is_err()
        );
        assert_eq!(malformed_settings, original);

        let mut shared_ide_hooks = json!({
            "existing-ide-hook": { "Stop": [{ "type": "command", "command": "notify-existing" }] },
            "lume": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{ "type": "command", "command": "/old/lume hook antigravity:PreToolUse" }]
                }],
                "custom-event": [{ "hooks": [{ "type": "command", "command": "user-owned-hook" }] }]
            }
        });
        apply_named_antigravity_hooks(
            &mut shared_ide_hooks,
            ANTIGRAVITY_LEGACY_HOOK_NAME,
            "/opt/Lume/lume",
            false,
        )
        .expect("remove só o hook legado do Lume");
        assert!(!has_lume_handler(
            &cli_settings.to_string(),
            &IntegrationKind::Antigravity
        ));
        assert_eq!(
            legacy_antigravity_hook_enabled(&shared_ide_hooks.to_string()),
            None
        );
        assert_eq!(
            shared_ide_hooks["existing-ide-hook"]["Stop"][0]["command"],
            "notify-existing"
        );
        assert_eq!(
            shared_ide_hooks["lume"]["custom-event"][0]["hooks"][0]["command"],
            "user-owned-hook"
        );
    }

    #[test]
    fn antigravity_refresh_preserves_explicitly_disabled_hooks() {
        let disabled_cli = json!({
            "hooks": {
                "lume-session-monitor": {
                    "enabled": false,
                    "PreToolUse": [{
                        "matcher": "*",
                        "hooks": [{
                            "type": "command",
                            "command": "/opt/Lume/lume hook antigravity:PreToolUse"
                        }]
                    }]
                }
            }
        })
        .to_string();
        let active_legacy = json!({
            "lume": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": "/old/lume hook antigravity:PreToolUse"
                    }]
                }]
            }
        })
        .to_string();
        let disabled_legacy = json!({
            "lume": {
                "enabled": false,
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": "/old/lume hook antigravity:PreToolUse"
                    }]
                }]
            }
        })
        .to_string();

        assert_eq!(
            antigravity_refresh_enabled(Some(&disabled_cli), Some(&active_legacy)),
            Some(false),
            "the CLI's explicit disabled setting wins during migration"
        );
        assert_eq!(
            antigravity_refresh_enabled(None, Some(&disabled_legacy)),
            Some(false),
            "a disabled legacy hook must not become an active CLI hook"
        );
        assert_eq!(
            antigravity_refresh_enabled(None, Some(&active_legacy)),
            Some(false),
            "migrating an old shared hook must not silently enable automatic tool approval"
        );
        let old_cli = json!({
            "hooks": {
                "lume-session-monitor": {
                    "enabled": true,
                    "PreToolUse": [{
                        "matcher": "*",
                        "hooks": [{
                            "type": "command",
                            "command": "/old/lume hook antigravity:PreToolUse"
                        }]
                    }]
                }
            }
        })
        .to_string();
        assert_eq!(
            antigravity_refresh_enabled(Some(&old_cli), None),
            Some(false),
            "older CLI hooks also require renewed consent"
        );
        let explicitly_approved_cli = json!({
            "hooks": {
                "lume-session-monitor": {
                    "enabled": true,
                    "PreToolUse": [{
                        "matcher": "*",
                        "hooks": [{
                            "type": "command",
                            "command": "/new/lume hook antigravity:PreToolUseAllow"
                        }]
                    }]
                }
            }
        })
        .to_string();
        assert_eq!(
            antigravity_refresh_enabled(Some(&explicitly_approved_cli), None),
            Some(true)
        );
        assert!(!configured_content(
            &disabled_cli,
            &IntegrationKind::Antigravity,
            "/opt/Lume/lume"
        ));

        let mut settings: Value = serde_json::from_str(&disabled_cli).expect("settings JSON");
        apply_antigravity_cli_settings(&mut settings, "/opt/Lume/lume", true)
            .expect("explicit connect enables Lume hook");
        assert_eq!(settings["hooks"][ANTIGRAVITY_HOOK_NAME]["enabled"], true);
        assert!(configured_content(
            &settings.to_string(),
            &IntegrationKind::Antigravity,
            "/opt/Lume/lume"
        ));
    }

    #[test]
    fn antigravity_file_migration_preserves_user_settings_and_creates_backups() {
        let root = std::env::temp_dir().join(format!(
            "lume-antigravity-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let cli_settings_path = root.join(".gemini/antigravity-cli/settings.json");
        let legacy_hooks_path = root.join(".gemini/config/hooks.json");
        fs::create_dir_all(cli_settings_path.parent().expect("CLI settings dir"))
            .expect("create CLI settings dir");
        fs::create_dir_all(legacy_hooks_path.parent().expect("legacy hooks dir"))
            .expect("create legacy hooks dir");
        let original_cli_settings = json!({
            "model": "user-selected-model",
            "hooks": {
                "third-party": {
                    "Stop": [{ "hooks": [{ "type": "command", "command": "user-hook" }] }]
                },
                "lume-session-monitor": {
                    "PreToolUse": [{
                        "matcher": "*",
                        "hooks": [{ "type": "command", "command": "/old/lume hook antigravity:PreToolUse" }]
                    }],
                    "custom-event": [{ "hooks": [{ "type": "command", "command": "user-custom-hook" }] }]
                }
            }
        });
        let original_legacy_hooks = json!({
            "third-party-ide": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "ide-hook" }] }]
            },
            "third-party-permissions": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{ "type": "command", "command": "user-permission-hook" }]
                }]
            },
            "lume": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": "/old/lume hook antigravity:PreToolUse"
                    }]
                }],
                "custom-event": [{ "hooks": [{ "type": "command", "command": "user-lume-hook" }] }]
            }
        });
        fs::write(
            &cli_settings_path,
            serde_json::to_vec(&original_cli_settings).expect("CLI settings JSON"),
        )
        .expect("write CLI settings");
        fs::write(
            &legacy_hooks_path,
            serde_json::to_vec(&original_legacy_hooks).expect("legacy hooks JSON"),
        )
        .expect("write legacy hooks");

        configure_antigravity_at_paths(
            &cli_settings_path,
            &legacy_hooks_path,
            "/opt/Lume/lume",
            true,
        )
        .expect("migrate and configure hooks");

        let migrated_cli: Value =
            serde_json::from_slice(&fs::read(&cli_settings_path).expect("read CLI settings"))
                .expect("migrated CLI JSON");
        let migrated_shared: Value =
            serde_json::from_slice(&fs::read(&legacy_hooks_path).expect("read legacy hooks"))
                .expect("migrated shared hooks JSON");
        assert_eq!(migrated_cli["model"], "user-selected-model");
        assert_eq!(
            migrated_cli["hooks"]["third-party"]["Stop"][0]["hooks"][0]["command"],
            "user-hook"
        );
        assert_eq!(
            migrated_cli["hooks"][ANTIGRAVITY_HOOK_NAME]["PreToolUse"][0]["matcher"],
            "*"
        );
        assert!(
            migrated_cli["hooks"][ANTIGRAVITY_HOOK_NAME]["PreToolUse"][0]["hooks"][0]["command"]
                .as_str()
                .is_some_and(|command| command.contains(r#"{"decision":"allow"}"#))
        );
        assert_eq!(
            migrated_cli["hooks"][ANTIGRAVITY_HOOK_NAME]["custom-event"][0]["hooks"][0]["command"],
            "user-custom-hook"
        );
        assert_eq!(
            migrated_cli["hooks"][ANTIGRAVITY_HOOK_NAME]["enabled"],
            true
        );
        assert_eq!(
            migrated_shared["third-party-ide"]["Stop"][0]["hooks"][0]["command"],
            "ide-hook"
        );
        assert_eq!(
            migrated_shared["third-party-permissions"]["PreToolUse"][0]["hooks"][0]["command"],
            "user-permission-hook"
        );
        assert_eq!(
            migrated_shared["lume"]["custom-event"][0]["hooks"][0]["command"],
            "user-lume-hook"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(
                &fs::read(cli_settings_path.with_extension("lume-backup.json"))
                    .expect("CLI settings backup"),
            )
            .expect("CLI backup JSON"),
            original_cli_settings
        );
        assert_eq!(
            serde_json::from_slice::<Value>(
                &fs::read(legacy_hooks_path.with_extension("lume-backup.json"))
                    .expect("shared hooks backup"),
            )
            .expect("shared backup JSON"),
            original_legacy_hooks
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn antigravity_migration_removes_shared_hook_even_when_cli_settings_are_invalid() {
        let root = std::env::temp_dir().join(format!(
            "lume-antigravity-invalid-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let cli_settings_path = root.join(".gemini/antigravity-cli/settings.json");
        let legacy_hooks_path = root.join(".gemini/config/hooks.json");
        fs::create_dir_all(cli_settings_path.parent().expect("CLI settings dir"))
            .expect("create CLI settings dir");
        fs::create_dir_all(legacy_hooks_path.parent().expect("legacy hooks dir"))
            .expect("create legacy hooks dir");

        let invalid_cli_settings = b"{invalid json";
        let original_legacy_hooks = json!({
            "third-party": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "user-hook" }] }]
            },
            "lume": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": "/old/lume hook antigravity:PreToolUse"
                    }]
                }]
            }
        });
        fs::write(&cli_settings_path, invalid_cli_settings).expect("write invalid settings");
        fs::write(
            &legacy_hooks_path,
            serde_json::to_vec(&original_legacy_hooks).expect("legacy hooks JSON"),
        )
        .expect("write legacy hooks");

        assert!(configure_antigravity_at_paths(
            &cli_settings_path,
            &legacy_hooks_path,
            "/opt/Lume/lume",
            true,
        )
        .is_err());

        assert_eq!(
            fs::read(&cli_settings_path).expect("invalid settings remain untouched"),
            invalid_cli_settings
        );
        let migrated_shared: Value =
            serde_json::from_slice(&fs::read(&legacy_hooks_path).expect("read legacy hooks"))
                .expect("migrated shared hooks JSON");
        assert!(migrated_shared.get("lume").is_none());
        assert_eq!(
            migrated_shared["third-party"]["Stop"][0]["hooks"][0]["command"],
            "user-hook"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(
                &fs::read(legacy_hooks_path.with_extension("lume-backup.json"))
                    .expect("shared hooks backup"),
            )
            .expect("shared backup JSON"),
            original_legacy_hooks
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn antigravity_migration_does_not_overwrite_invalid_shared_hooks() {
        let root = std::env::temp_dir().join(format!(
            "lume-antigravity-invalid-shared-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let cli_settings_path = root.join(".gemini/antigravity-cli/settings.json");
        let legacy_hooks_path = root.join(".gemini/config/hooks.json");
        fs::create_dir_all(cli_settings_path.parent().expect("CLI settings dir"))
            .expect("create CLI settings dir");
        fs::create_dir_all(legacy_hooks_path.parent().expect("legacy hooks dir"))
            .expect("create legacy hooks dir");

        let cli_settings = br#"{"model":"user-model"}"#;
        let invalid_shared_hooks = b"{invalid json";
        fs::write(&cli_settings_path, cli_settings).expect("write CLI settings");
        fs::write(&legacy_hooks_path, invalid_shared_hooks).expect("write invalid shared hooks");

        let error = configure_antigravity_at_paths(
            &cli_settings_path,
            &legacy_hooks_path,
            "/opt/Lume/lume",
            true,
        )
        .expect_err("invalid shared hooks need an explicit repair");

        assert!(error.contains("JSON inválido"));
        assert_eq!(
            fs::read(&legacy_hooks_path).expect("invalid shared hooks remain untouched"),
            invalid_shared_hooks
        );
        assert_eq!(
            fs::read(&cli_settings_path).expect("CLI settings remain untouched"),
            cli_settings
        );
        assert!(!legacy_hooks_path
            .with_extension("lume-backup.json")
            .exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn antigravity_status_warns_about_shared_hook_migration_without_changing_config() {
        let root = std::env::temp_dir().join(format!(
            "lume-antigravity-shared-warning-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let legacy_hooks_path = root.join(".gemini/config/hooks.json");
        fs::create_dir_all(legacy_hooks_path.parent().expect("legacy hooks dir"))
            .expect("create legacy hooks dir");

        let invalid_json = b"{invalid json";
        fs::write(&legacy_hooks_path, invalid_json).expect("write invalid shared hooks");
        let warning = antigravity_legacy_hook_warning_at_path(&legacy_hooks_path)
            .expect("invalid shared hooks are reported");
        assert!(warning.contains("JSON inválido"));
        assert_eq!(
            fs::read(&legacy_hooks_path).expect("shared hooks remain unchanged"),
            invalid_json
        );

        fs::write(
            &legacy_hooks_path,
            serde_json::to_vec(&json!({
                "lume": {
                    "PreToolUse": [{
                        "matcher": "*",
                        "hooks": [{
                            "type": "command",
                            "command": "/old/lume hook antigravity:PreToolUse"
                        }]
                    }]
                }
            }))
            .expect("legacy hooks JSON"),
        )
        .expect("write legacy hooks");
        assert!(antigravity_legacy_hook_warning_at_path(&legacy_hooks_path)
            .expect("active shared hook is reported")
            .contains("ainda está ativo"));

        fs::write(
            &legacy_hooks_path,
            serde_json::to_vec(&json!({
                "lume": {
                    "enabled": false,
                    "PreToolUse": [{
                        "matcher": "*",
                        "hooks": [{
                            "type": "command",
                            "command": "/old/lume hook antigravity:PreToolUse"
                        }]
                    }]
                }
            }))
            .expect("disabled legacy hooks JSON"),
        )
        .expect("write disabled legacy hooks");
        assert!(antigravity_legacy_hook_warning_at_path(&legacy_hooks_path).is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn disabled_legacy_antigravity_hook_is_removed_without_enabling_cli_hook() {
        let root = std::env::temp_dir().join(format!(
            "lume-antigravity-disabled-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let cli_settings_path = root.join(".gemini/antigravity-cli/settings.json");
        let legacy_hooks_path = root.join(".gemini/config/hooks.json");
        fs::create_dir_all(legacy_hooks_path.parent().expect("legacy hooks dir"))
            .expect("create legacy hooks dir");
        fs::write(
            &legacy_hooks_path,
            serde_json::to_vec(&json!({
                "lume": {
                    "enabled": false,
                    "PreToolUse": [{
                        "matcher": "*",
                        "hooks": [{
                            "type": "command",
                            "command": "/old/lume hook antigravity:PreToolUse"
                        }]
                    }]
                }
            }))
            .expect("legacy hooks JSON"),
        )
        .expect("write legacy hooks");

        configure_antigravity_at_paths(
            &cli_settings_path,
            &legacy_hooks_path,
            "/opt/Lume/lume",
            false,
        )
        .expect("migrate disabled hook");

        assert!(!cli_settings_path.exists());
        let migrated_shared: Value =
            serde_json::from_slice(&fs::read(&legacy_hooks_path).expect("read migrated hooks"))
                .expect("migrated shared hooks JSON");
        assert_eq!(migrated_shared["lume"]["enabled"], false);
        assert!(migrated_shared["lume"].get("PreToolUse").is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn antigravity_hook_commands_fail_open_when_lume_is_missing_or_returns_no_output() {
        for (provider, expected) in [
            ("antigravity:PreToolUseAllow", r#"{"decision":"allow"}"#),
            ("antigravity:PostToolUse", "{}"),
            ("antigravity:PreInvocation", "{}"),
            ("antigravity:PostInvocation", "{}"),
            ("antigravity:Stop", r#"{"decision":"allow"}"#),
        ] {
            let command = posix_fail_open_hook_command(
                "/definitely/missing/Lume App/lume",
                provider,
                antigravity_fallback_output(provider),
            );
            let output = Command::new("sh")
                .arg("-c")
                .arg(command)
                .output()
                .expect("executa fallback shell");

            assert!(output.status.success(), "{provider}");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                expected,
                "{provider}"
            );

            let command = posix_fail_open_hook_command(
                "/usr/bin/true",
                provider,
                antigravity_fallback_output(provider),
            );
            let output = Command::new("sh")
                .arg("-c")
                .arg(command)
                .output()
                .expect("executa fallback para saída vazia");

            assert!(output.status.success(), "{provider}");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                expected,
                "{provider} sem stdout"
            );
        }
    }

    #[test]
    fn windows_hook_command_is_a_fail_open_powershell_script() {
        let command = powershell_fail_open_hook_command(
            r"C:\Program Files\Lume\lume.exe",
            "antigravity:PreToolUseAllow",
            antigravity_fallback_output("antigravity:PreToolUseAllow"),
        );
        let script = decode_powershell_command(&command).expect("script codificado");

        assert!(command.starts_with("powershell.exe -NoLogo -NoProfile -NonInteractive"));
        assert!(script.contains("C:\\Program Files\\Lume\\lume.exe"));
        assert!(script.contains("antigravity:PreToolUseAllow"));
        assert!(script.contains(r#"{"decision":"allow"}"#));
        assert!(script.contains("$output = @("));
        assert!(script.contains("$payload.Length -gt 0"));
        assert!(script.contains("catch"));
        assert!(script.ends_with("exit 0"));
        assert!(command_mentions_lume_hook(
            &command,
            "antigravity:PreToolUseAllow"
        ));
        assert!(command_contains_executable_marker(
            &command,
            r"C:\Program Files\Lume\lume.exe",
            "antigravity:"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_hook_command_falls_back_when_lume_exits_successfully_without_stdout() {
        let root = std::env::temp_dir().join(format!(
            "lume-hook-empty-output-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("create hook test directory");
        let executable = root.join("Lume Hook Stub.cmd");
        fs::write(&executable, "@echo off\r\nexit /b 0\r\n").expect("write hook stub");
        let command = powershell_fail_open_hook_command(
            &executable.to_string_lossy(),
            "antigravity:PreToolUseAllow",
            antigravity_fallback_output("antigravity:PreToolUseAllow"),
        );
        let script = decode_powershell_command(&command).expect("decode PowerShell script");
        let output = Command::new("powershell.exe")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
            .arg(script)
            .output()
            .expect("run PowerShell hook wrapper");

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            r#"{"decision":"allow"}"#
        );
        fs::remove_dir_all(&root).expect("remove hook test directory");
    }

    #[test]
    fn enables_codex_hooks_without_replacing_other_features() {
        let content = "model = \"gpt\"\n[features]\nmemories = true\n\n[projects.test]\ntrust_level = \"trusted\"\n";
        let updated = config_with_hooks_enabled(content)
            .expect("configuração válida")
            .expect("mudança necessária");
        assert!(updated.contains("[features]\nhooks = true\nmemories = true"));
        assert!(updated.contains("[projects.test]"));
        assert!(config_with_hooks_enabled(&updated)
            .expect("configuração válida")
            .is_none());
    }

    #[test]
    fn respects_an_explicit_codex_hooks_disable() {
        let result = config_with_hooks_enabled("[features]\nhooks = false\n");
        assert!(result.is_err());
    }
}
