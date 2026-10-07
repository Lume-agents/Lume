//! Metadata-observer types and isolated transport fixtures.
//!
//! Loaded threads are NOT evidence of a live CLI: Codex retains them for an
//! inactivity grace period. Nothing here creates a session, supplies a PID,
//! resumes/subscribes to a thread, or grants control of it.
//!
//! Production observation is disabled: Codex 0.159.3's initialize handshake
//! changes process-global client identity for third-party observers. Restricting
//! subsequent RPCs to metadata reads does not make that handshake read-only.

use serde::Serialize;
#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use crate::codex_identity_probe::valid_thread_id;

#[cfg(all(unix, test))]
const MAX_THREADS: usize = 64;
#[cfg(test)]
const MAX_NAME_CHARS: usize = 240;
const SNAPSHOT_TTL_MS: i64 = 12_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DaemonThread {
    pub native_session_id: String,
    pub name: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub runtime_status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DaemonObservation {
    pub status: &'static str,
    pub observation_only: bool,
    pub cli_presence_verified: bool,
    pub observed_at: i64,
    pub loaded_thread_count: usize,
    pub truncated: bool,
    pub threads: Vec<DaemonThread>,
}

impl Default for DaemonObservation {
    fn default() -> Self {
        Self::unavailable("not_observed")
    }
}

impl DaemonObservation {
    fn unavailable(status: &'static str) -> Self {
        Self {
            status,
            observation_only: true,
            cli_presence_verified: false,
            observed_at: 0,
            loaded_thread_count: 0,
            truncated: false,
            threads: Vec::new(),
        }
    }

    pub(crate) fn names(&self, now: i64) -> impl Iterator<Item = (&str, &str)> {
        let fresh = self.status == "observed"
            && self.observed_at <= now
            && now.saturating_sub(self.observed_at) <= SNAPSHOT_TTL_MS;
        self.threads.iter().filter_map(move |thread| {
            fresh.then_some(())?;
            Some((thread.native_session_id.as_str(), thread.name.as_deref()?))
        })
    }
}

#[cfg(test)]
fn bounded_text(value: Option<&Value>, limit: usize) -> Option<String> {
    let text = value?.as_str()?.trim();
    let clean = text
        .chars()
        .filter(|character| !character.is_control())
        .take(limit)
        .collect::<String>();
    (!clean.is_empty()).then_some(clean)
}

#[cfg(test)]
fn parse_thread(value: &Value, expected_id: &str) -> Result<Option<DaemonThread>, &'static str> {
    let thread = value.get("thread").ok_or("invalid_metadata")?;
    if thread.get("id").and_then(Value::as_str) != Some(expected_id) {
        return Err("identity_mismatch");
    }
    // Subagents/internal jobs must not become extra top-level conversations.
    if thread
        .get("parentThreadId")
        .is_some_and(|value| !value.is_null())
        || thread.get("ephemeral").and_then(Value::as_bool) == Some(true)
        || thread.get("source").is_some_and(|source| {
            source.is_object() || source.as_str().is_some_and(|s| s == "exec")
        })
    {
        return Ok(None);
    }
    let status = thread
        .pointer("/status/type")
        .and_then(Value::as_str)
        .ok_or("invalid_status")?;
    if !matches!(status, "active" | "idle" | "systemError" | "notLoaded") {
        return Err("invalid_status");
    }
    if status == "notLoaded" {
        return Ok(None);
    }
    Ok(Some(DaemonThread {
        native_session_id: expected_id.into(),
        name: bounded_text(thread.get("name"), MAX_NAME_CHARS),
        model: bounded_text(thread.get("model"), 100),
        reasoning_effort: bounded_text(thread.get("reasoningEffort"), 32),
        runtime_status: status.into(),
    }))
}

/// Do not contact the real daemon until it supports a non-originating observer.
/// This central guard also covers diagnostic reports, not just desktop polling.
pub(crate) fn observe() -> DaemonObservation {
    DaemonObservation::unavailable("read_only_initialize_unavailable")
}

pub(crate) fn start(state: crate::state::AppState, _app: tauri::AppHandle) -> Result<(), String> {
    // No worker, polling, socket connection or initialize side effects. Keep
    // local ancestry/hooks and SQLite/index title recovery independent of this.
    if state.has_external_codex_cli().unwrap_or(false) {
        state.replace_codex_daemon_observation(observe())?;
    }
    Ok(())
}

// Fixtures exercise metadata bounds against owned fake servers only. This
// transport is deliberately absent from production builds.
#[cfg(all(unix, test))]
mod unix {
    use super::*;
    use serde_json::json;
    use std::{
        collections::HashSet,
        os::unix::{
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        },
        path::Path,
        time::{Duration, Instant},
    };
    use tungstenite::{client::client_with_config, protocol::WebSocketConfig, Message, WebSocket};

    const IO_BUDGET: Duration = Duration::from_millis(1500);
    const MAX_MESSAGE_BYTES: usize = 512 * 1024;
    const MAX_NOTIFICATIONS: usize = 64;

    fn remaining(deadline: Instant) -> Result<Duration, &'static str> {
        let value = deadline.saturating_duration_since(Instant::now());
        (!value.is_zero()).then_some(value).ok_or("timeout")
    }

    fn rpc(
        socket: &mut WebSocket<UnixStream>,
        id: u64,
        method: &'static str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, &'static str> {
        // Keep the allowlist next to transport I/O. No caller can request resume,
        // subscribe, turn/start, interrupt, config changes, or tool execution.
        if !matches!(method, "initialize" | "thread/loaded/list" | "thread/read") {
            return Err("non_readonly_method");
        }
        socket
            .get_mut()
            .set_write_timeout(Some(remaining(deadline)?))
            .map_err(|_| "transport_error")?;
        socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| "transport_error")?;
        for _ in 0..MAX_NOTIFICATIONS {
            socket
                .get_mut()
                .set_read_timeout(Some(remaining(deadline)?))
                .map_err(|_| "transport_error")?;
            let message = socket.read().map_err(|_| "transport_error")?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(&text).map_err(|_| "invalid_response")?;
            if value.get("method").is_some() {
                continue;
            }
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                // Never answer server-originated permission/question requests.
                continue;
            }
            if value.get("error").is_some() {
                return Err("rpc_error");
            }
            return value.get("result").cloned().ok_or("invalid_response");
        }
        Err("notification_limit")
    }

    pub(super) fn observe_socket(path: &Path) -> Result<DaemonObservation, &'static str> {
        let path = resolve_socket(path)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| "daemon_unavailable")?;
        if !metadata.file_type().is_socket()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o022 != 0
        {
            return Err("unsafe_socket");
        }
        let deadline = Instant::now() + IO_BUDGET;
        let stream = UnixStream::connect(&path).map_err(|_| "daemon_unavailable")?;
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
            let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
            let result = unsafe {
                libc::getsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    (&mut peer as *mut libc::ucred).cast(),
                    &mut length,
                )
            };
            if result != 0
                || length as usize != std::mem::size_of::<libc::ucred>()
                || peer.uid != unsafe { libc::geteuid() }
            {
                return Err("untrusted_peer");
            }
        }
        stream
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(|_| "transport_error")?;
        stream
            .set_write_timeout(Some(remaining(deadline)?))
            .map_err(|_| "transport_error")?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_MESSAGE_BYTES));
        let (mut socket, _) = client_with_config("ws://localhost/", stream, Some(config))
            .map_err(|_| "handshake_failed")?;
        let result = observe_connection(&mut socket, deadline);
        // Closing THIS observer connection does not unsubscribe any CLI.
        let _ = socket.close(None);
        result
    }

    fn private_parent(path: &Path) -> bool {
        path.parent()
            .and_then(|parent| std::fs::symlink_metadata(parent).ok())
            .is_some_and(|metadata| {
                metadata.is_dir()
                    && metadata.uid() == unsafe { libc::geteuid() }
                    && metadata.mode() & 0o077 == 0
            })
    }

    fn resolve_socket(path: &Path) -> Result<std::path::PathBuf, &'static str> {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| "daemon_unavailable")?;
        if !metadata.file_type().is_symlink() {
            return Ok(path.to_owned());
        }
        // The official daemon uses a link from its private control directory
        // into a private /tmp/codex-daemon-UID directory to keep UDS paths short.
        // Do not accept arbitrary links in shared/writable directories.
        if metadata.uid() != unsafe { libc::geteuid() } || !private_parent(path) {
            return Err("unsafe_socket");
        }
        let target = std::fs::canonicalize(path).map_err(|_| "daemon_unavailable")?;
        if !private_parent(&target) {
            return Err("unsafe_socket");
        }
        Ok(target)
    }

    fn observe_connection(
        socket: &mut WebSocket<UnixStream>,
        deadline: Instant,
    ) -> Result<DaemonObservation, &'static str> {
        rpc(
            socket,
            1,
            "initialize",
            json!({"clientInfo":{
                "name":"lume_metadata_observer", "title":"Lume metadata observer",
                "version":env!("CARGO_PKG_VERSION")
            }}),
            deadline,
        )?;
        socket
            .send(Message::Text(
                json!({"method":"initialized","params":{}})
                    .to_string()
                    .into(),
            ))
            .map_err(|_| "transport_error")?;
        let loaded = rpc(
            socket,
            2,
            "thread/loaded/list",
            json!({"limit":MAX_THREADS}),
            deadline,
        )?;
        let ids = loaded
            .get("data")
            .and_then(Value::as_array)
            .ok_or("invalid_thread_list")?;
        if ids.len() > MAX_THREADS {
            return Err("thread_limit");
        }
        let mut seen = HashSet::new();
        let mut threads = Vec::new();
        for (offset, id) in ids.iter().enumerate() {
            let id = id
                .as_str()
                .filter(|id| valid_thread_id(id))
                .ok_or("invalid_thread_id")?;
            if !seen.insert(id) {
                return Err("duplicate_thread_id");
            }
            let result = rpc(
                socket,
                3 + offset as u64,
                "thread/read",
                json!({
                    "threadId": id, "includeTurns": false
                }),
                deadline,
            )?;
            if let Some(thread) = parse_thread(&result, id)? {
                threads.push(thread);
            }
        }
        threads.sort_by(|a, b| a.native_session_id.cmp(&b.native_session_id));
        Ok(DaemonObservation {
            status: "observed",
            observation_only: true,
            cli_presence_verified: false,
            observed_at: crate::state::now_millis(),
            loaded_thread_count: ids.len(),
            truncated: loaded
                .get("nextCursor")
                .is_some_and(|cursor| !cursor.is_null()),
            threads,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        const THREAD: &str = "01a0d079-9807-7b02-bba9-31792b1aa275";

        #[test]
        fn fixture_transaction_limits_metadata_and_does_not_promote_presence() {
            let mut nonce = [0u8; 8];
            getrandom::getrandom(&mut nonce).unwrap();
            let directory =
                std::env::temp_dir().join(format!("lume-{:x}", u64::from_ne_bytes(nonce)));
            std::fs::create_dir(&directory).unwrap();
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
            let path = directory.join("observer.sock");
            let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let mut methods = Vec::new();
                while let Ok(message) = socket.read() {
                    let Message::Text(text) = message else {
                        continue;
                    };
                    let request: Value = serde_json::from_str(&text).unwrap();
                    let method = request["method"].as_str().unwrap();
                    methods.push(method.to_owned());
                    if method == "thread/read" {
                        // Ignore server-originated requests even when they reuse
                        // our request ID. Monitoring must not answer questions.
                        socket
                            .send(Message::Text(
                                json!({"id":request["id"],
                            "method":"item/tool/requestUserInput", "params":{"questions":[]}})
                                .to_string()
                                .into(),
                            ))
                            .unwrap();
                        socket
                            .send(Message::Text(
                                json!({"method":"thread/status/changed",
                            "params":{"threadId":THREAD,"status":{"type":"active"}}})
                                .to_string()
                                .into(),
                            ))
                            .unwrap();
                    }
                    let result = match method {
                        "initialize" => json!({"userAgent":"fixture"}),
                        "initialized" => continue,
                        "thread/loaded/list" => {
                            json!({"data":[THREAD,"01a0d079-9807-7b02-bba9-31792b1aa276"],"nextCursor":null})
                        }
                        "thread/read" => {
                            assert_eq!(request["params"]["includeTurns"], false);
                            let id = request["params"]["threadId"].as_str().unwrap();
                            json!({"thread":{"id":id,"name":"Name without first prompt",
                                "parentThreadId":(id != THREAD).then_some(THREAD),
                                "ephemeral":false,"source":"cli","status":{"type":"idle"}}})
                        }
                        _ => panic!("non-readonly request: {method}"),
                    };
                    socket
                        .send(Message::Text(
                            json!({"id":request["id"],"result":result})
                                .to_string()
                                .into(),
                        ))
                        .unwrap();
                }
                methods
            });
            let control = directory.join("control.sock");
            std::os::unix::fs::symlink(&path, &control).unwrap();
            let snapshot = observe_socket(&control).unwrap_or_else(|error| {
                panic!(
                    "fixture socket observation failed ({error}); socket path was {} bytes",
                    path.to_string_lossy().len()
                )
            });
            let methods = server.join().unwrap();
            assert_eq!(
                methods,
                [
                    "initialize",
                    "initialized",
                    "thread/loaded/list",
                    "thread/read",
                    "thread/read"
                ]
            );
            assert_eq!(snapshot.loaded_thread_count, 2);
            assert_eq!(snapshot.threads.len(), 1);
            assert_eq!(
                snapshot.threads[0].name.as_deref(),
                Some("Name without first prompt")
            );
            assert!(!snapshot.cli_presence_verified);
            std::fs::remove_dir_all(&directory).unwrap();
        }

        #[test]
        fn refuses_insecure_or_symlinked_sockets_before_handshake() {
            let mut nonce = [0u8; 8];
            getrandom::getrandom(&mut nonce).unwrap();
            let directory = std::env::temp_dir().join(format!(
                "lume-daemon-observer-{:x}",
                u64::from_ne_bytes(nonce)
            ));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("observer.sock");
            let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
            assert_eq!(observe_socket(&path).unwrap_err(), "unsafe_socket");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let link = directory.join("link.sock");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert_eq!(observe_socket(&link).unwrap_err(), "unsafe_socket");
            std::fs::remove_dir_all(&directory).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const THREAD: &str = "01a0d079-9807-7b02-bba9-31792b1aa275";

    #[test]
    fn production_observation_is_disabled_without_contacting_a_daemon() {
        let observation = observe();
        assert_eq!(observation.status, "read_only_initialize_unavailable");
        assert!(observation.observation_only);
        assert!(!observation.cli_presence_verified);
        assert_eq!(observation.observed_at, 0);
        assert_eq!(observation.loaded_thread_count, 0);
        assert!(!observation.truncated);
        assert!(observation.threads.is_empty());
        assert_eq!(observation.names(1).count(), 0);
    }

    fn thread() -> Value {
        json!({"thread":{"id":THREAD,"name":"A real title","source":"vscode",
            "parentThreadId":null,"ephemeral":false,"status":{"type":"idle"}}})
    }

    #[test]
    fn historical_editor_origin_does_not_claim_a_current_cli_or_editor() {
        let value = parse_thread(&thread(), THREAD).unwrap().unwrap();
        assert_eq!(value.name.as_deref(), Some("A real title"));
        let output = serde_json::to_value(value).unwrap();
        assert!(output.get("processId").is_none());
        assert!(output.get("source").is_none());
        assert!(output.get("controlOrigin").is_none());
    }

    #[test]
    fn ignores_internal_subagents_ephemeral_and_unloaded_threads() {
        for (field, value) in [
            ("parentThreadId", json!(THREAD)),
            ("ephemeral", json!(true)),
            (
                "source",
                json!({"subAgent":{"thread_spawn":{"parent_thread_id":THREAD}}}),
            ),
            ("source", json!("exec")),
            ("status", json!({"type":"notLoaded"})),
        ] {
            let mut item = thread();
            item["thread"][field] = value;
            assert!(parse_thread(&item, THREAD).unwrap().is_none());
        }
    }

    #[test]
    fn never_imports_preview_permissions_turns_or_capability_tokens() {
        let mut item = thread();
        item["thread"]["preview"] = json!("private prompt");
        item["thread"]["turns"] = json!([{"text":"private history"}]);
        item["thread"]["extra"] = json!({"capability":"private token"});
        let output = serde_json::to_string(&parse_thread(&item, THREAD).unwrap()).unwrap();
        assert!(!output.contains("private"));
    }

    #[test]
    fn mismatched_identity_is_not_cached() {
        let mut item = thread();
        item["thread"]["id"] = json!("different");
        assert_eq!(parse_thread(&item, THREAD), Err("identity_mismatch"));
    }

    #[test]
    fn names_are_bounded_and_expire_without_granting_presence() {
        let mut item = thread();
        item["thread"]["name"] = json!(format!("\n{}\0", "x".repeat(400)));
        let value = parse_thread(&item, THREAD).unwrap().unwrap();
        assert_eq!(value.name.as_ref().unwrap().len(), MAX_NAME_CHARS);
        let snapshot = DaemonObservation {
            status: "observed",
            observed_at: 100,
            threads: vec![value],
            ..Default::default()
        };
        assert_eq!(snapshot.names(100).count(), 1);
        assert_eq!(snapshot.names(99).count(), 0);
        assert_eq!(snapshot.names(100 + SNAPSHOT_TTL_MS + 1).count(), 0);
        assert!(!snapshot.cli_presence_verified);
    }
}
