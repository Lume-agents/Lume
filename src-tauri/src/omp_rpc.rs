use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    domain::{AgentKind, HookEvent, HookEventKind, SessionControlOrigin, SessionSource},
    event_server,
    state::AppState,
};

type EventPublisher = Arc<dyn Fn(HookEvent) + Send + Sync>;
type RawSender = Arc<dyn Fn(Value) + Send + Sync>;

/// Ordinary commands answer at once; compaction runs a model turn before it responds.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const COMPACT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_RPC_FRAME_BYTES: usize = 1_048_576;
const MAX_RPC_REASSEMBLED_BYTES: usize = 67_108_864;

struct RpcChunk {
    id: String,
    count: usize,
    next: usize,
    length: usize,
    bytes: Vec<u8>,
}

fn decode_rpc_frame(
    frame: Value,
    pending: &mut Option<RpcChunk>,
) -> Result<Option<Value>, &'static str> {
    if frame["type"] != "rpc_chunk" {
        if pending.is_some() {
            return Err("Interrupted omp RPC chunk sequence");
        }
        return Ok(Some(frame));
    }
    let id = frame["chunkId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("Missing omp RPC chunk id")?;
    let index = frame["index"]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or("Invalid omp RPC chunk index")?;
    let count = frame["count"]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .filter(|&n| (1..=MAX_RPC_REASSEMBLED_BYTES).contains(&n))
        .ok_or("Invalid omp RPC chunk count")?;
    let length = frame["byteLength"]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .filter(|&n| n <= MAX_RPC_REASSEMBLED_BYTES)
        .ok_or("Invalid omp RPC chunk length")?;
    let data = frame["data"]
        .as_str()
        .ok_or("Invalid omp RPC chunk payload")?;
    if index == 0 {
        if pending.is_some() {
            return Err("Interleaved omp RPC chunk sequence");
        }
        *pending = Some(RpcChunk {
            id: id.to_string(),
            count,
            next: 0,
            length,
            bytes: Vec::with_capacity(length),
        });
    }
    let chunk = pending.as_mut().ok_or("Orphaned omp RPC chunk")?;
    if chunk.id != id || chunk.count != count || chunk.length != length || chunk.next != index {
        return Err("Out-of-order omp RPC chunk sequence");
    }
    if data.len() > MAX_RPC_FRAME_BYTES
        || data.len()
            > (length.saturating_sub(chunk.bytes.len()).saturating_add(2) / 3)
                .saturating_mul(4)
                .saturating_add(4)
    {
        return Err("Oversized omp RPC chunk");
    }
    STANDARD
        .decode_vec(data, &mut chunk.bytes)
        .map_err(|_| "Invalid omp RPC chunk encoding")?;
    if chunk.bytes.len() > chunk.length {
        return Err("Oversized omp RPC reassembly");
    }
    chunk.next += 1;
    if chunk.next != chunk.count {
        return Ok(None);
    }
    let chunk = pending.take().expect("chunk exists");
    if chunk.bytes.len() != chunk.length {
        return Err("Truncated omp RPC reassembly");
    }
    serde_json::from_slice(&chunk.bytes)
        .map(Some)
        .map_err(|_| "Invalid omp RPC reassembly")
}

#[derive(Clone)]
pub struct OmpRpc {
    sessions: Arc<Mutex<HashMap<String, Handle>>>,
    start_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    state: AppState,
    publish: EventPublisher,
}

/// One child per session. Requests lock only their own runtime, and answers to approvals and
/// questions write through `input` directly, so a slow command never blocks another session
/// or a pending decision.
#[derive(Clone)]
struct Handle {
    runtime: Arc<Mutex<Runtime>>,
    input: Arc<Mutex<ChildStdin>>,
}

struct Runtime {
    child: Child,
    input: Arc<Mutex<ChildStdin>>,
    responses: mpsc::Receiver<Value>,
    next_id: u64,
    native_id: String,
    session_key: Arc<Mutex<String>>,
    cwd: String,
    profile: Option<String>,
    idle_since: Option<Instant>,
    /// Set before Lume kills the child, so the reader does not report the exit as a crash.
    stopped: Arc<AtomicBool>,
    readers: Vec<thread::JoinHandle<()>>,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.child.kill();
        let _ = self.child.wait();
        for handle in self.readers.drain(..) {
            let _ = handle.join();
        }
    }
}

impl OmpRpc {
    pub fn new(state: AppState, app: AppHandle) -> Self {
        let publish_state = state.clone();
        let publish: EventPublisher = Arc::new(move |event| {
            let _ = event_server::publish_event(&publish_state, &app, event);
        });
        Self::with_publisher(state, publish)
    }

    fn with_publisher(state: AppState, publish: EventPublisher) -> Self {
        let bridge = Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            start_locks: Arc::new(Mutex::new(HashMap::new())),
            state,
            publish,
        };
        let monitor = bridge.clone();
        let _ = thread::Builder::new()
            .name("lume-omp-idle-timeout".into())
            .spawn(move || loop {
                thread::sleep(Duration::from_secs(30));
                monitor.expire_idle_sessions();
            });
        bridge
    }

    fn session_start_lock(&self, session_id: &str) -> Result<Arc<Mutex<()>>, String> {
        let mut locks = self
            .start_locks
            .lock()
            .map_err(|_| "omp bridge start locks poisoned")?;
        Ok(locks
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }

    fn handle(&self, session_id: &str) -> Result<Handle, String> {
        self.sessions
            .lock()
            .map_err(|_| "omp bridge lock poisoned")?
            .get(session_id)
            .cloned()
            .ok_or_else(|| "omp RPC session is not running".to_string())
    }

    pub fn start(
        &self,
        session_id: &str,
        cwd: &str,
        native_id: Option<&str>,
        profile: Option<&str>,
        approval_mode: Option<&str>,
    ) -> Result<String, String> {
        let session_lock = self.session_start_lock(session_id)?;
        let _guard = session_lock
            .lock()
            .map_err(|_| "omp session start lock poisoned")?;
        self.start_locked(session_id, cwd, native_id, profile, approval_mode)
    }

    fn start_locked(
        &self,
        session_id: &str,
        cwd: &str,
        native_id: Option<&str>,
        profile: Option<&str>,
        approval_mode: Option<&str>,
    ) -> Result<String, String> {
        let cwd = std::fs::canonicalize(cwd)
            .map_err(|e| format!("Project directory is unavailable: {e}"))?;
        let mut command = crate::executables::command("omp")?;
        command.args(["--mode", "rpc-ui"]);
        if let Some(profile) = profile.filter(|v| !v.is_empty()) {
            command.args(["--profile", profile]);
        }
        if let Some(mode) = approval_mode.filter(|v| !v.is_empty()) {
            command.args(["--approval-mode", mode]);
        }
        if let Some(id) = native_id {
            command.args(["--resume", id]);
        }
        command
            .current_dir(&cwd)
            .env("LUME_CONTROLLED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not launch omp RPC: {e}"))?;
        let (Some(input), Some(output), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("omp stdio unavailable".into());
        };
        let (frames_tx, frames_rx) = mpsc::channel();
        let stdout_handle = thread::spawn(move || {
            let mut pending = None;
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if line.len() > MAX_RPC_FRAME_BYTES {
                    break;
                }
                let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                    break;
                };
                match decode_rpc_frame(frame, &mut pending) {
                    Ok(Some(frame)) => {
                        if frames_tx.send(frame).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });
        // Keep draining stderr for the child's whole life (a closed pipe would fail its writes);
        // the last lines explain a startup failure ("No models available.") or a crash.
        let stderr_tail = Arc::new(Mutex::new(Vec::<String>::new()));
        let stderr_lines = stderr_tail.clone();
        let stderr_handle = thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Ok(mut lines) = stderr_lines.lock() {
                    lines.push(line);
                    let excess = lines.len().saturating_sub(3);
                    lines.drain(..excess);
                }
            }
        });
        let stderr_text = |tail: &Arc<Mutex<Vec<String>>>| {
            tail.lock()
                .map(|lines| lines.join("\n").trim().to_string())
                .unwrap_or_default()
        };
        let ready = match frames_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(frame) if is_ready_frame(&frame) => frame,
            Ok(frame) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(format!("omp RPC did not become ready: {frame}"));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                let stderr = stderr_text(&stderr_tail);
                return Err(if stderr.is_empty() {
                    "omp RPC exited before ready".into()
                } else {
                    stderr
                });
            }
        };
        let input = Arc::new(Mutex::new(input));
        let raw_sender_input = input.clone();
        let raw_sender: RawSender = Arc::new(move |frame| {
            let _ = send_raw(&raw_sender_input, frame);
        });
        let (events_tx, events_rx) = mpsc::channel();
        let session_key = Arc::new(Mutex::new(session_id.to_string()));
        let stopped = Arc::new(AtomicBool::new(false));
        let reader_session_key = session_key.clone();
        let reader_publish = self.publish.clone();
        let reader_state = self.state.clone();
        let reader_stopped = stopped.clone();
        let reader_raw_sender = raw_sender.clone();
        let reader_stderr_tail = stderr_tail.clone();
        let response_sender = events_tx.clone();
        let rpc_reader_handle = match thread::Builder::new()
            .name("lume-omp-rpc-reader".into())
            .spawn(move || {
                for frame in frames_rx {
                    if frame["type"] == "response" {
                        let _ = response_sender.send(frame);
                        continue;
                    }
                    map_frame(
                        &reader_session_key,
                        &frame,
                        &reader_state,
                        &reader_publish,
                        Some(&reader_raw_sender),
                    );
                }
                // stdout closed without Lume stopping the child: omp exited on its own.
                if !reader_stopped.load(Ordering::SeqCst) {
                    let session_id = reader_session_key
                        .lock()
                        .map(|id| id.clone())
                        .unwrap_or_default();
                    let mut event = base_event(&session_id);
                    event.event = HookEventKind::Failed;
                    let stderr = stderr_text(&reader_stderr_tail);
                    event.status_label = Some(if stderr.is_empty() {
                        "O omp foi encerrado inesperadamente".into()
                    } else {
                        stderr
                    });
                    reader_publish(event);
                }
            }) {
            Ok(handle) => handle,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(e.to_string());
            }
        };
        let mut runtime = Runtime {
            child,
            input: input.clone(),
            responses: events_rx,
            next_id: 1,
            native_id: native_id.unwrap_or("").to_string(),
            session_key,
            cwd: cwd.to_string_lossy().to_string(),
            profile: profile.map(str::to_string),
            idle_since: None,
            stopped,
            readers: vec![stdout_handle, stderr_handle, rpc_reader_handle],
        };
        if ready["supportedProtocolVersions"]
            .as_array()
            .is_some_and(|versions| versions.iter().any(|version| version.as_u64() == Some(2)))
        {
            if let Err(error) = self.request(
                &mut runtime,
                "negotiate_protocol",
                json!({"protocolVersion": 2}),
            ) {
                drop(runtime);
                return Err(error);
            }
        }
        if let Err(err) = self.request(
            &mut runtime,
            "set_subagent_subscription",
            json!({"level":"events"}),
        ) {
            drop(runtime);
            let stderr = stderr_text(&stderr_tail);
            let err_msg = if !stderr.is_empty() { stderr } else { err };
            return Err(err_msg);
        }
        let state = match self.request(&mut runtime, "get_state", json!({})) {
            Ok(state) => state,
            Err(err) => {
                drop(runtime);
                let stderr = stderr_text(&stderr_tail);
                let err_msg = if !stderr.is_empty() { stderr } else { err };
                return Err(err_msg);
            }
        };
        runtime.native_id = state["sessionId"]
            .as_str()
            .or(native_id)
            .unwrap_or("")
            .to_string();
        let id = runtime.native_id.clone();
        self.sessions
            .lock()
            .map_err(|_| "omp bridge lock poisoned")?
            .insert(
                session_id.to_string(),
                Handle {
                    runtime: Arc::new(Mutex::new(runtime)),
                    input,
                },
            );
        Ok(id)
    }

    pub fn prompt(&self, session_id: &str, text: &str, delivery: &str) -> Result<(), String> {
        self.ensure_started(session_id)?;
        let handle = self.handle(session_id)?;
        let mut runtime = handle
            .runtime
            .lock()
            .map_err(|_| "omp RPC session lock poisoned")?;
        runtime.idle_since = None;
        let ty = prompt_command(delivery);
        self.request(&mut runtime, ty, json!({"message":text}))
            .map(|_| ())
    }

    pub fn command(&self, session_id: &str, ty: &str, args: Value) -> Result<Value, String> {
        self.ensure_started(session_id)?;
        let handle = self.handle(session_id)?;
        let mut runtime = handle
            .runtime
            .lock()
            .map_err(|_| "omp RPC session lock poisoned")?;
        self.request(&mut runtime, ty, args)
    }

    /// Respawns the child of a Lume-controlled session after a Lume restart or idle expiry.
    fn ensure_started(&self, session_id: &str) -> Result<(), String> {
        if self.handle(session_id).is_ok() {
            return Ok(());
        }
        let session_lock = self.session_start_lock(session_id)?;
        let _guard = session_lock
            .lock()
            .map_err(|_| "omp session start lock poisoned")?;
        if self.handle(session_id).is_ok() {
            return Ok(());
        }
        let session = self.state.connected_session(session_id)?;
        if session.agent != AgentKind::Omp || session.control_origin != SessionControlOrigin::Lume {
            return Err("Session is not controlled by the omp RPC bridge".into());
        }
        let cwd = session
            .working_directory
            .as_deref()
            .ok_or("omp session has no project directory")?;
        let native_id = session
            .native_session_id
            .as_deref()
            .ok_or("omp session has no native id")?;
        // Profile sessions are labelled "Oh My Pi · <profile>" when they start.
        let profile = session
            .agent_label
            .strip_prefix("Oh My Pi · ")
            .map(str::to_string);
        let (mode, _) = approval_mode(&session.permission_profile.mode);
        self.start_locked(
            session_id,
            cwd,
            Some(native_id),
            profile.as_deref(),
            Some(mode),
        )
        .map(|_| ())
    }

    pub fn rekey(&self, old_id: &str, new_id: &str) -> Result<(), String> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "omp bridge lock poisoned")?;
        if sessions.contains_key(new_id) {
            return Err("An omp RPC session already uses this native id".into());
        }
        let handle = sessions
            .remove(old_id)
            .ok_or("omp RPC session is not running")?;
        if let Ok(runtime) = handle.runtime.lock() {
            if let Ok(mut key) = runtime.session_key.lock() {
                *key = new_id.to_string();
            }
        }
        sessions.insert(new_id.to_string(), handle);
        if let Ok(mut locks) = self.start_locks.lock() {
            locks.remove(old_id);
        }
        Ok(())
    }

    pub fn answer_ui_request(
        &self,
        session_id: &str,
        request_id: &str,
        value: Value,
    ) -> Result<(), String> {
        let frame = match value {
            Value::Bool(b) => json!({"type":"extension_ui_response","id":request_id,"confirmed":b}),
            Value::Null => json!({"type":"extension_ui_response","id":request_id,"cancelled":true}),
            Value::Object(map) if map.contains_key("type") => Value::Object(map),
            Value::Object(mut map) => {
                map.insert("type".into(), json!("extension_ui_response"));
                map.insert("id".into(), json!(request_id));
                Value::Object(map)
            }
            v => json!({"type":"extension_ui_response","id":request_id,"value":v}),
        };
        send_raw(&self.handle(session_id)?.input, frame)
    }

    pub fn restart_approval_mode(
        &self,
        session_id: &str,
        approval_mode: &str,
    ) -> Result<(), String> {
        let handle = self.handle(session_id)?;
        let (native_id, cwd, profile) = {
            let mut runtime = handle
                .runtime
                .lock()
                .map_err(|_| "omp RPC session lock poisoned")?;
            let state = self.request(&mut runtime, "get_state", json!({}))?;
            if let Some(native_id) = state["sessionId"].as_str() {
                runtime.native_id = native_id.to_string();
            }
            if state["isSettled"] != true || !queue_is_empty(&state) {
                return Err("Permission mode can only change when the omp session is settled and its queue is empty".into());
            }
            (
                runtime.native_id.clone(),
                runtime.cwd.clone(),
                runtime.profile.clone(),
            )
        };
        self.sessions
            .lock()
            .map_err(|_| "omp bridge lock poisoned")?
            .remove(session_id);
        // Dropping the last handle kills the old child before the new one resumes the session.
        drop(handle);
        self.start(
            session_id,
            &cwd,
            Some(&native_id),
            profile.as_deref(),
            Some(approval_mode),
        )?;
        Ok(())
    }

    pub fn answer_question(
        &self,
        session_id: &str,
        question_id: &str,
        answers: &[crate::domain::QuestionAnswer],
    ) -> Result<(), String> {
        let mut parts = question_id.splitn(3, ':');
        if parts.next() != Some("omp-ui") {
            return Err("Invalid omp question id".into());
        }
        let method = parts.next().ok_or("Invalid omp question id")?;
        let request_id = parts.next().ok_or("Invalid omp question id")?;
        let pending = if method == "ask" {
            self.state.connected_session(session_id)?.pending_question
        } else {
            None
        };
        let value = question_response(method, request_id, answers, pending.as_ref());
        send_raw(&self.handle(session_id)?.input, value)
    }

    pub fn answer_approval(
        &self,
        session_id: &str,
        request_id: &str,
        action: crate::domain::PermissionAction,
    ) -> Result<(), String> {
        send_raw(
            &self.handle(session_id)?.input,
            approval_response(request_id, action),
        )
    }

    pub fn close(&self, session_id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(session_id);
        }
        if let Ok(mut locks) = self.start_locks.lock() {
            locks.remove(session_id);
        }
    }

    pub fn terminate_all(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.clear();
        }
        if let Ok(mut locks) = self.start_locks.lock() {
            locks.clear();
        }
    }

    fn expire_idle_sessions(&self) {
        let timeout_minutes = match self.state.preferences() {
            Ok(preferences) => preferences.omp_idle_timeout_minutes,
            Err(_) => return,
        };
        if timeout_minutes == 0 {
            return;
        }
        let handles = match self.sessions.lock() {
            Ok(sessions) => sessions
                .iter()
                .map(|(id, handle)| (id.clone(), handle.clone()))
                .collect::<Vec<_>>(),
            Err(_) => return,
        };
        for (session_id, handle) in handles {
            let completed = self
                .state
                .connected_session(&session_id)
                .is_ok_and(|session| session.status == crate::domain::SessionStatus::Completed);
            // A session busy with a command is not idle; check it on the next pass.
            let Ok(mut runtime) = handle.runtime.try_lock() else {
                continue;
            };
            if !completed {
                runtime.idle_since = None;
                continue;
            }
            let Ok(snapshot) = self.request(&mut runtime, "get_state", json!({})) else {
                continue;
            };
            let queue_empty = queue_is_empty(&snapshot);
            if !queue_empty || snapshot["isSettled"] != true {
                runtime.idle_since = None;
                continue;
            }
            let idle_since = *runtime.idle_since.get_or_insert_with(Instant::now);
            if idle_timeout_expired(true, queue_empty, timeout_minutes, idle_since.elapsed()) {
                drop(runtime);
                if let Ok(mut sessions) = self.sessions.lock() {
                    sessions.remove(&session_id);
                }
                if let Ok(mut locks) = self.start_locks.lock() {
                    locks.remove(&session_id);
                }
            }
        }
    }

    fn request(&self, runtime: &mut Runtime, ty: &str, args: Value) -> Result<Value, String> {
        runtime.next_id += 1;
        let id = format!("lume-{}", runtime.next_id);
        let mut frame = json!({"id":id,"type":ty});
        if let (Some(dst), Some(src)) = (frame.as_object_mut(), args.as_object()) {
            dst.extend(src.clone());
        }
        send_raw(&runtime.input, frame)?;
        let timeout = if ty == "compact" {
            COMPACT_TIMEOUT
        } else {
            REQUEST_TIMEOUT
        };
        loop {
            let response = runtime
                .responses
                .recv_timeout(timeout)
                .map_err(|e| match e {
                    mpsc::RecvTimeoutError::Timeout => format!("omp RPC command {ty} timed out"),
                    mpsc::RecvTimeoutError::Disconnected => {
                        "omp RPC process disconnected".to_string()
                    }
                })?;
            if response["id"] == id {
                if response["success"] == false {
                    return Err(response["error"]
                        .as_str()
                        .unwrap_or("omp RPC command failed")
                        .to_string());
                }
                return Ok(response["data"].clone());
            }
        }
    }
}

fn queue_is_empty(state: &Value) -> bool {
    ["steering", "followUp"].iter().all(|queue| {
        state["queuedMessages"][queue]
            .as_array()
            .is_none_or(Vec::is_empty)
    })
}
fn idle_timeout_expired(
    completed: bool,
    queue_empty: bool,
    timeout_minutes: u32,
    idle_for: Duration,
) -> bool {
    completed
        && queue_empty
        && timeout_minutes > 0
        && idle_for >= Duration::from_secs(u64::from(timeout_minutes) * 60)
}

fn send_raw(input: &Arc<Mutex<ChildStdin>>, frame: Value) -> Result<(), String> {
    let mut input = input.lock().map_err(|_| "omp RPC stdin lock poisoned")?;
    serde_json::to_writer(&mut *input, &frame).map_err(|e| e.to_string())?;
    input.write_all(b"\n").map_err(|e| e.to_string())?;
    input.flush().map_err(|e| e.to_string())
}

pub fn approval_mode(mode: &crate::domain::AccessMode) -> (&'static str, bool) {
    use crate::domain::AccessMode::*;
    match mode {
        FullAccess => ("yolo", false),
        WorkspaceWrite => ("write", false),
        Custom => ("always-ask", false),
        ReadOnly | Plan => ("always-ask", true),
    }
}

/// The approval tier omp itself applies in `cwd` when Lume passes no `--approval-mode`
/// (project `.omp/config.yml` over the global config; omp's own default is yolo).
pub fn configured_access_mode(cwd: &str) -> crate::domain::AccessMode {
    use crate::domain::AccessMode;
    let configured = crate::executables::command("omp")
        .ok()
        .and_then(|mut command| {
            command
                .args(["config", "get", "tools.approvalMode"])
                .current_dir(cwd)
                .output()
                .ok()
        })
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
    match configured.as_deref() {
        Some("always-ask") => AccessMode::Custom,
        Some("write") => AccessMode::WorkspaceWrite,
        _ => AccessMode::FullAccess,
    }
}

fn question_response(
    method: &str,
    request_id: &str,
    answers: &[crate::domain::QuestionAnswer],
    pending: Option<&crate::domain::PendingQuestion>,
) -> Value {
    if answers.is_empty() {
        return json!({"type":"extension_ui_response","id":request_id,"cancelled":true});
    }
    let values = answers
        .iter()
        .flat_map(|answer| answer.answers.iter().cloned())
        .collect::<Vec<_>>();
    if values.is_empty() && method != "ask" {
        return json!({"type":"extension_ui_response","id":request_id,"cancelled":true});
    }
    match method {
        "confirm" => {
            json!({"type":"extension_ui_response","id":request_id,"confirmed":values.first().is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "yes" | "true" | "confirm" | "ok"))})
        }
        "ask" => {
            let mapped = answers
                .iter()
                .map(|answer| {
                    let options = pending
                        .and_then(|pending| {
                            pending
                                .questions
                                .iter()
                                .find(|question| question.id == answer.question_id)
                        })
                        .map(|question| {
                            question
                                .options
                                .iter()
                                .map(|option| option.label.as_str())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let selected = answer
                        .answers
                        .iter()
                        .filter(|value| options.contains(&value.as_str()))
                        .cloned()
                        .collect::<Vec<_>>();
                    let mut result = serde_json::Map::new();
                    result.insert("id".into(), json!(answer.question_id));
                    result.insert("selectedOptions".into(), json!(selected));
                    if let Some(custom) = answer.answers.iter().find(|value| {
                        !options.contains(&value.as_str()) && !value.trim().is_empty()
                    }) {
                        result.insert("customInput".into(), json!(custom.trim()));
                    }
                    Value::Object(result)
                })
                .collect::<Vec<_>>();
            json!({"type":"extension_ui_response","id":request_id,"answers":mapped})
        }
        _ => {
            json!({"type":"extension_ui_response","id":request_id,"value":values.first().cloned().unwrap_or_default()})
        }
    }
}

fn is_approval_request(frame: &Value) -> bool {
    let method = frame["method"].as_str().unwrap_or("");
    if method != "select" {
        return false;
    }
    if frame["title"]
        .as_str()
        .unwrap_or("")
        .starts_with("Allow tool:")
    {
        return true;
    }
    if let Some(options) = frame["options"].as_array() {
        let has_approve = options
            .iter()
            .any(|opt| opt.as_str() == Some("Approve") || opt["label"].as_str() == Some("Approve"));
        let has_deny = options
            .iter()
            .any(|opt| opt.as_str() == Some("Deny") || opt["label"].as_str() == Some("Deny"));
        if has_approve && has_deny {
            return true;
        }
    }
    false
}

fn is_ready_frame(frame: &Value) -> bool {
    frame["type"] == "ready"
}

/// `prompt_result` is the authoritative end of a submitted prompt, including any steering it
/// absorbed; `agent_end`/`session_settled` also fire mid-queue and after aborts, so they do not
/// decide the status.
fn event_kind(frame: &Value) -> Option<HookEventKind> {
    match frame["type"].as_str()? {
        "agent_start" | "turn_start" => Some(HookEventKind::Running),
        "prompt_result" => match frame["status"].as_str()? {
            "completed" => Some(HookEventKind::Completed),
            "failed" | "error" => Some(HookEventKind::Failed),
            // Same convention as an interrupted Codex turn: the conversation waits for the user.
            "aborted" | "interrupted" => Some(HookEventKind::WaitingForInput),
            _ => None,
        },
        _ => None,
    }
}

fn prompt_command(delivery: &str) -> &'static str {
    match delivery {
        "steer" => "steer",
        "queue" => "follow_up",
        _ => "prompt",
    }
}

fn approval_response(request_id: &str, action: crate::domain::PermissionAction) -> Value {
    let value = match action {
        crate::domain::PermissionAction::AllowOnce => "Approve",
        crate::domain::PermissionAction::Deny => "Deny",
        _ => "Deny",
    };
    json!({"type":"extension_ui_response","id":request_id,"value":value})
}

fn map_frame(
    session_key: &Arc<Mutex<String>>,
    frame: &Value,
    state: &AppState,
    publish: &EventPublisher,
    raw_sender: Option<&RawSender>,
) {
    let session_id = session_key.lock().map(|id| id.clone()).unwrap_or_default();
    let mut event = base_event(&session_id);
    match frame["type"].as_str().unwrap_or("") {
        "agent_start" | "turn_start" | "prompt_result" => {
            let Some(kind) = event_kind(frame) else {
                return;
            };
            event.event = kind;
            event.status_label = Some(match kind {
                HookEventKind::Failed => frame["error"]["message"]
                    .as_str()
                    .or(frame["error"].as_str())
                    .unwrap_or("Falha no Oh My Pi")
                    .to_string(),
                HookEventKind::WaitingForInput => "Tarefa interrompida".into(),
                HookEventKind::Completed => "Concluído".into(),
                _ => "Executando".into(),
            });
        }
        "tool_execution_start" | "tool_execution_end" => {
            event.event = HookEventKind::Activity;
            let ended = frame["type"] == "tool_execution_end";
            let status = match (ended, frame["isError"].as_bool()) {
                (false, _) => "running",
                (true, Some(true)) => "failed",
                (true, _) => "completed",
            };
            let mut activity = crate::omp_sessions::tool_activity(
                frame["toolCallId"].as_str().unwrap_or_default(),
                frame["toolName"].as_str().unwrap_or("Ferramenta"),
                &frame["args"],
                status,
                crate::state::now_millis(),
            );
            if ended {
                // The end frame has no arguments: only settle the row the start frame created.
                activity.detail = None;
                activity.append_detail = true;
            }
            event.activity = Some(activity);
        }
        "message_end" => {
            let message = &frame["message"];
            if message["role"] == "user" {
                event.event = HookEventKind::Activity;
                event.activity = Some(rpc_activity(
                    &session_id,
                    "prompt",
                    "Prompt enviado",
                    message_text(&message["content"]),
                    "completed",
                ));
            } else if message["role"] == "assistant" {
                let text = message_text(&message["content"]);
                event.last_response = (!text.is_empty()).then_some(text.clone());
                if !text.is_empty() {
                    event.event = HookEventKind::Activity;
                    event.activity = Some(rpc_activity(
                        &session_id,
                        "message",
                        "Resposta do agente",
                        text,
                        "completed",
                    ));
                } else {
                    return;
                }
            } else {
                return;
            }
        }
        "queue_update" => {
            // The frame is the whole displayable queue: show each entry as a waiting queued
            // prompt and settle the ones omp has delivered since the previous snapshot.
            event.event = HookEventKind::Activity;
            let queued = ["steering", "followUp"]
                .iter()
                .flat_map(|queue| {
                    frame[queue]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(move |text| (format!("omp-queue:{queue}:{text}"), text))
                })
                .collect::<Vec<_>>();
            let delivered = state
                .connected_session(&session_id)
                .map(|session| session.activities)
                .unwrap_or_default()
                .into_iter()
                .filter(|activity| {
                    activity.kind == "queued_prompt"
                        && activity.status == "waiting"
                        && activity.id.starts_with("omp-queue:")
                        && !queued.iter().any(|(id, _)| *id == activity.id)
                })
                .map(|mut activity| {
                    activity.status = "completed".into();
                    activity
                });
            event.activities = queued
                .iter()
                .map(|(id, text)| crate::domain::SessionActivity {
                    id: id.clone(),
                    kind: "queued_prompt".into(),
                    title: "Queued prompt".into(),
                    detail: Some(text.to_string()),
                    status: "waiting".into(),
                    created_at: crate::state::now_millis(),
                    files: vec![],
                    attachments: vec![],
                    append_detail: false,
                })
                .chain(delivered)
                .collect();
        }
        // Progress and inner events repeat many times per child; the lifecycle frame alone
        // carries the start and the final state.
        "subagent_lifecycle" => {
            let payload = &frame["payload"];
            let native_id = session_id.trim_start_matches("omp-rpc:");
            let status = match payload["status"].as_str().unwrap_or("started") {
                "completed" => "completed",
                "failed" | "error" => "failed",
                "aborted" | "cancelled" => "interrupted",
                _ => "running",
            };
            event.event = HookEventKind::Activity;
            event.activity = Some(crate::domain::SessionActivity {
                id: format!(
                    "omp:{native_id}:subagent:{}",
                    payload["id"].as_str().unwrap_or("unknown")
                ),
                kind: "subagent".into(),
                title: format!(
                    "Subagente · {}",
                    payload["agent"].as_str().unwrap_or("subagent")
                ),
                detail: None,
                status: status.into(),
                created_at: crate::state::now_millis(),
                files: vec![],
                attachments: vec![],
                append_detail: false,
            });
        }
        "notice"
            if frame["source"] == "session-persistence" && frame["reason"] == "open-elsewhere" =>
        {
            let native_id = frame["to"]
                .as_str()
                .or(frame["newSessionId"].as_str())
                .unwrap_or_default();
            if !native_id.is_empty() {
                if let Ok(next_id) = state.rebind_omp_session(&session_id, native_id, false) {
                    if let Ok(mut key) = session_key.lock() {
                        *key = next_id.clone();
                    }
                    event = base_event(&next_id);
                    event.event = HookEventKind::Activity;
                    event.activity = Some(rpc_activity(
                        &next_id,
                        "warning",
                        "Session opened elsewhere",
                        frame["message"]
                            .as_str()
                            .unwrap_or("The omp session continued in another window."),
                        "completed",
                    ));
                }
            }
        }
        "extension_ui_request" => {
            let method = frame["method"].as_str().unwrap_or("");
            if is_approval_request(frame) {
                event.event = HookEventKind::PermissionRequest;
                event.wait_for_decision = true;
                // The title is "Allow tool: <name>" followed by the tool's arguments.
                let (summary, resource) = frame["title"]
                    .as_str()
                    .unwrap_or("Allow tool")
                    .split_once('\n')
                    .unwrap_or((frame["title"].as_str().unwrap_or("Allow tool"), ""));
                event.permission = Some(crate::domain::PermissionRequest {
                    id: frame["id"].as_str().unwrap_or_default().into(),
                    kind: "tool".into(),
                    summary: summary.into(),
                    resource: resource.chars().take(2_000).collect(),
                    risk: "approval".into(),
                    requested_at: chrono::DateTime::<chrono::Utc>::from(
                        std::time::SystemTime::now(),
                    )
                    .to_rfc3339(),
                });
            } else if method == "cancel" {
                let target_id = frame["targetId"].as_str().or_else(|| frame["id"].as_str());
                if let Some(target_id) = target_id {
                    let mut cleared = false;
                    if let Ok(session) = state.connected_session(&session_id) {
                        if let Some(pending) = session.pending_question {
                            if pending.id.ends_with(&format!(":{target_id}"))
                                || pending.id == target_id
                            {
                                let _ = state.expire_question(&pending.id);
                                cleared = true;
                            }
                        }
                        if let Some(permission) = session.pending_permission {
                            if permission.id == target_id {
                                cleared = true;
                            }
                        }
                    }
                    if cleared {
                        let mut event = base_event(&session_id);
                        event.event = HookEventKind::Running;
                        event.status_label = Some("Executando".into());
                        publish(event);
                    }
                }
                return;
            } else if method == "notify" {
                let notify_type = frame["notifyType"].as_str().unwrap_or("info");
                let (kind, title) = match notify_type {
                    "error" => ("error", "Erro"),
                    "warning" => ("warning", "Aviso"),
                    _ => ("notification", "Notificação"),
                };
                let message = frame["message"].as_str().unwrap_or_default();
                event.event = HookEventKind::Activity;
                event.activity = Some(rpc_activity(&session_id, kind, title, message, "completed"));
            } else if method == "setStatus" {
                let status_text = frame["statusText"].as_str().unwrap_or_default();
                if !status_text.is_empty() {
                    let key = frame["statusKey"].as_str().unwrap_or("Status");
                    event.event = HookEventKind::Activity;
                    event.activity = Some(rpc_activity(
                        &session_id,
                        "status",
                        key,
                        status_text,
                        "completed",
                    ));
                } else {
                    return;
                }
            } else if method == "setTitle" {
                let title = frame["title"].as_str().unwrap_or_default();
                if !title.is_empty() {
                    event.event = HookEventKind::Activity;
                    event.activity = Some(rpc_activity(
                        &session_id,
                        "title",
                        "Título",
                        title,
                        "completed",
                    ));
                } else {
                    return;
                }
            } else if method == "setWidget" {
                let lines = frame["widgetLines"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>();
                if !lines.is_empty() {
                    let key = frame["widgetKey"].as_str().unwrap_or("Widget");
                    event.event = HookEventKind::Activity;
                    event.activity = Some(rpc_activity(
                        &session_id,
                        "widget",
                        key,
                        lines.join("\n"),
                        "completed",
                    ));
                } else {
                    return;
                }
            } else if method == "open_url" {
                let url = frame["launchUrl"]
                    .as_str()
                    .or_else(|| frame["url"].as_str())
                    .unwrap_or_default();
                let instructions = frame["instructions"].as_str().unwrap_or_default();
                let detail = if instructions.is_empty() {
                    url.to_string()
                } else {
                    format!("{instructions}\n{url}")
                };
                event.event = HookEventKind::Activity;
                event.activity = Some(rpc_activity(
                    &session_id,
                    "open_url",
                    "Abrir URL",
                    detail,
                    "completed",
                ));
            } else if method == "set_editor_text" {
                let text = frame["text"].as_str().unwrap_or_default();
                if !text.is_empty() {
                    event.event = HookEventKind::Activity;
                    event.activity = Some(rpc_activity(
                        &session_id,
                        "editor_text",
                        "Texto do editor",
                        text,
                        "completed",
                    ));
                } else {
                    return;
                }
            } else if matches!(method, "ask" | "confirm" | "input" | "editor" | "select") {
                if let Some(request_id) = frame["id"].as_str() {
                    event.event = HookEventKind::QuestionRequest;
                    event.wait_for_decision = true;
                    let questions = if method == "ask" {
                        frame["questions"].as_array().cloned().unwrap_or_default()
                    } else {
                        vec![json!({
                            "id": "answer",
                            "question": frame["message"].as_str().or(frame["title"].as_str()).unwrap_or("Input required"),
                            "header": frame["title"].as_str().unwrap_or("Question"),
                            "options": frame["options"].as_array().cloned().unwrap_or_default(),
                            "isOther": true,
                        })]
                    };
                    let questions = questions
                        .iter()
                        .enumerate()
                        .map(|(index, item)| {
                            let options = item["options"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .map(|option| {
                                    let label = option
                                        .as_str()
                                        .or(option["label"].as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    crate::domain::QuestionOption {
                                        label,
                                        description: option["description"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .to_string(),
                                    }
                                })
                                .collect();
                            crate::domain::InteractiveQuestion {
                                id: item["id"]
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| format!("question-{index}")),
                                header: item["header"].as_str().unwrap_or_default().to_string(),
                                question: item["question"].as_str().unwrap_or_default().to_string(),
                                is_other: true,
                                is_secret: false,
                                options,
                            }
                        })
                        .collect();
                    event.question = Some(crate::domain::PendingQuestion {
                        id: format!("omp-ui:{method}:{request_id}"),
                        questions,
                        requested_at: chrono::DateTime::<chrono::Utc>::from(
                            std::time::SystemTime::now(),
                        )
                        .to_rfc3339(),
                    });
                } else {
                    event.event = HookEventKind::Activity;
                    event.activity = Some(rpc_activity(
                        &session_id,
                        "warning",
                        "Solicitação inválida",
                        format!("Solicitação {method} sem identificador"),
                        "completed",
                    ));
                }
            } else {
                if let Some(request_id) = frame["id"].as_str() {
                    if let Some(sender) = raw_sender {
                        sender(json!({
                            "type": "extension_ui_response",
                            "id": request_id,
                            "cancelled": true
                        }));
                    }
                }
                event.event = HookEventKind::Activity;
                event.activity = Some(rpc_activity(
                    &session_id,
                    "warning",
                    "Solicitação não suportada",
                    format!("Método de interface não suportado: {method}"),
                    "completed",
                ));
            }
        }
        _ => return,
    }
    if event.status_label.is_none() {
        event.status_label = match event.event {
            HookEventKind::Running => Some("Executando".into()),
            HookEventKind::PermissionRequest => Some("Aprovação necessária".into()),
            HookEventKind::QuestionRequest => Some("Aguardando resposta".into()),
            _ => None,
        };
    }
    publish(event);
}

fn message_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("")
}

fn rpc_activity(
    session_id: &str,
    kind: &str,
    title: &str,
    detail: impl Into<String>,
    status: &str,
) -> crate::domain::SessionActivity {
    let detail = detail.into();
    crate::domain::SessionActivity {
        id: format!(
            "omp-rpc:{}:{}:{}",
            session_id,
            kind,
            crate::state::now_millis()
        ),
        kind: kind.to_string(),
        title: title.to_string(),
        detail: (!detail.is_empty()).then_some(detail),
        status: status.to_string(),
        created_at: crate::state::now_millis(),
        files: vec![],
        attachments: vec![],
        append_detail: false,
    }
}

fn base_event(session_id: &str) -> HookEvent {
    HookEvent {
        event: HookEventKind::Running,
        session_id: session_id.to_string(),
        agent: AgentKind::Omp,
        agent_label: Some("Oh My Pi".into()),
        session_name: None,
        project: None,
        source: Some(SessionSource::Desktop),
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status_label: None,
        started_at: None,
        process_id: None,
        native_session_id: Some(session_id.trim_start_matches("omp-rpc:").to_string()),
        working_directory: None,
        permission_profile: None,
        permission: None,
        question: None,
        last_response: None,
        activity: None,
        activities: vec![],
        wait_for_decision: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_access_modes_to_omp_approval_modes() {
        use crate::domain::AccessMode::*;
        assert_eq!(approval_mode(&FullAccess), ("yolo", false));
        assert_eq!(approval_mode(&WorkspaceWrite), ("write", false));
        assert_eq!(approval_mode(&Custom), ("always-ask", false));
        assert_eq!(approval_mode(&ReadOnly), ("always-ask", true));
        assert_eq!(approval_mode(&Plan), ("always-ask", true));
    }
    #[test]
    fn maps_ready_result_and_approval_frames() {
        assert!(is_ready_frame(&json!({"type":"ready","protocolVersion":1})));
        assert!(!is_ready_frame(&json!({"type":"response"})));
        assert_eq!(
            event_kind(&json!({"type":"prompt_result","status":"completed"})),
            Some(HookEventKind::Completed)
        );
        assert_eq!(
            event_kind(&json!({"type":"prompt_result","status":"aborted"})),
            Some(HookEventKind::WaitingForInput)
        );
        assert_eq!(
            event_kind(&json!({"type":"prompt_result","status":"error"})),
            Some(HookEventKind::Failed)
        );
        assert_eq!(
            approval_response("r1", crate::domain::PermissionAction::AllowOnce),
            json!({"type":"extension_ui_response","id":"r1","value":"Approve"})
        );
        assert_eq!(
            approval_response("r2", crate::domain::PermissionAction::Deny),
            json!({"type":"extension_ui_response","id":"r2","value":"Deny"})
        );
    }

    #[test]
    fn maps_prompt_delivery_modes_to_rpc_commands() {
        assert_eq!(prompt_command("new_turn"), "prompt");
        assert_eq!(prompt_command("steer"), "steer");
        assert_eq!(prompt_command("queue"), "follow_up");
    }
    #[test]
    fn queue_snapshots_show_pending_prompts_and_settle_delivered_ones() {
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        let mut started = base_event("omp-rpc:test");
        started.event = HookEventKind::SessionStarted;
        started.control_origin = SessionControlOrigin::Lume;
        state.ingest(started).expect("session");
        let sink = state.clone();
        let publish: EventPublisher = Arc::new(move |event| {
            sink.ingest(event).expect("ingest");
        });
        let key = Arc::new(Mutex::new("omp-rpc:test".into()));
        map_frame(
            &key,
            &json!({"type":"queue_update","steering":["now"],"followUp":["later"]}),
            &state,
            &publish,
            None,
        );
        map_frame(
            &key,
            &json!({"type":"queue_update","steering":[],"followUp":["later"]}),
            &state,
            &publish,
            None,
        );
        let session = state.connected_session("omp-rpc:test").expect("session");
        let queued = session
            .activities
            .iter()
            .filter(|activity| activity.kind == "queued_prompt")
            .map(|activity| (activity.detail.as_deref(), activity.status.as_str()))
            .collect::<Vec<_>>();
        assert!(queued.contains(&(Some("now"), "completed")));
        assert!(queued.contains(&(Some("later"), "waiting")));
    }

    #[test]
    fn ask_requests_become_pending_questions() {
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        let events = Arc::new(Mutex::new(Vec::<HookEvent>::new()));
        let captured = events.clone();
        let publish: EventPublisher =
            Arc::new(move |event| captured.lock().expect("events lock").push(event));
        let key = Arc::new(Mutex::new("omp-rpc:test".into()));
        map_frame(
            &key,
            &json!({
                "type":"extension_ui_request","id":"ask-1","method":"ask",
                "questions":[{"id":"q1","header":"Choice","question":"Pick","options":["A","B"]}]
            }),
            &state,
            &publish,
            None,
        );
        let events = events.lock().expect("events lock");
        assert_eq!(events[0].event, HookEventKind::QuestionRequest);
        assert!(events[0].wait_for_decision);
        let question = events[0].question.as_ref().expect("pending question");
        assert_eq!(question.id, "omp-ui:ask:ask-1");
        assert_eq!(question.questions[0].options.len(), 2);
    }

    #[test]
    fn maps_input_confirm_and_editor_to_pending_questions() {
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        for method in ["input", "confirm", "editor"] {
            let events = Arc::new(Mutex::new(Vec::<HookEvent>::new()));
            let captured = events.clone();
            let publish: EventPublisher =
                Arc::new(move |event| captured.lock().expect("events lock").push(event));
            map_frame(
                &Arc::new(Mutex::new("omp-rpc:test".into())),
                &json!({"type":"extension_ui_request","id":"r1","method":method,"message":"Answer"}),
                &state,
                &publish,
                None,
            );
            let events = events.lock().expect("events lock");
            assert_eq!(events[0].event, HookEventKind::QuestionRequest);
            assert_eq!(
                events[0].question.as_ref().unwrap().questions[0].question,
                "Answer"
            );
        }
    }
    #[test]
    fn serializes_question_answers_for_each_wire_method() {
        let answer = crate::domain::QuestionAnswer {
            question_id: "q1".into(),
            answers: vec!["A".into(), "custom".into()],
        };
        for method in ["input", "editor"] {
            assert_eq!(
                question_response(method, "r1", std::slice::from_ref(&answer), None),
                json!({"type":"extension_ui_response","id":"r1","value":"A"})
            );
        }
        assert_eq!(
            question_response(
                "confirm",
                "r2",
                &[crate::domain::QuestionAnswer {
                    question_id: "confirm".into(),
                    answers: vec!["yes".into()],
                }],
                None
            ),
            json!({"type":"extension_ui_response","id":"r2","confirmed":true})
        );
        let pending = crate::domain::PendingQuestion {
            id: "ask".into(),
            questions: vec![crate::domain::InteractiveQuestion {
                id: "q1".into(),
                header: String::new(),
                question: String::new(),
                is_other: true,
                is_secret: false,
                options: vec![crate::domain::QuestionOption {
                    label: "A".into(),
                    description: String::new(),
                }],
            }],
            requested_at: String::new(),
        };
        assert_eq!(
            question_response("ask", "r3", &[answer], Some(&pending)),
            json!({"type":"extension_ui_response","id":"r3","answers":[{
                "id":"q1","selectedOptions":["A"],"customInput":"custom"
            }]})
        );
        assert_eq!(
            question_response("input", "r4", &[], None),
            json!({"type":"extension_ui_response","id":"r4","cancelled":true})
        );
        assert_eq!(
            question_response("confirm", "r5", &[], None),
            json!({"type":"extension_ui_response","id":"r5","cancelled":true})
        );
        assert_eq!(
            question_response("ask", "r6", &[], None),
            json!({"type":"extension_ui_response","id":"r6","cancelled":true})
        );
    }
    #[test]
    fn fork_notice_rebinds_native_and_lume_session_ids() {
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        let mut started = base_event("omp-rpc:old");
        started.event = HookEventKind::SessionStarted;
        started.native_session_id = Some("old".into());
        state.ingest(started).expect("initial session");
        let key = Arc::new(Mutex::new("omp-rpc:old".into()));
        let events = Arc::new(Mutex::new(Vec::<HookEvent>::new()));
        let captured = events.clone();
        let publish_state = state.clone();
        let publish: EventPublisher = Arc::new(move |event| {
            let _ = publish_state.ingest(event.clone());
            captured.lock().expect("events lock").push(event);
        });
        map_frame(
            &key,
            &json!({
                "type":"notice","source":"session-persistence","reason":"open-elsewhere",
                "from":"old","to":"new"
            }),
            &state,
            &publish,
            None,
        );
        assert_eq!(*key.lock().expect("key lock"), "omp-rpc:new");
        let rebound = state.connected_session("omp-rpc:new").expect("rebound");
        assert_eq!(rebound.native_session_id.as_deref(), Some("new"));
        assert_eq!(rebound.activities.last().unwrap().kind, "warning");
        assert!(state.connected_session("omp-rpc:old").is_err());
    }
    #[test]
    fn idle_timeout_requires_completed_empty_queue_and_nonzero_preference() {
        let elapsed = Duration::from_secs(30 * 60);
        assert!(idle_timeout_expired(true, true, 30, elapsed));
        assert!(!idle_timeout_expired(false, true, 30, elapsed));
        assert!(!idle_timeout_expired(true, false, 30, elapsed));
        assert!(!idle_timeout_expired(true, true, 0, elapsed));
    }

    #[test]
    fn unsupported_extension_ui_request_receives_cancel_response_and_emits_warning() {
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        let events = Arc::new(Mutex::new(Vec::<HookEvent>::new()));
        let captured = events.clone();
        let publish: EventPublisher =
            Arc::new(move |event| captured.lock().expect("events lock").push(event));
        let key = Arc::new(Mutex::new("omp-rpc:test".into()));
        let responses = Arc::new(Mutex::new(Vec::<Value>::new()));
        let responses_clone = responses.clone();
        let raw_sender: RawSender = Arc::new(move |val| {
            responses_clone.lock().expect("responses lock").push(val);
        });
        map_frame(
            &key,
            &json!({
                "type": "extension_ui_request",
                "id": "unsupported-42",
                "method": "custom_unsupported_dialog",
                "title": "Custom Dialog"
            }),
            &state,
            &publish,
            Some(&raw_sender),
        );
        let sent = responses.lock().expect("responses lock");
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0],
            json!({
                "type": "extension_ui_response",
                "id": "unsupported-42",
                "cancelled": true
            })
        );
        let events = events.lock().expect("events lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, HookEventKind::Activity);
        let activity = events[0].activity.as_ref().expect("warning activity");
        assert_eq!(activity.kind, "warning");
        assert_eq!(activity.status, "completed");
    }

    #[test]
    fn presentation_extension_ui_requests_become_activities_without_reply() {
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        let events = Arc::new(Mutex::new(Vec::<HookEvent>::new()));
        let captured = events.clone();
        let publish: EventPublisher =
            Arc::new(move |event| captured.lock().expect("events lock").push(event));
        let key = Arc::new(Mutex::new("omp-rpc:test".into()));
        let responses = Arc::new(Mutex::new(Vec::<Value>::new()));
        let responses_clone = responses.clone();
        let raw_sender: RawSender = Arc::new(move |val| {
            responses_clone.lock().expect("responses lock").push(val);
        });

        for (notify_type, expected_kind) in [
            ("info", "notification"),
            ("warning", "warning"),
            ("error", "error"),
        ] {
            map_frame(
                &key,
                &json!({
                    "type": "extension_ui_request",
                    "id": "note-1",
                    "method": "notify",
                    "message": "Hello notification",
                    "notifyType": notify_type
                }),
                &state,
                &publish,
                Some(&raw_sender),
            );
            let last = events.lock().expect("events lock").pop().expect("activity");
            assert_eq!(last.event, HookEventKind::Activity);
            assert_eq!(last.activity.as_ref().unwrap().kind, expected_kind);
            assert_eq!(
                last.activity.as_ref().unwrap().detail.as_deref(),
                Some("Hello notification")
            );
        }

        map_frame(
            &key,
            &json!({
                "type": "extension_ui_request",
                "id": "status-1",
                "method": "setStatus",
                "statusKey": "git",
                "statusText": "indexing"
            }),
            &state,
            &publish,
            Some(&raw_sender),
        );
        let last = events
            .lock()
            .expect("events lock")
            .pop()
            .expect("status activity");
        assert_eq!(last.activity.as_ref().unwrap().kind, "status");
        assert_eq!(
            last.activity.as_ref().unwrap().detail.as_deref(),
            Some("indexing")
        );

        map_frame(
            &key,
            &json!({
                "type": "extension_ui_request",
                "id": "title-1",
                "method": "setTitle",
                "title": "Refactoring"
            }),
            &state,
            &publish,
            Some(&raw_sender),
        );
        let last = events
            .lock()
            .expect("events lock")
            .pop()
            .expect("title activity");
        assert_eq!(last.activity.as_ref().unwrap().kind, "title");
        assert_eq!(
            last.activity.as_ref().unwrap().detail.as_deref(),
            Some("Refactoring")
        );

        map_frame(
            &key,
            &json!({
                "type": "extension_ui_request",
                "id": "url-1",
                "method": "open_url",
                "url": "https://example.com/oauth",
                "launchUrl": "http://127.0.0.1:8080/callback"
            }),
            &state,
            &publish,
            Some(&raw_sender),
        );
        let last = events
            .lock()
            .expect("events lock")
            .pop()
            .expect("url activity");
        assert_eq!(last.activity.as_ref().unwrap().kind, "open_url");
        assert!(last
            .activity
            .as_ref()
            .unwrap()
            .detail
            .as_deref()
            .unwrap()
            .contains("http://127.0.0.1:8080/callback"));

        map_frame(
            &key,
            &json!({
                "type": "extension_ui_request",
                "id": "widget-1",
                "method": "setWidget",
                "widgetKey": "todos",
                "widgetLines": ["Task 1", "Task 2"]
            }),
            &state,
            &publish,
            Some(&raw_sender),
        );
        let last = events
            .lock()
            .expect("events lock")
            .pop()
            .expect("widget activity");
        assert_eq!(last.activity.as_ref().unwrap().kind, "widget");
        assert_eq!(
            last.activity.as_ref().unwrap().detail.as_deref(),
            Some("Task 1\nTask 2")
        );

        // Presentation methods must not send any responses back
        assert!(responses.lock().expect("responses lock").is_empty());
    }

    #[test]
    fn cancel_request_clears_pending_question_and_permission() {
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        let mut started = base_event("omp-rpc:test");
        started.event = HookEventKind::SessionStarted;
        started.control_origin = SessionControlOrigin::Lume;
        state.ingest(started).expect("session");

        let sink = state.clone();
        let publish: EventPublisher = Arc::new(move |event| {
            let _ = sink.ingest(event);
        });
        let key = Arc::new(Mutex::new("omp-rpc:test".into()));

        map_frame(
            &key,
            &json!({
                "type": "extension_ui_request",
                "id": "ask-42",
                "method": "ask",
                "questions": [{"id": "q1", "question": "Proceed?", "options": ["Yes", "No"]}]
            }),
            &state,
            &publish,
            None,
        );
        let session = state.connected_session("omp-rpc:test").expect("session");
        assert!(session.pending_question.is_some());

        map_frame(
            &key,
            &json!({
                "type": "extension_ui_request",
                "method": "cancel",
                "targetId": "ask-42"
            }),
            &state,
            &publish,
            None,
        );
        let session = state.connected_session("omp-rpc:test").expect("session");
        assert!(session.pending_question.is_none());
        assert_eq!(session.status, crate::domain::SessionStatus::Running);
    }

    #[test]
    fn per_session_startup_synchronization_is_independent() {
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        let publish: EventPublisher = Arc::new(|_| {});
        let rpc = OmpRpc::with_publisher(state, publish);

        let lock_a1 = rpc.session_start_lock("session-A").expect("lock a1");
        let lock_a2 = rpc.session_start_lock("session-A").expect("lock a2");
        let lock_b = rpc.session_start_lock("session-B").expect("lock b");

        assert!(Arc::ptr_eq(&lock_a1, &lock_a2));
        assert!(!Arc::ptr_eq(&lock_a1, &lock_b));

        let _guard_a = lock_a1.lock().expect("lock a1");
        let guard_b = lock_b.try_lock();
        assert!(guard_b.is_ok());
    }

    #[test]
    fn approval_detection_supports_options_and_title() {
        assert!(is_approval_request(&json!({
            "method": "select",
            "title": "Allow tool: bash\nrm -rf /"
        })));
        assert!(is_approval_request(&json!({
            "method": "select",
            "title": "Permission required",
            "options": ["Approve", "Deny"]
        })));
        assert!(is_approval_request(&json!({
            "method": "select",
            "title": "Permission required",
            "options": [{"label": "Approve"}, {"label": "Deny"}]
        })));
        assert!(!is_approval_request(&json!({
            "method": "select",
            "title": "Choose database",
            "options": ["Postgres", "SQLite"]
        })));
        assert!(!is_approval_request(&json!({
            "method": "confirm",
            "title": "Allow tool: bash"
        })));
    }

    #[test]
    fn empty_or_cancelled_answers_produce_cancelled_response() {
        assert_eq!(
            question_response("input", "r1", &[], None),
            json!({"type":"extension_ui_response","id":"r1","cancelled":true})
        );
        assert_eq!(
            question_response("confirm", "r2", &[], None),
            json!({"type":"extension_ui_response","id":"r2","cancelled":true})
        );
        assert_eq!(
            question_response("ask", "r3", &[], None),
            json!({"type":"extension_ui_response","id":"r3","cancelled":true})
        );
        let empty_answer = crate::domain::QuestionAnswer {
            question_id: "q1".into(),
            answers: vec![],
        };
        assert_eq!(
            question_response("input", "r4", &[empty_answer], None),
            json!({"type":"extension_ui_response","id":"r4","cancelled":true})
        );
    }
    #[test]
    fn rpc_v2_reassembles_large_frames_and_rejects_interleaving() {
        let frame = json!({"type":"message_end","message":{"content":"Olá \u{1f600}"}});
        let bytes = serde_json::to_vec(&frame).expect("serialize");
        let half = bytes.len() / 2;
        let chunks = [&bytes[..half], &bytes[half..]];
        let mut pending = None;
        for (index, part) in chunks.iter().enumerate() {
            let decoded = decode_rpc_frame(
                json!({
                    "type":"rpc_chunk", "chunkId":"c1", "index":index,
                    "count":2, "byteLength":bytes.len(), "data":STANDARD.encode(part)
                }),
                &mut pending,
            )
            .expect("valid chunk");
            if index == 0 {
                assert!(decoded.is_none());
                assert!(decode_rpc_frame(json!({"type":"response"}), &mut pending).is_err());
            } else {
                assert_eq!(decoded, Some(frame.clone()));
                assert!(pending.is_none());
            }
        }
        assert!(decode_rpc_frame(
            json!({
                "type":"rpc_chunk","chunkId":"c1","index":1,"count":2,
                "byteLength":bytes.len(),"data":STANDARD.encode(&bytes[half..])
            }),
            &mut pending
        )
        .is_err());
    }
}
