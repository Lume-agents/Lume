//! Exact private-CLI identity capture and opt-in observation diagnostics.
//! Nothing in this module reconciles sessions,
//! acquires a writer, talks to App Server, reads a PTY, or changes hook trust.
use std::{
    collections::HashMap,
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicI64, Ordering},
    time::Duration,
};

use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::{
    discovery,
    domain::{AgentKind, SessionSource},
    integrations::{self, IntegrationKind},
    state::now_millis,
};

const DATABASE: &str = "observations.sqlite3";
const APPLICATION_ID: u32 = 0x4c495031; // LIP1, distinct from the app/Codex databases.
const ENABLED: &str = "enabled";
const MAX_RECORDS: usize = 128;
const MAX_RECORD_BYTES: usize = 4096;
const MAX_DATABASE_BYTES: u64 = 2 * 1024 * 1024;
const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const ROOT_ACTIVITY_TTL_MS: i64 = 24 * 60 * 60 * 1000;
const IO_WAIT: Duration = Duration::from_millis(25);

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeStage {
    Discovery,
    State,
    Orb,
    Workspace,
}

impl RuntimeStage {
    fn index(self) -> usize {
        match self {
            Self::Discovery => 0,
            Self::State => 1,
            Self::Orb => 2,
            Self::Workspace => 3,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSnapshot {
    stage: RuntimeStage,
    host_pid: u32,
    observed_at: i64,
    cli_count: usize,
    cli_pids: Vec<u32>,
    failed: bool,
}

/// Opt-in, bounded metadata only. Never records names, prompts, command lines,
/// environments or control credentials, and never changes session state.
pub(crate) fn observe_runtime(stage: RuntimeStage, pids: &[u32], failed: bool) {
    static LAST_WRITE: [AtomicI64; 4] = [const { AtomicI64::new(0) }; 4];
    let Ok(directory) = state_directory() else {
        return;
    };
    if !enabled(&directory) {
        return;
    }
    let now = now_millis();
    if LAST_WRITE[stage.index()]
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
            (now < last || now.saturating_sub(last) >= 2_000).then_some(now)
        })
        .is_err()
    {
        return;
    }
    let mut unique_pids = pids.to_vec();
    unique_pids.sort_unstable();
    unique_pids.dedup();
    let snapshot = RuntimeSnapshot {
        stage,
        host_pid: std::process::id(),
        observed_at: now,
        cli_count: unique_pids.len(),
        cli_pids: unique_pids.into_iter().take(MAX_RECORDS).collect(),
        failed,
    };
    let _ = save_runtime(&directory, &snapshot);
}

fn save_runtime(directory: &Path, snapshot: &RuntimeSnapshot) -> Result<(), String> {
    if snapshot.cli_pids.len() > MAX_RECORDS {
        return Err("Runtime process list exceeds limit".into());
    }
    private_directory(directory)?;
    let path = directory.join(DATABASE);
    if !regular_file(&path, MAX_DATABASE_BYTES) {
        return Err("Probe database unavailable".into());
    }
    let payload = serde_json::to_string(snapshot).map_err(|error| error.to_string())?;
    if payload.len() > MAX_RECORD_BYTES {
        return Err("Runtime snapshot exceeds limit".into());
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    connection
        .busy_timeout(IO_WAIT)
        .map_err(|error| error.to_string())?;
    verify_database(&connection)?;
    connection.execute_batch(
        "PRAGMA max_page_count = 512;
         CREATE TABLE IF NOT EXISTS runtime_snapshots (stage INTEGER PRIMARY KEY, payload TEXT NOT NULL);",
    ).map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT OR REPLACE INTO runtime_snapshots (stage, payload) VALUES (?1, ?2)",
            params![snapshot.stage.index() as i64, payload],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn load_runtime(directory: &Path) -> Result<Vec<RuntimeSnapshot>, String> {
    let path = directory.join(DATABASE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    private_directory(directory)?;
    if !regular_file(&path, MAX_DATABASE_BYTES) {
        return Err("Probe database unavailable".into());
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    connection
        .busy_timeout(IO_WAIT)
        .map_err(|error| error.to_string())?;
    verify_database(&connection)?;
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'runtime_snapshots')",
        [], |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    if !exists {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare("SELECT payload FROM runtime_snapshots ORDER BY stage LIMIT 4")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    Ok(rows
        .filter_map(Result::ok)
        .filter(|payload| payload.len() <= MAX_RECORD_BYTES)
        .filter_map(|payload| serde_json::from_str::<RuntimeSnapshot>(&payload).ok())
        .filter(|snapshot| snapshot.cli_pids.len() <= MAX_RECORDS)
        .collect())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProcessIdentity {
    pub(crate) pid: u32,
    pub(crate) started_at: u64,
    // Linux clock ticks / Windows FILETIME / macOS microseconds disambiguate
    // PID reuse within a second. The boot marker also excludes previous boots.
    pub(crate) start_marker: Option<u64>,
    pub(crate) boot_marker: String,
}

impl ProcessIdentity {
    fn from_process(process: &Process) -> Option<Self> {
        (process.start_time() > 0).then(|| Self {
            pid: process.pid().as_u32(),
            started_at: process.start_time(),
            start_marker: process_start_marker(process.pid().as_u32()),
            boot_marker: boot_marker(),
        })
    }

    pub(crate) fn matches(&self, current: &Self) -> bool {
        self.pid > 0
            && self.started_at > 0
            && self.start_marker.is_some()
            && !self.boot_marker.is_empty()
            && self == current
    }

    pub(crate) fn discovered(pid: u32, started_at: u64) -> Option<Self> {
        let identity = Self {
            pid,
            started_at,
            start_marker: process_start_marker(pid),
            boot_marker: boot_marker(),
        };
        identity.matches(&identity).then_some(identity)
    }

    pub(crate) fn current(pid: u32) -> Option<Self> {
        let mut system = System::new();
        let pid = Pid::from_u32(pid);
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        Self::from_process(system.process(pid)?)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct HookLineage {
    cli: Option<ProcessIdentity>,
    servers: Vec<ProcessIdentity>,
    shared_server: bool,
    ambiguous_cli_ancestry: bool,
    editor_ancestor: bool,
    host_boundary: Option<HostBoundary>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum HostBoundary {
    Lume,
    Editor,
}

impl HookLineage {
    fn confirmed_cli(&self) -> Option<&ProcessIdentity> {
        // A shared server is never a CLI, even if its launcher happened to
        // have a Codex ancestor. Do not guess one of the server's clients.
        (!self.shared_server && !self.ambiguous_cli_ancestry)
            .then_some(self.cli.as_ref())
            .flatten()
    }

    fn process_context(&self) -> (Option<u32>, SessionSource) {
        if let Some(cli) = self.confirmed_cli() {
            (Some(cli.pid), SessionSource::Cli)
        } else if self.host_boundary == Some(HostBoundary::Editor)
            && !self.shared_server
            && self.cli.is_none()
        {
            (None, SessionSource::Vscode)
        } else {
            (None, SessionSource::Cli)
        }
    }
}

pub(crate) fn hook_process_context() -> (Option<u32>, SessionSource) {
    hook_lineage().process_context()
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum HookKind {
    SessionStart,
    UserPromptSubmit,
    Stop,
    SessionEnd,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StartSource {
    Startup,
    Resume,
    Clear,
    Compact,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    version: u8,
    thread_id: String,
    hook: HookKind,
    start_source: Option<StartSource>,
    observed_at: i64,
    lineage: HookLineage,
}

impl Observation {
    fn from_hook(raw: &Value, lineage: HookLineage, observed_at: i64) -> Option<Self> {
        // Subagent hooks carry the parent's session ID. They are not evidence
        // of a newly opened user CLI. Unknown start sources fail closed too.
        if raw
            .get("parent_session_id")
            .is_some_and(|value| !value.is_null())
            || raw
                .get("parent_thread_id")
                .is_some_and(|value| !value.is_null())
            || raw.get("is_subagent").and_then(Value::as_bool) == Some(true)
            || raw.get("source").is_some_and(Value::is_object)
            || raw.get("source").and_then(Value::as_str) == Some("subagent")
        {
            return None;
        }
        let hook = match raw.get("hook_event_name")?.as_str()? {
            "SessionStart" => HookKind::SessionStart,
            "UserPromptSubmit" => HookKind::UserPromptSubmit,
            "Stop" => HookKind::Stop,
            "SessionEnd" => HookKind::SessionEnd,
            _ => return None,
        };
        let start_source = if hook == HookKind::SessionStart {
            match raw.get("source").and_then(Value::as_str) {
                Some("startup") => Some(StartSource::Startup),
                Some("resume") => Some(StartSource::Resume),
                Some("clear") => Some(StartSource::Clear),
                Some("compact") => Some(StartSource::Compact),
                None if raw.get("source").is_none_or(Value::is_null) => None,
                _ => return None,
            }
        } else {
            None
        };
        let thread_id = raw.get("session_id")?.as_str()?;
        valid_thread_id(thread_id).then(|| Self {
            version: 1,
            thread_id: thread_id.to_ascii_lowercase(),
            hook,
            start_source,
            observed_at,
            lineage,
        })
    }

    fn valid(&self, now: i64) -> bool {
        self.version == 1
            && valid_thread_id(&self.thread_id)
            && self.observed_at > 0
            && self.observed_at <= now.saturating_add(1000)
            && self.observed_at >= now.saturating_sub(RETENTION_MS)
            && self.lineage.servers.len() <= 10
            && self
                .lineage
                .servers
                .iter()
                .chain(self.lineage.cli.iter())
                .all(|identity| {
                    identity.pid > 0
                        && identity.started_at > 0
                        && !identity.boot_marker.is_empty()
                        && identity.boot_marker.len() <= 64
                })
    }
}

pub(crate) fn valid_thread_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Normal hook capture is independent of opt-in diagnostics. Private CLI
/// ancestry may prove a binding; unbound root-turn hooks prove monitoring only.
/// The returned context reuses this ancestry scan when mapping the UI event.
pub(crate) fn observe_hook(provider: &str, raw: &Value) -> Option<(Option<u32>, SessionSource)> {
    if provider != "codex"
        || Observation::from_hook(raw, HookLineage::default(), now_millis()).is_none()
    {
        return None;
    }
    let lineage = hook_lineage();
    let context = lineage.process_context();
    let observation = Observation::from_hook(raw, lineage, now_millis())?;
    if let Ok(directory) = identity_directory() {
        let _ = save_cli_identity(&directory, &observation);
    }
    // No stdout/stderr: never inject diagnostic text into the model context,
    // hold a permission request, or make a failing recorder block a tool.
    if let Ok(directory) = state_directory() {
        if enabled(&directory) {
            let _ = save_observation(&directory, &observation);
        }
    }
    Some(context)
}

fn state_directory() -> Result<PathBuf, String> {
    if let Some(value) = env::var_os("LUME_IDENTITY_PROBE_STATE_DIR") {
        return Ok(PathBuf::from(value));
    }
    default_state_directory("identity-probe")
}

fn identity_directory() -> Result<PathBuf, String> {
    // A diagnostic --state-dir / opt-in flag must not select or disable the
    // production identity store, including when the desktop is not running.
    default_state_directory("cli-identity")
}

fn default_state_directory(name: &str) -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    if let Some(value) = env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(value).join("Lume").join(name));
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(value) = env::var_os("XDG_STATE_HOME") {
            return Ok(PathBuf::from(value).join("lume").join(name));
        }
        if let Some(value) = env::var_os("HOME") {
            return Ok(PathBuf::from(value).join(".local/state/lume").join(name));
        }
    }
    Err("Could not resolve the identity probe state directory".into())
}

fn regular_file(path: &Path, limit: u64) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() <= limit)
}

fn enabled(directory: &Path) -> bool {
    private_directory(directory).is_ok()
        && regular_file(&directory.join(ENABLED), 16)
        && fs::read(directory.join(ENABLED)).is_ok_and(|bytes| bytes == b"v1\n")
}

fn private_directory(directory: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(directory).map_err(|error| error.to_string())?;
    if !metadata.is_dir() {
        return Err("Probe directory must not be a symlink".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("Probe directory must be private (0700); use a dedicated directory".into());
        }
    }
    Ok(())
}

fn enable(directory: &Path) -> Result<(), String> {
    initialize_observation_store(directory)?;
    let flag = directory.join(ENABLED);
    // create_new rejects a dangling symlink too. No rewrite of Codex settings.
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&flag) {
        Ok(mut file) => file.write_all(b"v1\n").map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && enabled(directory) => {
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn initialize_observation_store(directory: &Path) -> Result<(), String> {
    if !directory.is_absolute() {
        return Err("Probe state directory must be absolute".into());
    }
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .map_err(|error| error.to_string())?;
    private_directory(directory)?;
    let database = directory.join(DATABASE);
    let existing_database = fs::symlink_metadata(&database).is_ok();
    if existing_database && !regular_file(&database, MAX_DATABASE_BYTES) {
        return Err("Probe database is not a bounded regular file".into());
    }
    if existing_database {
        let connection = Connection::open_with_flags(
            &database,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| error.to_string())?;
        connection
            .busy_timeout(IO_WAIT)
            .map_err(|error| error.to_string())?;
        return verify_database(&connection);
    }

    // Publish a fully initialized private database without replacing a winner
    // from another simultaneous CLI startup. A raw SQLite file must never be
    // visible to a second hook before its application ID/schema are ready.
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).map_err(|error| error.to_string())?;
    let temporary = directory.join(format!(
        ".identity-init-{:x}.sqlite3",
        Sha256::digest(nonce)
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| {
        let connection = Connection::open_with_flags(
            &temporary,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| error.to_string())?;
        connection
            .busy_timeout(IO_WAIT)
            .map_err(|error| error.to_string())?;
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch(
                "PRAGMA max_page_count = 512;
             CREATE TABLE observations (
               identity TEXT PRIMARY KEY, observed_at INTEGER NOT NULL, payload TEXT NOT NULL
             );",
            )
            .map_err(|error| error.to_string())?;
        drop(connection);
        match fs::hard_link(&temporary, &database) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Another creator won. Verify it; never overwrite its records.
                initialize_observation_store(directory)
            }
            Err(error) => Err(error.to_string()),
        }
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn save_cli_identity(directory: &Path, observation: &Observation) -> Result<(), String> {
    if observation.lineage.confirmed_cli().is_none() && !root_monitor_lifecycle(observation) {
        // A loaded thread or SessionStart alone is not an open conversation.
        return Ok(());
    }
    if !directory.join(DATABASE).exists() {
        initialize_observation_store(directory)?;
    } else {
        private_directory(directory)?;
    }
    save_observation(directory, observation)
}

fn save_observation(directory: &Path, observation: &Observation) -> Result<(), String> {
    if !observation.valid(now_millis()) {
        return Err("Invalid probe observation".into());
    }
    let payload = serde_json::to_string(observation).map_err(|error| error.to_string())?;
    if payload.len() > MAX_RECORD_BYTES {
        return Err("Probe observation exceeds limit".into());
    }
    let path = directory.join(DATABASE);
    if !regular_file(&path, MAX_DATABASE_BYTES) {
        return Err("Probe database unavailable".into());
    }
    let mut connection = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    connection
        .busy_timeout(IO_WAIT)
        .map_err(|error| error.to_string())?;
    verify_database(&connection)?;
    connection
        .execute_batch("PRAGMA max_page_count = 512;")
        .map_err(|error| error.to_string())?;
    // De-duplicate repeated lifecycle hooks for the same process incarnation.
    // Distinct threads / incarnations remain distinct; no cwd/time guessing.
    let identity = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                &observation.thread_id,
                observation.hook,
                &observation.lineage,
            ))
            .map_err(|error| error.to_string())?
        )
    );
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction.execute(
        "INSERT INTO observations (identity, observed_at, payload) VALUES (?1, ?2, ?3)
         ON CONFLICT(identity) DO UPDATE SET observed_at = excluded.observed_at, payload = excluded.payload
         WHERE excluded.observed_at >= observations.observed_at",
        params![identity, observation.observed_at, payload],
    ).map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM observations WHERE observed_at < ?1 OR rowid NOT IN
         (SELECT rowid FROM observations ORDER BY observed_at DESC, rowid DESC LIMIT ?2)",
            params![
                now_millis().saturating_sub(RETENTION_MS),
                MAX_RECORDS as i64
            ],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn load_observations(directory: &Path, now: i64) -> Result<Vec<Observation>, String> {
    let path = directory.join(DATABASE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    private_directory(directory)?;
    if !regular_file(&path, MAX_DATABASE_BYTES) {
        return Err("Probe database is not a bounded regular file".into());
    }
    let connection = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    connection
        .busy_timeout(IO_WAIT)
        .map_err(|error| error.to_string())?;
    verify_database(&connection)?;
    let mut statement = connection
        .prepare(
            "SELECT payload FROM observations WHERE observed_at >= ?1
         ORDER BY observed_at DESC, rowid DESC LIMIT ?2",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            params![now.saturating_sub(RETENTION_MS), MAX_RECORDS as i64],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?;
    Ok(rows
        .filter_map(Result::ok)
        .filter(|payload| payload.len() <= MAX_RECORD_BYTES)
        .filter_map(|payload| serde_json::from_str::<Observation>(&payload).ok())
        .filter(|observation| observation.valid(now))
        .collect())
}

fn verify_database(connection: &Connection) -> Result<(), String> {
    let id: u32 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if id != APPLICATION_ID {
        return Err("This database does not belong to the identity probe".into());
    }
    Ok(())
}

fn arguments(process: &Process) -> Vec<String> {
    process
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy().to_lowercase())
        .collect()
}

fn host_boundary(name: &str, args: &[String]) -> Option<HostBoundary> {
    if matches!(name, "lume" | "lume.exe") {
        Some(HostBoundary::Lume)
    } else if matches!(
        name,
        "code" | "code.exe" | "code-insiders" | "code-insiders.exe"
    ) || (args.iter().any(|part| part.contains(".vscode/extensions"))
        && discovery::detect_agent_arguments(name, args) != Some(AgentKind::Codex)
        && !discovery::is_codex_infrastructure_arguments(name, args))
    {
        Some(HostBoundary::Editor)
    } else {
        None
    }
}

fn hook_lineage() -> HookLineage {
    let mut system = System::new();
    let mut pid = Pid::from_u32(std::process::id());
    let mut lineage = HookLineage::default();
    for index in 0..11 {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .without_tasks(),
        );
        let Some(process) = system.process(pid) else {
            break;
        };
        if index > 0 {
            let name = process.name().to_string_lossy().to_lowercase();
            let args = arguments(process);
            if let Some(boundary) = host_boundary(&name, &args) {
                // An app/editor can itself have been launched by an unrelated
                // Codex CLI. Never walk through that host to borrow its parent
                // CLI's identity for a managed/extension conversation. A real
                // TUI found BEFORE the boundary remains a valid candidate.
                lineage.editor_ancestor |= boundary == HostBoundary::Editor;
                lineage.host_boundary = Some(boundary);
                break;
            }
            if discovery::is_codex_infrastructure_arguments(&name, &args) {
                if let Some(identity) = ProcessIdentity::from_process(process) {
                    lineage.servers.push(identity);
                }
                lineage.shared_server |= args
                    .iter()
                    .any(|part| part == "--managed-daemon" || part == "pid-update-loop");
            } else if discovery::detect_agent_arguments(&name, &args) == Some(AgentKind::Codex) {
                if let Some(identity) = ProcessIdentity::from_process(process) {
                    // Multiple nested CLIs / wrappers are evidence to inspect,
                    // not permission to pick the outermost conversation.
                    lineage.ambiguous_cli_ancestry |= lineage
                        .cli
                        .as_ref()
                        .is_some_and(|existing| existing.pid != identity.pid);
                    lineage.cli = Some(identity);
                }
            }
            lineage.editor_ancestor |= matches!(name.as_str(), "code" | "code.exe")
                || args.iter().any(|part| part.contains(".vscode/extensions"));
        }
        let Some(parent) = process.parent() else {
            break;
        };
        pid = parent;
    }
    lineage
}

#[cfg(target_os = "linux")]
fn process_start_marker(pid: u32) -> Option<u64> {
    let mut stat = String::new();
    fs::File::open(format!("/proc/{pid}/stat"))
        .ok()?
        .take(4096)
        .read_to_string(&mut stat)
        .ok()?;
    // comm may contain spaces and parentheses; fields after its final ')' start
    // at field 3 (state). Field 22 is the process start clock tick.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

#[cfg(target_os = "windows")]
fn process_start_marker(pid: u32) -> Option<u64> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut created = std::mem::zeroed::<FILETIME>();
        let mut exited = std::mem::zeroed::<FILETIME>();
        let mut kernel = std::mem::zeroed::<FILETIME>();
        let mut user = std::mem::zeroed::<FILETIME>();
        let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
        CloseHandle(handle);
        (ok != 0)
            .then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn process_start_marker(pid: u32) -> Option<u64> {
    use std::mem::{size_of, MaybeUninit};

    let native_pid = libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 0)?;
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = size_of::<libc::proc_bsdinfo>();
    // SAFETY: the kernel receives an aligned buffer of the exact declared
    // struct size. Its contents are inspected only after a complete response.
    let written = unsafe {
        libc::proc_pidinfo(
            native_pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as libc::c_int,
        )
    };
    if written != size as libc::c_int {
        return None;
    }
    // SAFETY: proc_pidinfo initialized the entire proc_bsdinfo above.
    let info = unsafe { info.assume_init() };
    // SAFETY: geteuid has no arguments or memory preconditions.
    let current_uid = unsafe { libc::geteuid() };
    if info.pbi_pid != pid
        || info.pbi_uid != current_uid
        || info.pbi_start_tvsec == 0
        || info.pbi_start_tvusec >= 1_000_000
    {
        return None;
    }
    info.pbi_start_tvsec
        .checked_mul(1_000_000)?
        .checked_add(info.pbi_start_tvusec)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn process_start_marker(_pid: u32) -> Option<u64> {
    None
}

fn boot_marker() -> String {
    #[cfg(target_os = "linux")]
    if let Ok(value) = fs::read_to_string("/proc/sys/kernel/random/boot_id") {
        let value = value.trim();
        if valid_thread_id(value) {
            return value.to_string();
        }
    }
    format!("boot:{}", System::boot_time())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliReport {
    process: ProcessIdentity,
    source: SessionSource,
    association: &'static str,
    verified_thread_id: Option<String>,
    thread_name: Option<String>,
    launch_thread_candidates: Vec<String>,
    // This is the CURRENT detector's heuristic, deliberately not a verified
    // binding. The prototype does not feed this value back into AppState.
    detector_thread_candidates: Vec<String>,
}

fn cli_report(
    process: ProcessIdentity,
    source: SessionSource,
    launch_thread_candidates: Vec<String>,
    detector_thread_candidates: Vec<String>,
    observations: &[Observation],
    names: &HashMap<String, String>,
) -> CliReport {
    let latest_time = observations
        .iter()
        .filter(|item| {
            item.lineage
                .confirmed_cli()
                .is_some_and(|old| old.matches(&process))
        })
        .map(|item| item.observed_at)
        .max();
    let latest = observations
        .iter()
        .filter(|item| {
            Some(item.observed_at) == latest_time
                && item
                    .lineage
                    .confirmed_cli()
                    .is_some_and(|old| old.matches(&process))
        })
        .collect::<Vec<_>>();
    let (association, verified_thread_id) = if let Some(first) = latest.first() {
        if latest
            .iter()
            .any(|other| other.thread_id != first.thread_id)
        {
            ("ambiguous_hook_observations", None)
        } else if latest
            .iter()
            .any(|other| other.hook == HookKind::SessionEnd)
        {
            ("session_end_observed", None)
        } else {
            ("verified_hook_ancestry", Some(first.thread_id.clone()))
        }
    } else if launch_thread_candidates.len() > 1 || detector_thread_candidates.len() > 1 {
        ("ambiguous_candidates", None)
    } else if !launch_thread_candidates.is_empty() {
        ("launch_argument_only", None)
    } else if !detector_thread_candidates.is_empty() {
        ("detector_candidate_only", None)
    } else {
        ("unresolved", None)
    };
    let thread_name = verified_thread_id
        .as_ref()
        .and_then(|id| names.get(id))
        .cloned();
    CliReport {
        process,
        source,
        association,
        verified_thread_id,
        thread_name,
        launch_thread_candidates,
        detector_thread_candidates,
    }
}

/// Positive CLI ancestry captured at startup also works when Lume starts later.
/// Shared-daemon observations deliberately remain unbound.
pub(crate) fn confirmed_cli_bindings(live: &[ProcessIdentity]) -> Vec<(ProcessIdentity, String)> {
    if live.is_empty() {
        return Vec::new();
    }
    let Ok(directory) = identity_directory() else {
        return Vec::new();
    };
    let diagnostic_directory = state_directory().ok();
    let observations = binding_observations(&directory, diagnostic_directory.as_deref());
    confirmed_bindings_from_observations(live, &observations)
}

fn binding_observations(identity: &Path, diagnostics: Option<&Path>) -> Vec<Observation> {
    let mut observations = load_observations(identity, now_millis()).unwrap_or_default();
    // Compatibility with already captured prototype evidence. Disabling the
    // probe no longer disables normal bindings, and never enables recording.
    if let Some(directory) = diagnostics {
        if enabled(&directory) {
            observations.extend(load_observations(&directory, now_millis()).unwrap_or_default());
        }
    }
    observations
}

fn root_monitor_lifecycle(observation: &Observation) -> bool {
    observation.lineage.host_boundary != Some(HostBoundary::Lume)
        && observation.lineage.process_context().1 == SessionSource::Cli
        && matches!(
            observation.hook,
            HookKind::UserPromptSubmit | HookKind::Stop | HookKind::SessionEnd
        )
}

/// Restore visibility evidence only. No sessions, native writers or process
/// bindings are created here, and loaded daemon metadata is never consulted.
pub(crate) fn recent_root_hook_activity() -> Vec<(String, i64)> {
    let Ok(identity) = identity_directory() else {
        return Vec::new();
    };
    let diagnostics = state_directory().ok();
    let observations = binding_observations(&identity, diagnostics.as_deref());
    root_hook_activity_from_observations(&observations, now_millis())
}

fn root_hook_activity_from_observations(
    observations: &[Observation],
    now: i64,
) -> Vec<(String, i64)> {
    let mut latest = HashMap::<String, (i64, i64, i64)>::new();
    for observation in observations.iter().filter(|observation| {
        observation.valid(now)
            && observation.observed_at <= now
            && now.saturating_sub(observation.observed_at) <= ROOT_ACTIVITY_TTL_MS
            && root_monitor_lifecycle(observation)
    }) {
        let entry = latest.entry(observation.thread_id.clone()).or_default();
        match observation.hook {
            HookKind::UserPromptSubmit => entry.0 = entry.0.max(observation.observed_at),
            HookKind::Stop => entry.1 = entry.1.max(observation.observed_at),
            HookKind::SessionEnd => entry.2 = entry.2.max(observation.observed_at),
            HookKind::SessionStart => {}
        }
    }
    latest
        .into_iter()
        .filter_map(|(id, (prompt, stop, end))| {
            // A delayed completion cannot resurrect an ended conversation.
            // Only a new prompt reopens it; an end also wins timestamp ties.
            (end == 0 || prompt > end).then_some((id, prompt.max(stop)))
        })
        .collect()
}

fn confirmed_bindings_from_observations(
    live: &[ProcessIdentity],
    observations: &[Observation],
) -> Vec<(ProcessIdentity, String)> {
    live.iter()
        .filter_map(|process| {
            let report = cli_report(
                process.clone(),
                SessionSource::Cli,
                Vec::new(),
                Vec::new(),
                observations,
                &HashMap::new(),
            );
            report.verified_thread_id.map(|id| (process.clone(), id))
        })
        .collect()
}

fn report(directory: &Path) -> Result<Value, String> {
    let now = now_millis();
    let observations = load_observations(directory, now)?;
    // Default diagnostics also explain normal identity recovery. An explicitly
    // isolated --state-dir report must not include the user's normal records.
    let normal_observations = if state_directory().is_ok_and(|path| path == directory) {
        identity_directory()
            .ok()
            .and_then(|path| load_observations(&path, now).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let identity_observations = observations
        .iter()
        .chain(&normal_observations)
        .cloned()
        .collect::<Vec<_>>();
    let mut system = System::new();
    let discovered = discovery::read_only_process_snapshot(&mut system);
    // Uses the existing read-only, WAL-aware index cache, not thread/resume or
    // full-history hydration. Only resolved names leave this function.
    let names = integrations::indexed_session_names(&IntegrationKind::Codex).unwrap_or_default();
    let mut clis = discovered
        .into_iter()
        .filter(|item| item.agent == AgentKind::Codex)
        .filter_map(|item| {
            let process = system.process(Pid::from_u32(item.process_id))?;
            let identity = ProcessIdentity::from_process(process)?;
            Some(cli_report(
                identity,
                item.source,
                discovery::native_session_ids_from_command(process.cmd()),
                item.native_session_ids,
                &identity_observations,
                &names,
            ))
        })
        .collect::<Vec<_>>();
    clis.sort_by_key(|item| (item.process.started_at, item.process.pid));
    let hooks = observations
        .iter()
        .map(|item| {
            let state = match item.lineage.confirmed_cli() {
                None if item.lineage.shared_server => "shared_server_unbound",
                None if item.lineage.ambiguous_cli_ancestry => "ambiguous_cli_ancestry",
                None if item.lineage.host_boundary == Some(HostBoundary::Lume) => {
                    "lume_host_unbound"
                }
                None if item.lineage.host_boundary == Some(HostBoundary::Editor) => {
                    "editor_host_unbound"
                }
                None => "native_thread_only",
                Some(old) if old.start_marker.is_none() => "precise_process_birth_unavailable",
                Some(old) => match system
                    .process(Pid::from_u32(old.pid))
                    .and_then(ProcessIdentity::from_process)
                {
                    Some(current) if current.start_marker.is_none() => {
                        "precise_process_birth_unavailable"
                    }
                    Some(current) if old.matches(&current) => "live_process",
                    Some(_) => "pid_reused_or_new_boot",
                    None => "process_not_visible",
                },
            };
            json!({"observation": item, "processState": state})
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "version": 1, "mode": "observation_only", "enabled": enabled(directory),
        "stateDirectory": directory, "generatedAt": now,
        "cliCount": clis.len(), "clis": clis, "hookObservations": hooks,
        "normalIdentityObservationCount": normal_observations.len(),
        "desktopRuntime": load_runtime(directory)?,
        "daemonObserver": crate::codex_daemon_observer::observe(),
        "hookTrust": "not_inspected_use_codex_slash_hooks",
        "limitations": [
            "No CLI-to-thread association is inferred from a shared server, cwd, timestamps or loaded threads.",
            "Launch arguments and existing detector candidates may be stale after switching threads.",
            "Normal identity capture is independent of diagnostic recording; missing evidence can mean untrusted hooks or a CLI opened before this build.",
            "Verified associations require a precise OS process-start marker (implemented for Linux and Windows).",
            "Process visibility depends on the OS and the permissions of the user running this report."
        ]
    }))
}

pub(crate) fn run_cli(arguments: &[String]) -> i32 {
    let result = (|| -> Result<Value, String> {
        let action = arguments.first().map(String::as_str).unwrap_or("status");
        let directory = match &arguments[arguments.len().min(1)..] {
            [] => state_directory()?,
            [flag, path] if flag == "--state-dir" && !path.is_empty() => PathBuf::from(path),
            _ => return Err("Usage: lume identity-probe [enable|disable|status] [--state-dir <private-directory>]".into()),
        };
        match action {
            "enable" => { enable(&directory)?; Ok(json!({"enabled": true, "stateDirectory": directory, "mode": "observation_only"})) }
            "disable" => {
                if directory.exists() {
                    private_directory(&directory)?;
                    let flag = directory.join(ENABLED);
                    if fs::symlink_metadata(&flag).is_ok() && !enabled(&directory) {
                        return Err("Unrecognized enable flag; refusing to remove it".into());
                    }
                    match fs::remove_file(directory.join(ENABLED)) {
                        Ok(()) => {},
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                        Err(error) => return Err(error.to_string()),
                    }
                }
                Ok(json!({"enabled": false, "observationsRetained": true, "stateDirectory": directory}))
            }
            "status" => report(&directory),
            _ => Err("Usage: lume identity-probe [enable|disable|status] [--state-dir <private-directory>]".into()),
        }
    })();
    match result {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
            0
        }
        Err(error) => {
            eprintln!("Identity probe: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const THREAD_A: &str = "019f8061-7032-7521-b333-84f84c744fa8";
    const THREAD_B: &str = "01a0d079-9807-7b02-bba9-31792b1aa275";

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let mut nonce = [0u8; 16];
            getrandom::getrandom(&mut nonce).unwrap();
            let path =
                env::temp_dir().join(format!("lume-identity-probe-{:x}", Sha256::digest(nonce)));
            enable(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn runtime_diagnostics_keep_only_four_bounded_metadata_snapshots() {
        let directory = TestDirectory::new();
        assert!(load_runtime(&directory.0).unwrap().is_empty());
        for host_pid in [42, 43] {
            for stage in [
                RuntimeStage::Discovery,
                RuntimeStage::State,
                RuntimeStage::Orb,
                RuntimeStage::Workspace,
            ] {
                save_runtime(
                    &directory.0,
                    &RuntimeSnapshot {
                        stage,
                        host_pid,
                        observed_at: now_millis(),
                        cli_count: 2,
                        cli_pids: vec![10, 11],
                        failed: false,
                    },
                )
                .unwrap();
            }
        }
        let snapshots = load_runtime(&directory.0).unwrap();
        assert_eq!(snapshots.len(), 4);
        assert!(snapshots.iter().all(|snapshot| snapshot.host_pid == 43));
        assert!(load_observations(&directory.0, now_millis())
            .unwrap()
            .is_empty());
        let oversized = RuntimeSnapshot {
            stage: RuntimeStage::State,
            host_pid: 43,
            observed_at: now_millis(),
            cli_count: MAX_RECORDS + 1,
            cli_pids: vec![10; MAX_RECORDS + 1],
            failed: false,
        };
        assert!(save_runtime(&directory.0, &oversized).is_err());
        assert_eq!(load_runtime(&directory.0).unwrap().len(), 4);
    }

    fn identity(pid: u32) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            started_at: 123,
            start_marker: Some(456),
            boot_marker: "test-boot".into(),
        }
    }
    fn observation(id: &str, at: i64) -> Observation {
        Observation::from_hook(
            &json!({"hook_event_name": "SessionStart", "source": "resume", "session_id": id}),
            HookLineage {
                cli: Some(identity(42)),
                ..Default::default()
            },
            at,
        )
        .unwrap()
    }
    fn test_report(items: &[Observation]) -> CliReport {
        cli_report(
            identity(42),
            SessionSource::Cli,
            vec![],
            vec![],
            items,
            &HashMap::new(),
        )
    }

    #[test]
    fn current_cli_origin_is_not_the_editor_host_or_historical_origin() {
        let lineage = HookLineage {
            cli: Some(identity(42)),
            editor_ancestor: true,
            host_boundary: Some(HostBoundary::Editor),
            ..Default::default()
        };
        assert_eq!(lineage.process_context(), (Some(42), SessionSource::Cli));
        let shared = HookLineage {
            shared_server: true,
            host_boundary: None,
            ..lineage.clone()
        };
        assert_eq!(shared.process_context(), (None, SessionSource::Cli));
        let shared_editor = HookLineage {
            host_boundary: Some(HostBoundary::Editor),
            ..shared
        };
        assert_eq!(shared_editor.process_context(), (None, SessionSource::Cli));
        let editor = HookLineage {
            cli: None,
            ..lineage
        };
        assert_eq!(editor.process_context(), (None, SessionSource::Vscode));
        let managed = HookLineage {
            host_boundary: Some(HostBoundary::Lume),
            ..Default::default()
        };
        assert_eq!(managed.process_context(), (None, SessionSource::Cli));
    }

    #[test]
    fn metadata_whitelist_never_persists_messages_commands_paths_or_permissions() {
        let raw = json!({"hook_event_name": "UserPromptSubmit", "session_id": THREAD_A,
            "prompt": "secret-prompt", "cwd": "/secret/project", "tool_input": {"command": "secret-command"},
            "transcript_path": "/secret/transcript", "permission_mode": "secret-permission", "token": "secret-token"});
        let item = Observation::from_hook(&raw, HookLineage::default(), now_millis()).unwrap();
        let serialized = serde_json::to_string(&item).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(serialized.contains(THREAD_A));
    }

    #[test]
    fn ignores_tool_permission_subagent_and_unknown_start_events() {
        for raw in [
            json!({"hook_event_name": "PermissionRequest", "session_id": THREAD_A}),
            json!({"hook_event_name": "PostToolUse", "session_id": THREAD_A}),
            json!({"hook_event_name": "SubagentStart", "session_id": THREAD_A}),
            json!({"hook_event_name": "SessionStart", "source": "subagent", "session_id": THREAD_A}),
            json!({"hook_event_name": "SessionStart", "source": {"subagent": {}}, "session_id": THREAD_A}),
            json!({"hook_event_name": "SessionStart", "parent_session_id": THREAD_B, "session_id": THREAD_A}),
            json!({"hook_event_name": "UserPromptSubmit", "parent_thread_id": THREAD_B, "session_id": THREAD_A}),
            json!({"hook_event_name": "Stop", "source": "subagent", "session_id": THREAD_A}),
            json!({"hook_event_name": "UserPromptSubmit", "is_subagent": true, "session_id": THREAD_A}),
            json!({"hook_event_name": "SessionStart", "session_id": "not-a-native-thread"}),
        ] {
            assert!(Observation::from_hook(&raw, HookLineage::default(), now_millis()).is_none());
        }
    }

    #[test]
    fn shared_server_never_becomes_a_cli_association() {
        let mut item = observation(THREAD_A, now_millis());
        item.lineage.shared_server = true;
        item.lineage.servers.push(identity(99));
        assert!(item.lineage.confirmed_cli().is_none());
        assert_eq!(test_report(&[item]).association, "unresolved");
    }

    #[test]
    fn nested_cli_ancestry_or_unavailable_precise_birth_is_not_verified() {
        let mut item = observation(THREAD_A, now_millis());
        item.lineage.ambiguous_cli_ancestry = true;
        assert_eq!(test_report(&[item.clone()]).association, "unresolved");
        item.lineage.ambiguous_cli_ancestry = false;
        item.lineage.cli.as_mut().unwrap().start_marker = None;
        assert_eq!(test_report(&[item]).association, "unresolved");
        let incomplete = ProcessIdentity {
            start_marker: None,
            ..identity(42)
        };
        assert!(!incomplete.matches(&incomplete));
    }

    #[test]
    fn app_and_editor_hosts_are_boundaries_not_clis() {
        assert_eq!(host_boundary("lume", &[]), Some(HostBoundary::Lume));
        assert_eq!(host_boundary("code.exe", &[]), Some(HostBoundary::Editor));
        assert_eq!(
            host_boundary(
                "node",
                &["/home/test/.vscode/extensions/openai.chatgpt/server.js".into()]
            ),
            Some(HostBoundary::Editor)
        );
        assert_eq!(
            host_boundary("codex", &["codex".into(), "resume".into()]),
            None
        );
        assert_eq!(
            host_boundary(
                "codex",
                &[
                    "/home/test/.vscode/extensions/openai.chatgpt/bin/codex".into(),
                    "resume".into(),
                ]
            ),
            None
        );
        let mut item = observation(THREAD_A, now_millis());
        item.lineage.host_boundary = Some(HostBoundary::Editor);
        // A genuine CLI already seen beneath the editor is its integrated TUI,
        // not a guessed session from above the editor boundary.
        assert_eq!(
            test_report(&[item.clone()]).association,
            "verified_hook_ancestry"
        );
        item.lineage.cli = None;
        assert_eq!(test_report(&[item]).association, "unresolved");
    }

    #[test]
    fn thread_from_another_process_or_reused_pid_is_not_associated() {
        let now = now_millis();
        for old in [
            ProcessIdentity {
                pid: 99,
                ..identity(42)
            },
            ProcessIdentity {
                started_at: 124,
                ..identity(42)
            },
            ProcessIdentity {
                start_marker: Some(457),
                ..identity(42)
            },
            ProcessIdentity {
                boot_marker: "another-boot".into(),
                ..identity(42)
            },
        ] {
            let mut item = observation(THREAD_A, now);
            item.lineage.cli = Some(old);
            assert_eq!(test_report(&[item]).association, "unresolved");
        }
    }

    #[test]
    fn latest_hook_supersedes_launch_argument_without_mutating_sessions() {
        let now = now_millis();
        let report = cli_report(
            identity(42),
            SessionSource::Cli,
            vec![THREAD_A.into()],
            vec![],
            &[observation(THREAD_A, now - 10), observation(THREAD_B, now)],
            &HashMap::from([(THREAD_B.into(), "Current title".into())]),
        );
        assert_eq!(report.association, "verified_hook_ancestry");
        assert_eq!(report.verified_thread_id.as_deref(), Some(THREAD_B));
        assert_eq!(report.thread_name.as_deref(), Some("Current title"));
    }

    #[test]
    fn launch_arguments_and_detector_candidates_are_not_proof_of_current_thread() {
        let report = cli_report(
            identity(42),
            SessionSource::Cli,
            vec![THREAD_A.into()],
            vec![THREAD_A.into()],
            &[],
            &HashMap::new(),
        );
        assert_eq!(report.association, "launch_argument_only");
        assert!(report.verified_thread_id.is_none());
    }

    #[test]
    fn simultaneous_conflicting_observations_and_session_end_fail_closed() {
        let now = now_millis();
        assert_eq!(
            test_report(&[observation(THREAD_A, now), observation(THREAD_B, now)]).association,
            "ambiguous_hook_observations"
        );
        let mut ended = observation(THREAD_A, now);
        ended.hook = HookKind::SessionEnd;
        assert!(test_report(&[ended]).verified_thread_id.is_none());
    }

    #[test]
    fn automatic_bindings_recover_startup_metadata_without_a_prompt_or_launch_uuid() {
        let directory = TestDirectory::new();
        let now = now_millis();
        // This startup record predates the desktop, with no prompt/turn event.
        save_observation(&directory.0, &observation(THREAD_A, now - 1_000)).unwrap();
        let records = load_observations(&directory.0, now).unwrap();
        let bindings =
            confirmed_bindings_from_observations(&[identity(42), identity(43)], &records);
        assert_eq!(bindings, vec![(identity(42), THREAD_A.into())]);
        let switched = confirmed_bindings_from_observations(
            &[identity(42)],
            &[records[0].clone(), observation(THREAD_B, now)],
        );
        assert_eq!(switched, vec![(identity(42), THREAD_B.into())]);
        assert!(confirmed_bindings_from_observations(&[], &records).is_empty());
    }

    #[test]
    fn normal_identity_recovery_works_for_two_clis_with_diagnostics_disabled() {
        let directory = TestDirectory::new();
        fs::remove_file(directory.0.join(ENABLED)).unwrap();
        let diagnostics = TestDirectory::new();
        fs::remove_file(diagnostics.0.join(ENABLED)).unwrap();
        let now = now_millis();
        let first = observation(THREAD_A, now - 1_000);
        let mut second = observation(THREAD_B, now - 500);
        second.lineage.cli = Some(identity(43));
        save_cli_identity(&directory.0, &first).unwrap();
        save_cli_identity(&directory.0, &second).unwrap();

        let records = binding_observations(&directory.0, Some(&diagnostics.0));
        assert_eq!(
            confirmed_bindings_from_observations(&[identity(42), identity(43)], &records),
            vec![
                (identity(42), THREAD_A.into()),
                (identity(43), THREAD_B.into())
            ]
        );
        assert!(!enabled(&directory.0));
        assert!(!enabled(&diagnostics.0));
        assert!(load_observations(&diagnostics.0, now).unwrap().is_empty());

        let mut ended = second;
        ended.hook = HookKind::SessionEnd;
        ended.observed_at = now;
        save_cli_identity(&directory.0, &ended).unwrap();
        let records = binding_observations(&directory.0, None);
        assert_eq!(
            confirmed_bindings_from_observations(&[identity(42), identity(43)], &records),
            vec![(identity(42), THREAD_A.into())]
        );
    }

    #[test]
    fn normal_identity_capture_never_creates_a_store_for_shared_or_ambiguous_hooks() {
        let directory = TestDirectory::new();
        let absent = directory.0.join("must-not-be-created");
        let mut item = observation(THREAD_A, now_millis());
        item.lineage.shared_server = true;
        save_cli_identity(&absent, &item).unwrap();
        assert!(!absent.exists());
        item.lineage.shared_server = false;
        item.lineage.ambiguous_cli_ancestry = true;
        save_cli_identity(&absent, &item).unwrap();
        assert!(!absent.exists());
        item.lineage.ambiguous_cli_ancestry = false;
        item.lineage.cli = None;
        save_cli_identity(&absent, &item).unwrap();
        assert!(!absent.exists());
    }

    #[test]
    fn root_turn_capture_survives_without_diagnostics_but_never_binds_a_shared_cli() {
        let directory = TestDirectory::new();
        let normal = directory.0.join("normal-root-monitor");
        let now = now_millis();
        let mut item = observation(THREAD_A, now);
        item.hook = HookKind::UserPromptSubmit;
        item.lineage.shared_server = true;
        item.lineage.cli = None;
        save_cli_identity(&normal, &item).unwrap();
        assert!(!normal.join(ENABLED).exists());
        let records = binding_observations(&normal, None);
        assert_eq!(
            root_hook_activity_from_observations(&records, now),
            vec![(THREAD_A.into(), now)]
        );
        assert!(confirmed_bindings_from_observations(&[identity(42)], &records).is_empty());
    }

    #[test]
    fn root_monitor_recovery_ignores_compaction_and_respects_end_and_new_prompt() {
        let now = now_millis();
        let mut prompt = observation(THREAD_A, now - 20);
        prompt.hook = HookKind::UserPromptSubmit;
        let mut compact = observation(THREAD_A, now - 10);
        compact.start_source = Some(StartSource::Compact);
        assert_eq!(
            root_hook_activity_from_observations(&[compact.clone(), prompt.clone()], now),
            vec![(THREAD_A.into(), now - 20)]
        );
        let mut ended = prompt.clone();
        ended.hook = HookKind::SessionEnd;
        // End wins ties and is not undone by a later compact SessionStart.
        for records in [
            vec![prompt.clone(), ended.clone(), compact.clone()],
            vec![ended.clone(), prompt.clone(), compact],
        ] {
            assert!(root_hook_activity_from_observations(&records, now).is_empty());
        }
        let mut late_stop = ended.clone();
        late_stop.hook = HookKind::Stop;
        late_stop.observed_at = now - 1;
        assert!(root_hook_activity_from_observations(&[ended.clone(), late_stop], now).is_empty());
        prompt.observed_at = now;
        assert_eq!(
            root_hook_activity_from_observations(&[ended, prompt], now),
            vec![(THREAD_A.into(), now)]
        );
    }

    #[test]
    fn root_monitor_recovery_is_bounded_and_does_not_borrow_managed_or_editor_hosts() {
        let now = now_millis();
        let mut prompt = observation(THREAD_A, now);
        prompt.hook = HookKind::Stop;
        for lineage in [
            HookLineage {
                host_boundary: Some(HostBoundary::Lume),
                ..Default::default()
            },
            HookLineage {
                host_boundary: Some(HostBoundary::Editor),
                ..Default::default()
            },
        ] {
            prompt.lineage = lineage;
            assert!(root_hook_activity_from_observations(&[prompt.clone()], now).is_empty());
        }
        prompt.lineage = HookLineage::default();
        for at in [now + 1, now - ROOT_ACTIVITY_TTL_MS - 1] {
            prompt.observed_at = at;
            assert!(root_hook_activity_from_observations(&[prompt.clone()], now).is_empty());
        }
        assert!(
            root_hook_activity_from_observations(&[observation(THREAD_A, now)], now).is_empty()
        );
    }

    #[test]
    fn normal_identity_capture_initializes_without_enabling_diagnostics() {
        let directory = TestDirectory::new();
        let identity_store = directory.0.join("normal-cli-identity");
        save_cli_identity(&identity_store, &observation(THREAD_A, now_millis())).unwrap();
        assert!(identity_store.join(DATABASE).is_file());
        assert!(!identity_store.join(ENABLED).exists());
        assert_eq!(
            load_observations(&identity_store, now_millis())
                .unwrap()
                .len(),
            1
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&identity_store).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(identity_store.join(DATABASE))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn simultaneous_first_cli_hooks_do_not_see_a_partially_initialized_store() {
        let directory = TestDirectory::new();
        let identity_store = directory.0.join("concurrent-first-startups");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers = [THREAD_A, THREAD_B]
            .into_iter()
            .enumerate()
            .map(|(index, id)| {
                let path = identity_store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut item = observation(id, now_millis());
                    item.lineage.cli = Some(identity(42 + index as u32));
                    barrier.wait();
                    save_cli_identity(&path, &item)
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        let records = binding_observations(&identity_store, None);
        assert_eq!(records.len(), 2);
        assert_eq!(
            confirmed_bindings_from_observations(&[identity(42), identity(43)], &records).len(),
            2
        );
        assert!(!identity_store.join(ENABLED).exists());
        assert_eq!(fs::read_dir(&identity_store).unwrap().count(), 1);
    }

    #[test]
    fn automatic_bindings_reject_daemon_ambiguity_pid_reuse_and_closed_threads() {
        let now = now_millis();
        let mut shared = observation(THREAD_A, now);
        shared.lineage.shared_server = true;
        let mut ended = observation(THREAD_A, now);
        ended.hook = HookKind::SessionEnd;
        for records in [
            vec![shared],
            vec![ended],
            vec![observation(THREAD_A, now), observation(THREAD_B, now)],
        ] {
            assert!(confirmed_bindings_from_observations(&[identity(42)], &records).is_empty());
        }
        for current in [
            ProcessIdentity {
                pid: 43,
                ..identity(42)
            },
            ProcessIdentity {
                start_marker: Some(457),
                ..identity(42)
            },
            ProcessIdentity {
                boot_marker: "new-boot".into(),
                ..identity(42)
            },
        ] {
            assert!(confirmed_bindings_from_observations(
                &[current],
                &[observation(THREAD_A, now)]
            )
            .is_empty());
        }
    }

    #[test]
    fn records_work_without_desktop_or_socket_and_are_deduplicated_and_bounded() {
        let directory = TestDirectory::new();
        let now = now_millis();
        let item = observation(THREAD_A, now);
        save_observation(&directory.0, &item).unwrap();
        save_observation(&directory.0, &item).unwrap();
        assert_eq!(load_observations(&directory.0, now).unwrap().len(), 1);
        for index in 1..160 {
            let mut item = observation(THREAD_A, now);
            item.lineage.cli = Some(identity(index));
            save_observation(&directory.0, &item).unwrap();
        }
        assert_eq!(
            load_observations(&directory.0, now).unwrap().len(),
            MAX_RECORDS
        );
        assert!(fs::metadata(directory.0.join(DATABASE)).unwrap().len() <= MAX_DATABASE_BYTES);
    }

    #[test]
    fn expires_old_evidence_and_rejects_future_records() {
        let directory = TestDirectory::new();
        let now = now_millis();
        save_observation(&directory.0, &observation(THREAD_A, now)).unwrap();
        assert!(load_observations(&directory.0, now + RETENTION_MS + 1)
            .unwrap()
            .is_empty());
        assert!(save_observation(&directory.0, &observation(THREAD_A, now + 60_000)).is_err());
    }

    #[test]
    fn late_write_does_not_overwrite_newer_evidence_for_the_same_lifecycle() {
        let directory = TestDirectory::new();
        let now = now_millis();
        save_observation(&directory.0, &observation(THREAD_A, now)).unwrap();
        save_observation(&directory.0, &observation(THREAD_A, now - 100)).unwrap();
        let records = load_observations(&directory.0, now).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].observed_at, now);
    }

    #[test]
    fn disable_retains_records_and_report_read_does_not_create_a_database() {
        let directory = TestDirectory::new();
        let now = now_millis();
        save_observation(&directory.0, &observation(THREAD_A, now)).unwrap();
        fs::remove_file(directory.0.join(ENABLED)).unwrap();
        assert!(!enabled(&directory.0));
        assert_eq!(load_observations(&directory.0, now).unwrap().len(), 1);
        let nonexistent = directory.0.join("not-created");
        assert!(load_observations(&nonexistent, now).unwrap().is_empty());
        assert!(!nonexistent.exists());
    }

    #[test]
    fn locked_recorder_fails_quickly_instead_of_blocking_the_agent() {
        let directory = TestDirectory::new();
        let connection = Connection::open(directory.0.join(DATABASE)).unwrap();
        connection.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let before = std::time::Instant::now();
        assert!(save_observation(&directory.0, &observation(THREAD_A, now_millis())).is_err());
        assert!(before.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn refuses_to_modify_an_unrelated_database_in_the_selected_directory() {
        let directory = TestDirectory::new();
        let connection = Connection::open(directory.0.join(DATABASE)).unwrap();
        connection
            .pragma_update(None, "application_id", 17)
            .unwrap();
        let before = fs::read(directory.0.join(DATABASE)).unwrap();
        assert!(enable(&directory.0).is_err());
        assert!(save_observation(&directory.0, &observation(THREAD_A, now_millis())).is_err());
        assert!(load_observations(&directory.0, now_millis()).is_err());
        assert_eq!(fs::read(directory.0.join(DATABASE)).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn private_permissions_and_symlink_rejection() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory = TestDirectory::new();
        assert_eq!(
            fs::metadata(&directory.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.0.join(DATABASE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let linked = directory.0.join("linked-directory");
        symlink(&directory.0, &linked).unwrap();
        assert!(enable(&linked).is_err());
        fs::remove_file(directory.0.join(ENABLED)).unwrap();
        symlink(directory.0.join(DATABASE), directory.0.join(ENABLED)).unwrap();
        assert!(!enabled(&directory.0));
        assert!(enable(&directory.0).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn kernel_start_marker_handles_real_process_stat() {
        assert!(process_start_marker(std::process::id()).is_some_and(|value| value > 0));
        assert!(!boot_marker().is_empty());
    }
}
