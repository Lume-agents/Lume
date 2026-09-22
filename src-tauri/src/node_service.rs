use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process, thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, System};

use crate::{
    distributed_protocol::{
        NodeCapability, TransportKind, MAX_PROTOCOL_VERSION, MIN_PROTOCOL_VERSION,
    },
    node_client,
    node_identity::{verify as verify_node_statement, NodeIdentity, SignedDeviceStatement},
    node_network::NodeNetworkServer,
    node_pairing,
    state::now_millis,
};

const CONFIG_FILE: &str = "node.json";
const RUNTIME_FILE: &str = "runtime.json";
const LOCK_FILE: &str = "node.lock";
const LOG_FILE: &str = "node.log";
const CONTROL_POLL_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL_MS: i64 = 30_000;
const HEARTBEAT_STALE_MS: i64 = 90_000;
const MAX_LOG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NodeConfig {
    pub enabled: bool,
    pub start_at_login: bool,
    pub node_id: String,
    pub display_name: String,
    pub listen_port: u16,
    pub allowed_project_roots: Vec<PathBuf>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            start_at_login: false,
            node_id: new_node_id(),
            display_name: System::host_name().unwrap_or_else(|| "Lume Node".into()),
            listen_port: 43_132,
            allowed_project_roots: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeLifecycle {
    Disabled,
    Offline,
    Running,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSnapshot {
    pub hostname: String,
    pub operating_system: String,
    pub architecture: String,
    pub logical_cpu_count: usize,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeHealth {
    pub lifecycle: NodeLifecycle,
    pub enabled: bool,
    pub node_id: String,
    pub display_name: String,
    pub identity_algorithm: String,
    pub identity_fingerprint: String,
    pub identity_attestation: SignedDeviceStatement,
    pub process_id: Option<u32>,
    pub started_at: Option<i64>,
    pub heartbeat_at: Option<i64>,
    pub protocol_minimum: u16,
    pub protocol_maximum: u16,
    pub capabilities: Vec<NodeCapability>,
    pub transports: Vec<TransportKind>,
    pub network: Option<NodeNetworkHealth>,
    pub machine: MachineSnapshot,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeNetworkHealth {
    pub port: u16,
    pub discovery_available: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeRuntime {
    process_id: u32,
    process_started_at: u64,
    started_at: i64,
    heartbeat_at: i64,
    #[serde(default)]
    network_port: Option<u16>,
    #[serde(default)]
    discovery_available: bool,
}

pub fn run_cli(arguments: &[String]) -> i32 {
    match run_cli_inner(arguments) {
        Ok(message) => {
            if let Some(message) = message {
                println!("{message}");
            }
            0
        }
        Err(error) => {
            eprintln!("Lume Node: {error}");
            1
        }
    }
}

fn run_cli_inner(arguments: &[String]) -> Result<Option<String>, String> {
    let command = arguments.first().map(String::as_str).unwrap_or("help");
    let state_directory = state_directory(arguments)?;
    match command {
        "enable" => {
            let mut config = load_or_create_config(&state_directory)?;
            config.enabled = true;
            save_config(&state_directory, &config)?;
            append_log(&state_directory, "Node enabled")?;
            Ok(Some(json(&health(&state_directory)?)?))
        }
        "disable" => {
            let mut config = load_or_create_config(&state_directory)?;
            config.enabled = false;
            save_config(&state_directory, &config)?;
            append_log(&state_directory, "Node disabled")?;
            Ok(Some(json(&health(&state_directory)?)?))
        }
        "status" | "health" => Ok(Some(json(&health(&state_directory)?)?)),
        "pair" => {
            let config = load_or_create_config(&state_directory)?;
            if !config.enabled {
                return Err("the Node is disabled; run `lume node enable` first".into());
            }
            let identity = NodeIdentity::load_or_create(&state_directory)?;
            let offer = node_pairing::begin_pairing(&state_directory, &config.node_id, &identity)?;
            Ok(Some(json(&offer)?))
        }
        "clients" => Ok(Some(json(&node_pairing::clients(&state_directory)?)?)),
        "discover" => Ok(Some(json(&node_client::discover(Duration::from_secs(2))?)?)),
        "connect" => {
            let pairing_uri = positional_argument(arguments, 1)
                .ok_or_else(|| "connect requires a Lume Node pairing URI".to_string())?;
            Ok(Some(json(&node_client::pair(
                &state_directory,
                pairing_uri,
                Duration::from_secs(3),
            )?)?))
        }
        "remotes" => Ok(Some(json(&node_client::remotes(&state_directory)?)?)),
        "remote-health" => {
            let node_id = positional_argument(arguments, 1)
                .ok_or_else(|| "remote-health requires a Node ID".to_string())?;
            Ok(Some(json(&node_client::remote_health(
                &state_directory,
                node_id,
                Duration::from_secs(2),
            )?)?))
        }
        "forget" => {
            let node_id = positional_argument(arguments, 1)
                .ok_or_else(|| "forget requires a Node ID".to_string())?;
            let removed = node_client::forget(&state_directory, node_id)?;
            Ok(Some(json(&serde_json::json!({
                "nodeId": node_id,
                "forgotten": removed
            }))?))
        }
        "revoke" => {
            let device_id = arguments
                .get(1)
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| "revoke requires a device ID".to_string())?;
            let removed = node_pairing::revoke(&state_directory, device_id)?;
            Ok(Some(json(&serde_json::json!({
                "deviceId": device_id,
                "revoked": removed
            }))?))
        }
        "run" => {
            let once = arguments.iter().any(|argument| argument == "--once");
            run_service(&state_directory, once)?;
            Ok(None)
        }
        "help" | "--help" | "-h" => Ok(Some(help_text().into())),
        other => Err(format!("unknown command `{other}`\n{}", help_text())),
    }
}

fn help_text() -> &'static str {
    "Usage: lume node <enable|disable|status|run|pair|clients|revoke|discover|connect|remotes|remote-health|forget> [VALUE] [--state-dir PATH] [--once]"
}

fn positional_argument(arguments: &[String], index: usize) -> Option<&str> {
    arguments
        .iter()
        .enumerate()
        .filter(|(_, argument)| argument.as_str() != "--state-dir")
        .filter(|(position, _)| {
            *position == 0
                || arguments
                    .get(position.saturating_sub(1))
                    .is_none_or(|previous| previous != "--state-dir")
        })
        .map(|(_, argument)| argument.as_str())
        .nth(index)
}

fn state_directory(arguments: &[String]) -> Result<PathBuf, String> {
    if let Some(index) = arguments
        .iter()
        .position(|argument| argument == "--state-dir")
    {
        return arguments
            .get(index + 1)
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| "--state-dir requires a path".to_string());
    }
    if let Some(directory) = env::var_os("LUME_NODE_STATE_DIR") {
        return Ok(PathBuf::from(directory));
    }
    #[cfg(target_os = "windows")]
    if let Some(directory) = env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(directory).join("Lume").join("node"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(directory) = env::var_os("XDG_STATE_HOME") {
            return Ok(PathBuf::from(directory).join("lume").join("node"));
        }
        if let Some(directory) = env::var_os("HOME") {
            return Ok(PathBuf::from(directory)
                .join(".local")
                .join("state")
                .join("lume")
                .join("node"));
        }
    }
    Err("could not resolve the Lume Node state directory".into())
}

pub fn default_state_directory() -> Result<PathBuf, String> {
    state_directory(&[])
}

fn load_or_create_config(directory: &Path) -> Result<NodeConfig, String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    restrict_directory(directory)?;
    let path = directory.join(CONFIG_FILE);
    if path.exists() {
        let payload = fs::read(&path).map_err(|error| error.to_string())?;
        let config = serde_json::from_slice::<NodeConfig>(&payload)
            .map_err(|error| format!("invalid Node configuration: {error}"))?;
        validate_config(&config)?;
        return Ok(config);
    }
    let config = NodeConfig::default();
    save_config(directory, &config)?;
    Ok(config)
}

fn validate_config(config: &NodeConfig) -> Result<(), String> {
    if config.node_id.len() < 16
        || config.node_id.len() > 128
        || !config
            .node_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
    {
        return Err("invalid Node identity".into());
    }
    if config.display_name.trim().is_empty() || config.display_name.chars().count() > 80 {
        return Err("invalid Node display name".into());
    }
    if config.listen_port < 1024 {
        return Err("invalid Node listen port".into());
    }
    Ok(())
}

fn save_config(directory: &Path, config: &NodeConfig) -> Result<(), String> {
    validate_config(config)?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    restrict_directory(directory)?;
    write_json(&directory.join(CONFIG_FILE), config)
}

pub(crate) fn health(directory: &Path) -> Result<NodeHealth, String> {
    let config = load_or_create_config(directory)?;
    let identity = NodeIdentity::load_or_create(directory)?;
    let identity_attestation = identity.sign(serde_json::json!({
        "nodeId": config.node_id,
        "protocolMinimum": MIN_PROTOCOL_VERSION,
        "protocolMaximum": MAX_PROTOCOL_VERSION,
        "capabilities": ["session_monitoring", "resource_telemetry"]
    }))?;
    verify_node_statement(&identity_attestation)?;
    let runtime = read_runtime(directory).filter(runtime_is_live);
    let lifecycle = if !config.enabled {
        NodeLifecycle::Disabled
    } else if runtime.is_some() {
        NodeLifecycle::Running
    } else {
        NodeLifecycle::Offline
    };
    let transports = runtime
        .as_ref()
        .and_then(|runtime| runtime.network_port)
        .map(|_| vec![TransportKind::LanTlsWebSocket])
        .unwrap_or_default();
    let network = runtime.as_ref().and_then(|runtime| {
        runtime.network_port.map(|port| NodeNetworkHealth {
            port,
            discovery_available: runtime.discovery_available,
        })
    });
    let system = System::new_all();
    Ok(NodeHealth {
        lifecycle,
        enabled: config.enabled,
        node_id: config.node_id,
        display_name: config.display_name,
        identity_algorithm: identity.public().algorithm.clone(),
        identity_fingerprint: identity.public().fingerprint.clone(),
        identity_attestation,
        process_id: runtime.as_ref().map(|runtime| runtime.process_id),
        started_at: runtime.as_ref().map(|runtime| runtime.started_at),
        heartbeat_at: runtime.as_ref().map(|runtime| runtime.heartbeat_at),
        protocol_minimum: MIN_PROTOCOL_VERSION,
        protocol_maximum: MAX_PROTOCOL_VERSION,
        capabilities: vec![
            NodeCapability::SessionMonitoring,
            NodeCapability::ResourceTelemetry,
        ],
        transports,
        network,
        machine: MachineSnapshot {
            hostname: System::host_name().unwrap_or_else(|| "unknown".into()),
            operating_system: System::long_os_version().unwrap_or_else(|| env::consts::OS.into()),
            architecture: env::consts::ARCH.into(),
            logical_cpu_count: system.cpus().len(),
            total_memory_bytes: system.total_memory(),
            available_memory_bytes: system.available_memory(),
        },
    })
}

fn run_service(directory: &Path, once: bool) -> Result<(), String> {
    run_service_inner(directory, once, true)
}

fn run_service_inner(directory: &Path, once: bool, start_network: bool) -> Result<(), String> {
    let config = load_or_create_config(directory)?;
    if !config.enabled {
        return Err("the Node is disabled; run `lume node enable` first".into());
    }
    let identity = NodeIdentity::load_or_create(directory)?;
    let _lock = NodeLock::acquire(directory)?;
    let network = start_network
        .then(|| {
            NodeNetworkServer::start(directory, &config.node_id, config.listen_port, &identity)
        })
        .transpose()?;
    let process_started_at = process_start_time(process::id())
        .ok_or_else(|| "could not identify the running Node process".to_string())?;
    let started_at = now_millis();
    append_log(
        directory,
        &format!("Node started with pid {}", process::id()),
    )?;
    let mut last_heartbeat = 0;

    loop {
        let now = now_millis();
        if now.saturating_sub(last_heartbeat) >= HEARTBEAT_INTERVAL_MS {
            let runtime = NodeRuntime {
                process_id: process::id(),
                process_started_at,
                started_at,
                heartbeat_at: now,
                network_port: network.as_ref().map(|server| server.port),
                discovery_available: network
                    .as_ref()
                    .is_some_and(|server| server.discovery_available),
            };
            write_json(&directory.join(RUNTIME_FILE), &runtime)?;
            last_heartbeat = now;
        }
        if once {
            break;
        }
        thread::sleep(CONTROL_POLL_INTERVAL);
        if !load_or_create_config(directory)?.enabled {
            append_log(directory, "Node stopped because it was disabled")?;
            break;
        }
    }
    let _ = fs::remove_file(directory.join(RUNTIME_FILE));
    Ok(())
}

fn read_runtime(directory: &Path) -> Option<NodeRuntime> {
    let payload = fs::read(directory.join(RUNTIME_FILE)).ok()?;
    serde_json::from_slice(&payload).ok()
}

fn runtime_is_live(runtime: &NodeRuntime) -> bool {
    if now_millis().saturating_sub(runtime.heartbeat_at) > HEARTBEAT_STALE_MS {
        return false;
    }
    process_instance_is_live(runtime.process_id, runtime.process_started_at)
}

pub(crate) fn process_start_time(process_id: u32) -> Option<u64> {
    let system = System::new_all();
    system
        .process(Pid::from_u32(process_id))
        .map(|process| process.start_time())
}

pub(crate) fn process_instance_is_live(process_id: u32, started_at: u64) -> bool {
    process_start_time(process_id).is_some_and(|actual| actual == started_at)
}

struct NodeLock {
    path: PathBuf,
}

impl NodeLock {
    fn acquire(directory: &Path) -> Result<Self, String> {
        let path = directory.join(LOCK_FILE);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let started_at = process_start_time(process::id())
                    .ok_or_else(|| "could not identify the Node process".to_string())?;
                writeln!(file, "{} {started_at}", process::id())
                    .map_err(|error| error.to_string())?;
                restrict_file(&path)?;
                Ok(Self { path })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let owner_is_live = fs::read_to_string(&path).ok().is_some_and(|value| {
                    let mut parts = value.split_whitespace();
                    let process_id = parts.next().and_then(|part| part.parse::<u32>().ok());
                    let started_at = parts.next().and_then(|part| part.parse::<u64>().ok());
                    process_id
                        .zip(started_at)
                        .is_some_and(|(id, start)| process_instance_is_live(id, start))
                });
                if owner_is_live {
                    return Err("another Lume Node process is already running".into());
                }
                fs::remove_file(&path).map_err(|error| error.to_string())?;
                Self::acquire(directory)
            }
            Err(error) => Err(error.to_string()),
        }
    }
}

impl Drop for NodeLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("tmp-{}", process::id()));
    fs::write(&temporary, payload).map_err(|error| error.to_string())?;
    restrict_file(&temporary)?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, path).map_err(|error| error.to_string())?;
    Ok(())
}

fn append_log(directory: &Path, message: &str) -> Result<(), String> {
    let path = directory.join(LOG_FILE);
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES) {
        let previous = directory.join("node.previous.log");
        let _ = fs::remove_file(&previous);
        fs::rename(&path, previous).map_err(|error| error.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    writeln!(file, "{} {message}", now_millis()).map_err(|error| error.to_string())?;
    restrict_file(&path)
}

fn json(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|error| error.to_string())
}

fn new_node_id() -> String {
    let mut bytes = [0_u8; 16];
    if getrandom::getrandom(&mut bytes).is_err() {
        return format!("node-{}-{}", process::id(), now_millis());
    }
    let encoded = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("node-{encoded}")
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(name: &str) -> PathBuf {
        let mut random = [0_u8; 8];
        getrandom::getrandom(&mut random).expect("random test directory");
        env::temp_dir().join(format!(
            "lume-node-{name}-{}",
            random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ))
    }

    #[test]
    fn config_starts_disabled_and_keeps_a_stable_identity() {
        let directory = test_directory("identity");
        let first = load_or_create_config(&directory).expect("first config");
        let second = load_or_create_config(&directory).expect("second config");
        assert!(!first.enabled);
        assert_eq!(first.node_id, second.node_id);
        assert!(first.node_id.starts_with("node-"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn enable_and_disable_are_explicit_and_persisted() {
        let directory = test_directory("lifecycle");
        run_cli_inner(&[
            "enable".into(),
            "--state-dir".into(),
            directory.to_string_lossy().into_owned(),
        ])
        .expect("enable Node");
        assert!(load_or_create_config(&directory).expect("config").enabled);
        run_cli_inner(&[
            "disable".into(),
            "--state-dir".into(),
            directory.to_string_lossy().into_owned(),
        ])
        .expect("disable Node");
        assert!(!load_or_create_config(&directory).expect("config").enabled);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn stale_runtime_is_not_reported_as_running() {
        let directory = test_directory("stale");
        let mut config = load_or_create_config(&directory).expect("config");
        config.enabled = true;
        save_config(&directory, &config).expect("save config");
        write_json(
            &directory.join(RUNTIME_FILE),
            &NodeRuntime {
                process_id: process::id(),
                process_started_at: process_start_time(process::id()).expect("process start"),
                started_at: 1,
                heartbeat_at: 1,
                network_port: None,
                discovery_available: false,
            },
        )
        .expect("runtime");
        assert_eq!(
            health(&directory).expect("health").lifecycle,
            NodeLifecycle::Offline
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn one_shot_run_cleans_runtime_and_lock_files() {
        let directory = test_directory("once");
        let mut config = load_or_create_config(&directory).expect("config");
        config.enabled = true;
        save_config(&directory, &config).expect("save config");
        run_service_inner(&directory, true, false).expect("one-shot service");
        assert!(!directory.join(RUNTIME_FILE).exists());
        assert!(!directory.join(LOCK_FILE).exists());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn lock_rejects_a_second_live_node() {
        let directory = test_directory("lock");
        fs::create_dir_all(&directory).expect("state directory");
        let first = NodeLock::acquire(&directory).expect("first lock");
        assert_eq!(
            NodeLock::acquire(&directory).err().as_deref(),
            Some("another Lume Node process is already running")
        );
        drop(first);
        let _ = fs::remove_dir_all(directory);
    }
}
