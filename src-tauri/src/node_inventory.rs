//! Read-only inventory. Detected processes do not become Node-owned sessions.

use std::{
    collections::HashSet,
    io::Write,
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sysinfo::System;

use crate::{
    discovery,
    distributed_protocol::ExecutionOrigin,
    domain::AgentKind,
    node_http::{read_json_response, DeadlineStream},
    node_service,
    state::now_millis,
};

const SNAPSHOT_TTL: Duration = Duration::from_secs(5);
const RUNTIME_DEADLINE: Duration = Duration::from_secs(2);
const MAX_RUNTIME_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_AGENTS: usize = 128;
const MAX_MODELS: usize = 128;
const MAX_LABEL_CHARS: usize = 256;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInventory {
    pub node_id: String,
    pub observed_at: i64,
    pub agents: Vec<NodeAgentSnapshot>,
    pub agents_truncated: bool,
    pub runtimes: Vec<LocalRuntimeSnapshot>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeAgentSnapshot {
    /// Node + process start time prevents identity reuse after a PID is recycled.
    pub id: String,
    pub agent: AgentKind,
    pub agent_label: String,
    pub started_at: u64,
    pub origin: ExecutionOrigin,
    pub can_control: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAvailability {
    Available,
    Unavailable,
    InvalidResponse,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRuntimeSnapshot {
    pub id: String,
    pub availability: RuntimeAvailability,
    pub models: Vec<RuntimeModelSnapshot>,
    pub models_truncated: bool,
    pub loaded_state_known: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelSnapshot {
    pub name: String,
    pub digest: String,
    pub size_bytes: Option<u64>,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
    /// None means the runtime's loaded-model probe failed, not "unloaded".
    pub loaded: Option<bool>,
}

pub(crate) struct InventoryCache {
    system: System,
    cached: Option<(Instant, NodeInventory)>,
}

impl Default for InventoryCache {
    fn default() -> Self {
        Self {
            system: System::new(),
            cached: None,
        }
    }
}

impl InventoryCache {
    pub(crate) fn snapshot(&mut self, directory: &Path) -> Result<NodeInventory, String> {
        let config = node_service::load_or_create_config(directory)?;
        if !config.enabled {
            return Err("the Node is disabled".into());
        }
        if let Some((at, snapshot)) = &self.cached {
            if at.elapsed() < SNAPSHOT_TTL {
                return Ok(snapshot.clone());
            }
        }
        let mut agents = discovery::read_only_process_snapshot(&mut self.system)
            .into_iter()
            .map(|process| {
                let fingerprint = Sha256::digest(format!(
                    "lume-node-process-v1:{}:{}:{}",
                    config.node_id, process.process_id, process.started_at
                ));
                NodeAgentSnapshot {
                    id: format!("process:{fingerprint:x}"),
                    agent: process.agent,
                    agent_label: bounded_label(&process.agent_label),
                    started_at: process.started_at.saturating_mul(1_000),
                    origin: ExecutionOrigin::ExternalObserved,
                    can_control: false,
                }
            })
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| {
            left.agent_label
                .cmp(&right.agent_label)
                .then_with(|| left.id.cmp(&right.id))
        });
        let agents_truncated = agents.len() > MAX_AGENTS;
        agents.truncate(MAX_AGENTS);
        let runtimes = if config.ollama_inventory_enabled {
            vec![ollama_inventory(config.ollama_port)]
        } else {
            Vec::new()
        };
        let snapshot = NodeInventory {
            node_id: config.node_id,
            observed_at: now_millis(),
            agents,
            agents_truncated,
            runtimes,
        };
        self.cached = Some((Instant::now(), snapshot.clone()));
        Ok(snapshot)
    }
}

fn ollama_inventory(port: u16) -> LocalRuntimeSnapshot {
    let mut runtime = LocalRuntimeSnapshot {
        id: "ollama".into(),
        availability: RuntimeAvailability::Unavailable,
        models: Vec::new(),
        models_truncated: false,
        loaded_state_known: false,
    };
    let deadline = Instant::now() + RUNTIME_DEADLINE;
    let Ok(tags) = ollama_get(port, "/api/tags", deadline) else {
        return runtime;
    };
    let Some(models) = tags.get("models").and_then(Value::as_array) else {
        runtime.availability = RuntimeAvailability::InvalidResponse;
        return runtime;
    };
    let loaded = ollama_get(port, "/api/ps", deadline)
        .ok()
        .and_then(|response| {
            response["models"].as_array().map(|models| {
                models
                    .iter()
                    .filter_map(|model| model["name"].as_str())
                    .map(str::to_owned)
                    .collect::<HashSet<_>>()
            })
        });
    runtime.loaded_state_known = loaded.is_some();
    runtime.availability = RuntimeAvailability::Available;
    runtime.models_truncated = models.len() > MAX_MODELS;
    let mut names = HashSet::new();
    runtime.models = models
        .iter()
        .take(MAX_MODELS)
        .filter_map(|model| {
            let name = model["name"].as_str()?;
            if name.is_empty()
                || name.chars().count() > MAX_LABEL_CHARS
                || name.chars().any(char::is_control)
                || !names.insert(name.to_owned())
            {
                runtime.models_truncated = true;
                return None;
            }
            Some(RuntimeModelSnapshot {
                name: name.into(),
                digest: bounded_label(model["digest"].as_str().unwrap_or_default()),
                size_bytes: model["size"].as_u64(),
                parameter_size: model["details"]["parameter_size"]
                    .as_str()
                    .map(bounded_label),
                quantization: model["details"]["quantization_level"]
                    .as_str()
                    .map(bounded_label),
                loaded: loaded.as_ref().map(|names| names.contains(name)),
            })
        })
        .collect();
    runtime
        .models
        .sort_by(|left, right| left.name.cmp(&right.name));
    runtime
}

fn ollama_get(port: u16, path: &str, deadline: Instant) -> Result<Value, String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or("local runtime deadline exceeded")?;
    // No DNS, environment proxy, redirects or user-supplied URL. Only PC1 loopback.
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let socket = TcpStream::connect_timeout(&address, remaining)
        .map_err(|_| "local runtime is unavailable")?;
    let mut stream = DeadlineStream::new(socket, deadline);
    write!(
        stream,
        "GET {path} HTTP/1.0\r\nHost: localhost\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )
    .and_then(|_| stream.flush())
    .map_err(|error| error.to_string())?;
    read_json_response(&mut stream, MAX_RUNTIME_RESPONSE_BYTES)
}

fn bounded_label(label: &str) -> String {
    label
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_LABEL_CHARS)
        .collect()
}
