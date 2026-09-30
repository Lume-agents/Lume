use std::{
    collections::{HashMap, VecDeque},
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{IpAddr, TcpListener, TcpStream},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use rcgen::{CertificateParams, DnType, KeyPair};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    ServerConfig, ServerConnection, StreamOwned,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    distributed_protocol::{NodeScope, MAX_PROTOCOL_VERSION, MIN_PROTOCOL_VERSION},
    node_http::DeadlineStream,
    node_identity::{verify, NodeIdentity, PublicDeviceIdentity, SignedDeviceStatement},
    node_inventory::InventoryCache,
    node_pairing::{self, NodePairingRequest},
    node_service,
    state::now_millis,
};

const CERTIFICATE_FILE: &str = "node-tls.der";
const PRIVATE_KEY_FILE: &str = "node-tls-key.der";
const MDNS_SERVICE_TYPE: &str = "_lume-node._tcp.local.";
const MAX_CONNECTIONS: usize = 8;
const MAX_PEER_CONNECTIONS: usize = 2;
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);
const HEALTH_CACHE_TTL: Duration = Duration::from_secs(5);
const OBSERVE_RATE_WINDOW: Duration = Duration::from_secs(10);
const MAX_OBSERVE_REQUESTS: usize = 8;
const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
const MAX_HTTP_BODY_BYTES: usize = 64 * 1024;
const AUTH_CLOCK_WINDOW_MS: u64 = 60_000;
const MAX_REPLAY_NONCES: usize = 4_096;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeServerIdentity {
    pub node_id: String,
    pub identity: PublicDeviceIdentity,
    pub certificate_sha256: String,
    pub protocol_minimum: u16,
    pub protocol_maximum: u16,
    pub binding_attestation: SignedDeviceStatement,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedNodeRequest {
    pub device_id: String,
    pub proof: SignedDeviceStatement,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticationProof {
    device_id: String,
    node_id: String,
    method: String,
    path: String,
    timestamp: i64,
    nonce: String,
}

pub struct NodeNetworkServer {
    running: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
    _discovery: Option<MdnsAdvertisement>,
    pub address: IpAddr,
    pub port: u16,
    pub discovery_available: bool,
    pub identity: NodeServerIdentity,
}

impl NodeNetworkServer {
    pub fn start(
        directory: &Path,
        node_id: &str,
        address: IpAddr,
        port: u16,
        identity: &NodeIdentity,
    ) -> Result<Self, String> {
        validate_listen_address(address)?;
        let local_ip = (!address.is_loopback()).then_some(address);
        let tls = NodeTlsIdentity::load_or_create(directory, local_ip)?;
        let server_identity = server_identity(node_id, identity, &tls.certificate_sha256)?;
        let listener = TcpListener::bind((address, port))
            .map_err(|error| format!("could not bind the Lume Node TLS listener: {error}"))?;
        let bound_port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;

        let discovery = local_ip.and_then(|ip| {
            start_mdns_advertisement(ip, bound_port, &server_identity)
                .map_err(|error| eprintln!("Lume Node discovery unavailable: {error}"))
                .ok()
        });
        let discovery_available = discovery.is_some();
        let running = Arc::new(AtomicBool::new(true));
        let active_connections = Arc::new(Mutex::new(ConnectionCounts::default()));
        let state = Arc::new(NodeNetworkState {
            directory: directory.to_path_buf(),
            server_identity: server_identity.clone(),
            replay: Mutex::new(ReplayCache::default()),
            observe_budget: Mutex::new(ObserveBudget::default()),
            health: Mutex::new(None),
            inventory: Mutex::new(InventoryCache::default()),
        });
        let tls_config = tls.server_config;
        let thread_running = running.clone();
        let listener_thread = thread::Builder::new()
            .name("lume-node-tls-listener".into())
            .spawn(move || {
                while thread_running.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, peer)) => {
                            let Some(permit) =
                                ConnectionPermit::acquire(active_connections.clone(), peer.ip())
                            else {
                                drop(stream);
                                continue;
                            };
                            let connection_state = state.clone();
                            let connection_config = tls_config.clone();
                            let _ = thread::Builder::new()
                                .name("lume-node-tls-client".into())
                                .spawn(move || {
                                    let _permit = permit;
                                    handle_tls_connection(
                                        stream,
                                        connection_config,
                                        connection_state,
                                    );
                                });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(50));
                        }
                        Err(_) => thread::sleep(Duration::from_millis(100)),
                    }
                }
            })
            .map_err(|error| error.to_string())?;

        Ok(Self {
            running,
            listener_thread: Some(listener_thread),
            _discovery: discovery,
            address,
            port: bound_port,
            discovery_available,
            identity: server_identity,
        })
    }
}

impl Drop for NodeNetworkServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(thread) = self.listener_thread.take() {
            let _ = thread.join();
        }
    }
}

struct NodeNetworkState {
    directory: std::path::PathBuf,
    server_identity: NodeServerIdentity,
    replay: Mutex<ReplayCache>,
    observe_budget: Mutex<ObserveBudget>,
    health: Mutex<Option<(Instant, node_service::NodeHealth)>>,
    inventory: Mutex<InventoryCache>,
}

#[derive(Default)]
struct ObserveBudget {
    devices: HashMap<String, (Instant, usize)>,
}

impl ObserveBudget {
    fn accept(&mut self, device_id: &str) -> bool {
        self.devices
            .retain(|_, (at, _)| at.elapsed() < OBSERVE_RATE_WINDOW);
        if self.devices.len() >= MAX_REPLAY_NONCES && !self.devices.contains_key(device_id) {
            return false;
        }
        let (_, requests) = self
            .devices
            .entry(device_id.into())
            .or_insert((Instant::now(), 0));
        if *requests >= MAX_OBSERVE_REQUESTS {
            return false;
        }
        *requests += 1;
        true
    }
}

#[derive(Default)]
struct ReplayCache {
    entries: HashMap<(String, String), i64>,
    order: VecDeque<(String, String)>,
}

impl ReplayCache {
    fn accept(&mut self, device_id: &str, nonce: &str, now: i64) -> Result<(), String> {
        self.entries.retain(|_, expires_at| *expires_at >= now);
        while self
            .order
            .front()
            .is_some_and(|key| !self.entries.contains_key(key))
        {
            self.order.pop_front();
        }
        let key = (device_id.to_string(), nonce.to_string());
        if self.entries.contains_key(&key) {
            return Err("replayed Node request".into());
        }
        if self.entries.len() >= MAX_REPLAY_NONCES {
            return Err("Node replay cache is full".into());
        }
        self.entries.insert(
            key.clone(),
            now.saturating_add((AUTH_CLOCK_WINDOW_MS * 2) as i64),
        );
        self.order.push_back(key);
        Ok(())
    }
}

#[derive(Default)]
struct ConnectionCounts {
    total: usize,
    peers: HashMap<IpAddr, usize>,
}

struct ConnectionPermit {
    active: Arc<Mutex<ConnectionCounts>>,
    peer: IpAddr,
}

impl ConnectionPermit {
    fn acquire(active: Arc<Mutex<ConnectionCounts>>, peer: IpAddr) -> Option<Self> {
        {
            let mut counts = active.lock().ok()?;
            if counts.total >= MAX_CONNECTIONS
                || counts.peers.get(&peer).copied().unwrap_or_default() >= MAX_PEER_CONNECTIONS
            {
                return None;
            }
            counts.total += 1;
            *counts.peers.entry(peer).or_default() += 1;
        }
        Some(Self { active, peer })
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.active.lock() {
            counts.total = counts.total.saturating_sub(1);
            if let Some(active) = counts.peers.get_mut(&self.peer) {
                *active = active.saturating_sub(1);
                if *active == 0 {
                    counts.peers.remove(&self.peer);
                }
            }
        }
    }
}

struct NodeTlsIdentity {
    server_config: Arc<ServerConfig>,
    certificate_sha256: String,
}

impl NodeTlsIdentity {
    fn load_or_create(directory: &Path, local_ip: Option<IpAddr>) -> Result<Self, String> {
        let certificate_path = directory.join(CERTIFICATE_FILE);
        let private_key_path = directory.join(PRIVATE_KEY_FILE);
        let certificate_exists = certificate_path.exists();
        let private_key_exists = private_key_path.exists();
        if certificate_exists != private_key_exists {
            return Err("the persisted Node TLS identity is incomplete".into());
        }
        if !certificate_exists {
            let mut hosts = vec!["localhost".into(), "127.0.0.1".into(), "::1".into()];
            if let Some(ip) = local_ip {
                hosts.push(ip.to_string());
            }
            let mut parameters =
                CertificateParams::new(hosts).map_err(|error| error.to_string())?;
            parameters
                .distinguished_name
                .push(DnType::OrganizationName, "Lume Local");
            parameters
                .distinguished_name
                .push(DnType::CommonName, "Lume Node");
            let key = KeyPair::generate().map_err(|error| error.to_string())?;
            let certificate = parameters
                .self_signed(&key)
                .map_err(|error| error.to_string())?;
            write_restricted(&private_key_path, &key.serialize_der())?;
            if let Err(error) = write_restricted(&certificate_path, certificate.der()) {
                let _ = fs::remove_file(&private_key_path);
                return Err(error);
            }
        }

        let certificate = fs::read(&certificate_path).map_err(|error| error.to_string())?;
        let private_key = fs::read(&private_key_path).map_err(|error| error.to_string())?;
        let certificate_sha256 = sha256_hex(&certificate);
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(certificate)],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key)),
            )
            .map_err(|_| "the persisted Node TLS certificate and key do not match".to_string())?;
        Ok(Self {
            server_config: Arc::new(server_config),
            certificate_sha256,
        })
    }
}

struct MdnsAdvertisement {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Drop for MdnsAdvertisement {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

fn start_mdns_advertisement(
    ip: IpAddr,
    port: u16,
    identity: &NodeServerIdentity,
) -> Result<MdnsAdvertisement, String> {
    let suffix = identity
        .identity
        .fingerprint
        .chars()
        .take(12)
        .collect::<String>();
    let instance = format!("Lume-Node-{suffix}");
    let hostname = format!("lume-node-{suffix}.local.");
    let properties = HashMap::from([
        ("id".to_string(), identity.node_id.clone()),
        (
            "fingerprint".to_string(),
            identity.identity.fingerprint.clone(),
        ),
        (
            "certificate".to_string(),
            identity.certificate_sha256.clone(),
        ),
        (
            "protocolMin".to_string(),
            identity.protocol_minimum.to_string(),
        ),
        (
            "protocolMax".to_string(),
            identity.protocol_maximum.to_string(),
        ),
        ("transport".to_string(), "tls".to_string()),
    ]);
    let info = ServiceInfo::new(
        MDNS_SERVICE_TYPE,
        &instance,
        &hostname,
        ip,
        port,
        properties,
    )
    .map_err(|error| error.to_string())?;
    let fullname = info.get_fullname().to_string();
    let daemon = ServiceDaemon::new().map_err(|error| error.to_string())?;
    daemon.register(info).map_err(|error| error.to_string())?;
    Ok(MdnsAdvertisement { daemon, fullname })
}

pub(crate) fn validate_listen_address(address: IpAddr) -> Result<(), String> {
    let private = match address {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_private()
                || ip.is_link_local()
                // Explicitly selected Tailscale/private CGNAT interface.
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        IpAddr::V6(ip) => ip.is_unique_local(),
    };
    if address.is_loopback() || private {
        Ok(())
    } else {
        Err(
            "select a loopback or private interface IP; wildcard and public listeners are disabled"
                .into(),
        )
    }
}

fn server_identity(
    node_id: &str,
    identity: &NodeIdentity,
    certificate_sha256: &str,
) -> Result<NodeServerIdentity, String> {
    let binding_attestation = identity.sign(serde_json::json!({
        "nodeId": node_id,
        "certificateSha256": certificate_sha256,
        "protocolMinimum": MIN_PROTOCOL_VERSION,
        "protocolMaximum": MAX_PROTOCOL_VERSION
    }))?;
    verify(&binding_attestation)?;
    Ok(NodeServerIdentity {
        node_id: node_id.into(),
        identity: identity.public().clone(),
        certificate_sha256: certificate_sha256.into(),
        protocol_minimum: MIN_PROTOCOL_VERSION,
        protocol_maximum: MAX_PROTOCOL_VERSION,
        binding_attestation,
    })
}

pub fn authenticated_request(
    client_identity: &NodeIdentity,
    device_id: &str,
    node_id: &str,
    method: &str,
    path: &str,
    timestamp: i64,
) -> Result<AuthenticatedNodeRequest, String> {
    let mut nonce = [0_u8; 18];
    getrandom::getrandom(&mut nonce).map_err(|error| error.to_string())?;
    let payload = AuthenticationProof {
        device_id: device_id.into(),
        node_id: node_id.into(),
        method: method.to_ascii_uppercase(),
        path: path.into(),
        timestamp,
        nonce: URL_SAFE_NO_PAD.encode(nonce),
    };
    Ok(AuthenticatedNodeRequest {
        device_id: device_id.into(),
        proof: client_identity
            .sign(serde_json::to_value(payload).map_err(|error| error.to_string())?)?,
    })
}

fn verify_authenticated_request(
    directory: &Path,
    server_identity: &NodeServerIdentity,
    request: &AuthenticatedNodeRequest,
    method: &str,
    path: &str,
    replay: &Mutex<ReplayCache>,
    now: i64,
) -> Result<(), String> {
    verify(&request.proof)?;
    let paired = node_pairing::clients(directory)?
        .into_iter()
        .find(|client| client.device_id == request.device_id)
        .ok_or_else(|| "unpaired Node client".to_string())?;
    if paired.identity != request.proof.identity || !paired.scopes.contains(&NodeScope::Observe) {
        return Err("Node client identity or scope mismatch".into());
    }
    let proof = serde_json::from_value::<AuthenticationProof>(request.proof.payload.clone())
        .map_err(|_| "invalid Node authentication proof".to_string())?;
    if proof.device_id != request.device_id
        || proof.node_id != server_identity.node_id
        || proof.method != method.to_ascii_uppercase()
        || proof.path != path
        || now.abs_diff(proof.timestamp) > AUTH_CLOCK_WINDOW_MS
    {
        return Err("Node authentication proof does not match the request".into());
    }
    let nonce = URL_SAFE_NO_PAD
        .decode(&proof.nonce)
        .map_err(|_| "invalid Node authentication nonce".to_string())?;
    if nonce.len() < 16 || nonce.len() > 64 {
        return Err("invalid Node authentication nonce".into());
    }
    replay
        .lock()
        .map_err(|_| "Node replay cache is unavailable".to_string())?
        .accept(&request.device_id, &proof.nonce, now)
}

fn handle_tls_connection(
    stream: TcpStream,
    config: Arc<ServerConfig>,
    state: Arc<NodeNetworkState>,
) {
    let stream = DeadlineStream::new(stream, Instant::now() + REQUEST_DEADLINE);
    let Ok(connection) = ServerConnection::new(config) else {
        return;
    };
    let mut stream = StreamOwned::new(connection, stream);
    let response = match read_http_request(&mut stream) {
        Ok(request) => route(&request, &state),
        Err(error) => json_response(400, serde_json::json!({ "error": error })),
    };
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    stream.conn.send_close_notify();
    let _ = stream.conn.complete_io(&mut stream.sock);
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut impl Read) -> Result<HttpRequest, String> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("incomplete Node request".into());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_bytes(&buffer, b"\r\n\r\n") {
            break index + 4;
        }
        if buffer.len() > MAX_HTTP_HEADER_BYTES {
            return Err("Node request headers are too large".into());
        }
    };
    if header_end > MAX_HTTP_HEADER_BYTES {
        return Err("Node request headers are too large".into());
    }
    let headers = std::str::from_utf8(&buffer[..header_end])
        .map_err(|_| "invalid Node request headers".to_string())?;
    let mut lines = headers.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "missing Node request line".to_string())?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let path = request_parts.next().unwrap_or_default().to_string();
    let version = request_parts.next();
    if method.is_empty()
        || !path.starts_with('/')
        || !matches!(version, Some("HTTP/1.0" | "HTTP/1.1"))
        || request_parts.next().is_some()
    {
        return Err("invalid Node request line".into());
    }
    let mut content_length = None;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err("invalid Node request header".into());
        };
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("Node requests do not support transfer encoding".into());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err("duplicate Node content length".into());
            }
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "invalid Node content length".to_string())?,
            );
        }
    }
    let content_length = content_length.unwrap_or_default();
    if content_length > MAX_HTTP_BODY_BYTES {
        return Err("Node request body is too large".into());
    }
    if buffer.len().saturating_sub(header_end) > content_length {
        return Err("unexpected data after Node request".into());
    }
    while buffer.len().saturating_sub(header_end) < content_length {
        let remaining = content_length - buffer.len().saturating_sub(header_end);
        let read_limit = remaining.min(chunk.len());
        let read = stream
            .read(&mut chunk[..read_limit])
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("incomplete Node request body".into());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len().saturating_sub(header_end) > MAX_HTTP_BODY_BYTES {
            return Err("Node request body is too large".into());
        }
    }
    Ok(HttpRequest {
        method,
        path,
        body: buffer[header_end..header_end + content_length].to_vec(),
    })
}

fn route(request: &HttpRequest, state: &NodeNetworkState) -> String {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/identity") => json_response(
            200,
            serde_json::to_value(&state.server_identity).unwrap_or(Value::Null),
        ),
        ("POST", "/v1/pair") => {
            let request = match serde_json::from_slice::<NodePairingRequest>(&request.body) {
                Ok(request) => request,
                Err(_) => {
                    return json_response(
                        400,
                        serde_json::json!({ "error": "invalid pairing request" }),
                    );
                }
            };
            match node_pairing::accept_pairing(
                &state.directory,
                &request,
                &[NodeScope::Observe],
                now_millis(),
            ) {
                Ok(client) => {
                    json_response(200, serde_json::json!({ "paired": true, "client": client }))
                }
                Err(error) => json_response(403, serde_json::json!({ "error": error })),
            }
        }
        ("POST", "/v1/health" | "/v1/inventory") => {
            let authentication =
                match serde_json::from_slice::<AuthenticatedNodeRequest>(&request.body) {
                    Ok(authentication) => authentication,
                    Err(_) => {
                        return json_response(
                            400,
                            serde_json::json!({ "error": "invalid authentication request" }),
                        );
                    }
                };
            match verify_authenticated_request(
                &state.directory,
                &state.server_identity,
                &authentication,
                "POST",
                &request.path,
                &state.replay,
                now_millis(),
            ) {
                Ok(()) => observe_response(state, &authentication.device_id, &request.path),
                Err(_) => json_response(
                    401,
                    serde_json::json!({ "error": "Node authentication failed" }),
                ),
            }
        }
        _ => json_response(404, serde_json::json!({ "error": "not found" })),
    }
}

fn observe_response(state: &NodeNetworkState, device_id: &str, path: &str) -> String {
    if !state
        .observe_budget
        .lock()
        .is_ok_and(|mut budget| budget.accept(device_id))
    {
        return json_response(
            429,
            serde_json::json!({ "error": "Node observation rate limit exceeded" }),
        );
    }
    let snapshot = if path == "/v1/inventory" {
        state
            .inventory
            .lock()
            .map_err(|_| "inventory unavailable".to_string())
            .and_then(|mut cache| cache.snapshot(&state.directory))
            .and_then(|inventory| {
                serde_json::to_value(inventory).map_err(|error| error.to_string())
            })
    } else {
        state
            .health
            .lock()
            .map_err(|_| "health unavailable".to_string())
            .and_then(|mut cache| {
                if let Some((at, health)) = cache.as_ref() {
                    if at.elapsed() < HEALTH_CACHE_TTL {
                        return Ok(health.clone());
                    }
                }
                let health = node_service::health(&state.directory)?;
                *cache = Some((Instant::now(), health.clone()));
                Ok(health)
            })
            .and_then(|health| serde_json::to_value(health).map_err(|error| error.to_string()))
    };
    match snapshot {
        Ok(snapshot) => json_response(200, snapshot),
        Err(_) => json_response(
            503,
            serde_json::json!({ "error": "Node snapshot is unavailable" }),
        ),
    }
}

fn json_response(status: u16, value: Value) -> String {
    let body = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        403 => "Forbidden",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Error",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(unix)]
fn write_restricted(path: &Path, value: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(value).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn write_restricted(path: &Path, value: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(value).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, io::Cursor, path::PathBuf};

    fn test_directory(name: &str) -> PathBuf {
        let mut random = [0_u8; 8];
        getrandom::getrandom(&mut random).expect("random test directory");
        env::temp_dir().join(format!(
            "lume-node-network-{name}-{}",
            URL_SAFE_NO_PAD.encode(random)
        ))
    }

    #[test]
    fn tls_identity_is_persistent_and_bound_to_the_node_identity() {
        let directory = test_directory("identity");
        fs::create_dir_all(&directory).expect("directory");
        let node = NodeIdentity::load_or_create(&directory).expect("node identity");
        let first = NodeTlsIdentity::load_or_create(&directory, None).expect("first TLS identity");
        let second =
            NodeTlsIdentity::load_or_create(&directory, None).expect("second TLS identity");
        assert_eq!(first.certificate_sha256, second.certificate_sha256);
        let binding = server_identity("node-home", &node, &first.certificate_sha256)
            .expect("server identity");
        assert_eq!(verify(&binding.binding_attestation), Ok(()));
        assert_eq!(
            binding.binding_attestation.payload["certificateSha256"],
            first.certificate_sha256
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn authenticated_health_proof_is_scoped_and_replay_safe() {
        let directory = test_directory("auth");
        let client_directory = test_directory("client");
        let node = NodeIdentity::load_or_create(&directory).expect("node identity");
        let client = NodeIdentity::load_or_create(&client_directory).expect("client identity");
        let offer = node_pairing::begin_pairing(&directory, "node-home", &node).expect("offer");
        let pairing =
            node_pairing::pairing_request(&client, &offer, "Work laptop", vec![NodeScope::Observe])
                .expect("pairing request");
        let paired =
            node_pairing::accept_pairing(&directory, &pairing, &[NodeScope::Observe], now_millis())
                .expect("paired client");
        let tls = NodeTlsIdentity::load_or_create(&directory, None).expect("TLS identity");
        let server =
            server_identity("node-home", &node, &tls.certificate_sha256).expect("server identity");
        let request = authenticated_request(
            &client,
            &paired.device_id,
            "node-home",
            "POST",
            "/v1/health",
            now_millis(),
        )
        .expect("authenticated request");
        let replay = Mutex::new(ReplayCache::default());
        assert_eq!(
            verify_authenticated_request(
                &directory,
                &server,
                &request,
                "POST",
                "/v1/health",
                &replay,
                now_millis()
            ),
            Ok(())
        );
        assert_eq!(
            verify_authenticated_request(
                &directory,
                &server,
                &request,
                "POST",
                "/v1/health",
                &replay,
                now_millis()
            )
            .err()
            .as_deref(),
            Some("replayed Node request")
        );
        let _ = fs::remove_dir_all(directory);
        let _ = fs::remove_dir_all(client_directory);
    }

    #[test]
    fn http_reader_rejects_oversized_bodies() {
        let request = format!(
            "POST /v1/pair HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_HTTP_BODY_BYTES + 1
        );
        assert_eq!(
            read_http_request(&mut Cursor::new(request))
                .err()
                .as_deref(),
            Some("Node request body is too large")
        );
    }
}
