//! Lume Node access through the Relay: the Node side (a worker thread inside `lume node run`) and the
//! device side (`lume node relay-pair`, `relay-health`, `relay-inventory`).
//!
//! The Relay only forwards ciphertext. Pairing is protected by a "pairing box" keyed from the offer's secret
//! (which travels in the QR code, never through the Relay), and everything after pairing runs inside the
//! authenticated end-to-end session of [`crate::relay_e2e`].

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305},
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::{
    distributed_protocol::NodeScope,
    node_audit, node_auth,
    node_identity::{NodeIdentity, PublicDeviceIdentity},
    node_pairing::{self, NodePairingOffer, NodePairingRequest, PairedNodeClient},
    node_service,
    relay_e2e::{self, fingerprint, Session},
    relay_link::{RelayEvent, RelayLink, Role},
};

const OFFER_FILE: &str = "relay-offer.json";
const REMOTES_FILE: &str = "relay-remotes.json";
const CLIENT_IDENTITY_DIRECTORY: &str = "client-identity";
const PAIR_REQUEST: u8 = 0x10;
const PAIR_RESPONSE: u8 = 0x11;
const PAIR_AAD: &[u8] = b"lume-relay-pair-v1";
/// Plaintext answer of a Node to a sealed frame it has no session for: the type byte and 16 bytes of the frame's digest.
const RESET: u8 = 0x04;
const NODE_RESET: &str = "the Node no longer knows this connection";
const NODE_OFFLINE: &str = "that computer is offline: its Lume Node is not connected to the Relay";
const HEARTBEAT: Duration = Duration::from_secs(20);
const STATUS_FILE: &str = "relay-status.json";
// One persistent E2E connection per Node/device: all prompts and polls share its authorization,
// and an actual disconnect ends it instead of creating a new connection for each request.
struct ClientConnection { link: RelayLink, session: Session }
static CLIENT_CONNECTIONS: OnceLock<Mutex<HashMap<String, Arc<Mutex<Option<ClientConnection>>>>>> = OnceLock::new();
static CLIENT_HEARTBEATS: OnceLock<Mutex<HashMap<String, (u64, mpsc::Sender<()>)>>> = OnceLock::new();
static NEXT_CLIENT_HEARTBEAT: AtomicU64 = AtomicU64::new(1);
#[cfg(not(test))]
const CLIENT_HEARTBEAT: Duration = Duration::from_secs(8);
#[cfg(test)]
const CLIENT_HEARTBEAT: Duration = Duration::from_millis(80);

fn client_key_for(directory: &Path, fingerprint: &str) -> String {
    format!("{}\0{fingerprint}", directory.display())
}

fn connection_slot(directory: &Path, fingerprint: &str) -> Result<Arc<Mutex<Option<ClientConnection>>>, String> {
    let key = client_key_for(directory, fingerprint);
    Ok(CLIENT_CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new())).lock().map_err(|_| "Node connection is unavailable")?
        .entry(key).or_insert_with(|| Arc::new(Mutex::new(None))).clone())
}

fn close_client_connection(directory: &Path, fingerprint: &str) {
    stop_client_heartbeat(directory, fingerprint);
    if let Ok(slot) = connection_slot(directory, fingerprint) {
        if let Ok(mut guard) = slot.lock() {
            if let Some(connection) = guard.take() { connection.link.close(); }
        }
    }
}

fn stop_client_heartbeat(directory: &Path, fingerprint: &str) {
    if let Ok(mut heartbeats) = CLIENT_HEARTBEATS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        if let Some((_, stop)) = heartbeats.remove(&client_key_for(directory, fingerprint)) { let _ = stop.send(()); }
    }
}

/// Each authenticated Node has its own lightweight heartbeat; a slow or offline second Node
/// cannot make a healthy connection expire while the app is in the background.
fn start_client_heartbeat(directory: &Path, remote: &RelayRemote) -> Result<(), String> {
    let key = client_key_for(directory, &remote.identity.fingerprint);
    let generation = NEXT_CLIENT_HEARTBEAT.fetch_add(1, Ordering::Relaxed);
    let (stop, receiver) = mpsc::channel();
    {
        let mut heartbeats = CLIENT_HEARTBEATS.get_or_init(|| Mutex::new(HashMap::new())).lock().map_err(|_| "Node connection is unavailable")?;
        if let Some((_, previous)) = heartbeats.insert(key.clone(), (generation, stop)) { let _ = previous.send(()); }
    }
    let directory = directory.to_path_buf();
    let fingerprint = remote.identity.fingerprint.clone();
    let node_id = remote.node_id.clone();
    let worker_key = key.clone();
    let worker_directory = directory.clone();
    let worker_fingerprint = fingerprint.clone();
    let launched = thread::Builder::new().name("lume-relay-auth".into()).spawn(move || {
        loop {
            match receiver.recv_timeout(CLIENT_HEARTBEAT) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if node_auth::client_token(&worker_fingerprint).is_none() {
                crate::remote_sessions::connection_lost(&node_id);
                break;
            }
            if query(&worker_directory, &node_id, "keepalive").is_err() { break; }
        }
        if let Ok(mut heartbeats) = CLIENT_HEARTBEATS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
            if heartbeats.get(&worker_key).is_some_and(|(current, _)| *current == generation) { heartbeats.remove(&worker_key); }
        }
    });
    if launched.is_err() {
        node_auth::forget_client(&fingerprint);
        close_client_connection(&directory, &fingerprint);
        return Err("Could not maintain the authenticated Node connection".into());
    }
    Ok(())
}

/// Whether the running Node is connected to its Relay right now, for the app and `lume node status`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayStatus {
    pub connected: bool,
    pub since: i64,
    #[serde(default)]
    pub error: Option<String>,
}

fn write_status(directory: &Path, connected: bool, error: Option<String>) {
    let status = RelayStatus { connected, since: now_millis(), error };
    if let Ok(bytes) = serde_json::to_vec(&status) {
        let _ = write_private(&directory.join(STATUS_FILE), &bytes);
    }
}

/// The last Relay connection state the running Node reported, if any.
pub fn relay_status(directory: &Path) -> Option<RelayStatus> {
    serde_json::from_slice(&fs::read(directory.join(STATUS_FILE)).ok()?).ok()
}
const AUTHORIZATION_SYNC: Duration = Duration::from_secs(3);

fn io(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn derived(domain: &str, secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update([0]);
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
}

/// What the Relay learns of a pairing offer: a value that cannot be turned back into the offer's secret.
fn relay_slot(secret: &str) -> String {
    URL_SAFE_NO_PAD.encode(derived("lume-relay-slot-v1", secret))
}

fn box_key(secret: &str) -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, &derived("lume-relay-pairing-box-v1", secret)).expect("a 32-byte key"))
}

fn seal_box(secret: &str, kind: u8, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 12];
    SystemRandom::new().fill(&mut nonce).map_err(|_| "no randomness available".to_string())?;
    let mut buffer = plaintext.to_vec();
    box_key(secret).seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(PAIR_AAD), &mut buffer).map_err(|_| "encryption failed".to_string())?;
    let mut frame = vec![kind];
    frame.extend_from_slice(&nonce);
    frame.extend_from_slice(&buffer);
    Ok(frame)
}

fn open_box(secret: &str, kind: u8, frame: &[u8]) -> Result<Vec<u8>, String> {
    if frame.len() < 1 + 12 + 16 || frame[0] != kind {
        return Err("not a pairing frame".into());
    }
    let nonce: [u8; 12] = frame[1..13].try_into().map_err(|_| "bad nonce".to_string())?;
    let mut buffer = frame[13..].to_vec();
    box_key(secret).open_in_place(Nonce::assume_unique_for_key(nonce), Aad::from(PAIR_AAD), &mut buffer).map(|plain| plain.to_vec()).map_err(|_| "the pairing message could not be authenticated".to_string())
}

// ── Node side ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredRelayOffer {
    offer_id: String,
    secret: String,
    expires_at: i64,
}

/// Writes the offer for the running service, and returns the URI a device scans to pair through the Relay.
pub fn publish_offer(directory: &Path, relay_url: &str, offer: &NodePairingOffer) -> Result<String, String> {
    if relay_url.contains(['&', '=', ' ']) {
        return Err("the Relay address contains unsupported characters".into());
    }
    let stored = StoredRelayOffer { offer_id: offer.offer_id.clone(), secret: offer.secret.clone(), expires_at: offer.expires_at };
    let path = directory.join(OFFER_FILE);
    write_private(&path, &serde_json::to_vec(&stored).map_err(io)?)?;
    Ok(format!(
        "lume://pair-relay?v=1&relay={relay_url}&node={}&offer={}&secret={}&fingerprint={}",
        offer.node_id, offer.offer_id, offer.secret, offer.node_identity.fingerprint
    ))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io)?;
    }
    Ok(())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_millis() as i64).unwrap_or(0)
}

pub struct RelayWorker {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    directory: std::sync::Mutex<PathBuf>,
}

impl RelayWorker {
    pub fn start(directory: &Path, relay_url: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let (directory, stop) = (directory.to_path_buf(), stop.clone());
            thread::spawn(move || run_worker(&directory, &relay_url, &stop))
        };
        Self { stop, handle: Some(handle), directory: std::sync::Mutex::new(directory.to_path_buf()) }
    }
}

impl Drop for RelayWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if let Ok(directory) = self.directory.lock() {
            let _ = fs::remove_file(directory.join(STATUS_FILE));
        }
    }
}

fn log(directory: &Path, message: &str) {
    let _ = node_service::append_log_public(directory, message);
}

fn sleep_unless_stopped(stop: &AtomicBool, duration: Duration) {
    let until = Instant::now() + duration;
    while Instant::now() < until && !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(200));
    }
}

fn run_worker(directory: &Path, relay_url: &str, stop: &AtomicBool) {
    let Ok(identity) = NodeIdentity::load_or_create(directory) else { return };
    let mut delay = 1;
    while !stop.load(Ordering::SeqCst) {
        match serve(directory, relay_url, &identity, stop) {
            Ok(()) => delay = 1,
            Err(error) => {
                log(directory, &format!("Relay: {error}"));
                write_status(directory, false, Some(error.clone()));
                sleep_unless_stopped(stop, Duration::from_secs(delay));
                delay = (delay * 2).min(60);
            }
        }
    }
}

fn client_key(client: &PairedNodeClient) -> Option<[u8; 32]> {
    URL_SAFE_NO_PAD.decode(&client.identity.public_key).ok()?.try_into().ok()
}

fn serve(directory: &Path, relay_url: &str, identity: &NodeIdentity, stop: &AtomicBool) -> Result<(), String> {
    let room = identity.public().fingerprint.clone();
    let mut link = RelayLink::connect(relay_url, &room, identity, Role::Node, None)?;
    link.set_read_timeout(Some(Duration::from_secs(1)))?;
    log(directory, "Relay: connected");
    write_status(directory, true, None);
    let mut authorized: HashSet<String> = HashSet::new();
    sync_authorized(directory, &mut link, &mut authorized)?;
    let mut last_sync = Instant::now();
    let mut peers = Peers::default();
    let mut opened_offer = String::new();
    let mut last_heartbeat = Instant::now();
    let result = (|| -> Result<(), String> { loop {
        if stop.load(Ordering::SeqCst) {
            link.close();
            return Ok(());
        }
        if let Ok(bytes) = fs::read(directory.join(OFFER_FILE)) {
            if let Ok(offer) = serde_json::from_slice::<StoredRelayOffer>(&bytes) {
                let remaining = (offer.expires_at - now_millis()) / 1000;
                if offer.offer_id != opened_offer && remaining > 0 {
                    link.open_pairing(&relay_slot(&offer.secret), remaining.min(300) as u32)?;
                    opened_offer = offer.offer_id;
                }
            }
        }
        // Devices approved (or revoked) while the Node runs take effect without a restart.
        if last_sync.elapsed() >= AUTHORIZATION_SYNC {
            sync_authorized(directory, &mut link, &mut authorized)?;
            last_sync = Instant::now();
        }
        if last_heartbeat.elapsed() >= HEARTBEAT {
            link.heartbeat()?;
            last_heartbeat = Instant::now();
        }
        match link.receive()? {
            RelayEvent::Message { from, payload, replayed, .. } => {
                match handle_message(directory, identity, &mut peers, &from, &payload, replayed) {
                    Ok(outgoing) => for frame in outgoing {
                        match frame {
                            Outgoing::Message { id, payload } => link.send(Some(&from), id, &payload)?,
                            Outgoing::Authorize(key) => link.authorize(&key)?,
                        }
                    },
                    Err(error) => log(directory, &format!("Relay message from {}…: {error}", &from[..from.len().min(8)])),
                }
            }
            // One unreadable frame is dropped; the connection and every other device's session go on.
            RelayEvent::Malformed(error) => log(directory, &format!("Relay: {error}")),
            RelayEvent::Presence { fingerprint, online: false, .. } => {
                peers.forget(&fingerprint);
                node_auth::disconnect(directory, &fingerprint);
            }
            _ => {}
        }
    } })();
    // Losing the Node's Relay connection invalidates its sessions, without stopping local tasks.
    for fingerprint in peers.sessions.keys() { node_auth::disconnect(directory, fingerprint); }
    result
}

/// What the Node knows about each connected device during one Relay connection.
#[derive(Default)]
struct Peers {
    sessions: HashMap<String, Session>,
    /// The digest of the handshake that opened each session: the same handshake arriving again is a replay.
    hellos: HashMap<String, [u8; 32]>,
    /// Per device, so one busy or misbehaving device cannot slow the others. They outlive a reconnection.
    limits: HashMap<String, Limits>,
}

impl Peers {
    fn forget(&mut self, fingerprint: &str) {
        self.sessions.remove(fingerprint);
        self.hellos.remove(fingerprint);
    }

    fn limits(&mut self, fingerprint: &str) -> &mut Limits {
        if self.limits.len() > 1024 && !self.limits.contains_key(fingerprint) { self.limits.clear(); }
        self.limits.entry(fingerprint.to_string()).or_insert_with(Limits::new)
    }
}

/// A token bucket: `capacity` at once, refilled at `capacity` per `period`.
struct Bucket { tokens: f64, capacity: f64, period: Duration, updated: Instant }

impl Bucket {
    fn new(capacity: u32, period: Duration) -> Self {
        Self { tokens: capacity as f64, capacity: capacity as f64, period, updated: Instant::now() }
    }

    fn take(&mut self) -> bool {
        let now = Instant::now();
        let refill = now.duration_since(self.updated).as_secs_f64() / self.period.as_secs_f64() * self.capacity;
        self.tokens = (self.tokens + refill).min(self.capacity);
        self.updated = now;
        if self.tokens < 1.0 { return false; }
        self.tokens -= 1.0;
        true
    }
}

/// What one device may send: 30 commands per 10 s, 10 prompts, approvals or stops per minute, one handshake per 2 s.
struct Limits { commands: Bucket, acting: Bucket, handshakes: Bucket }

impl Limits {
    fn new() -> Self {
        Self { commands: Bucket::new(30, Duration::from_secs(10)), acting: Bucket::new(10, Duration::from_secs(60)), handshakes: Bucket::new(1, Duration::from_secs(2)) }
    }
}

const BUSY: &str = "busy: too many requests from this device, try again in a moment";
/// Larger than any sealed command a device needs to send (a prompt is at most a few tens of KiB).
const MAX_INCOMING: usize = relay_e2e::MAX_PLAINTEXT + 64;

/// A frame the Node sends back to the device a message came from, or a key the Relay must allow.
#[derive(Debug, PartialEq)]
enum Outgoing {
    Message { id: &'static str, payload: Vec<u8> },
    Authorize([u8; 32]),
}

/// The answer to a sealed frame the Node has no session for, so the device reconnects at once instead of waiting.
/// It is not authenticated (there is no session to seal it with), and it names the frame it answers, so a device
/// only acts on a reset of the frame it just sent; a forged one can do no more than a Relay dropping the frame.
fn reset_frame(frame: &[u8]) -> Vec<u8> {
    let mut reset = vec![RESET];
    reset.extend_from_slice(&Sha256::digest(frame)[..16]);
    reset
}

/// Makes the Relay's allow-list match the paired devices: new ones are authorized, removed ones revoked.
fn sync_authorized(directory: &Path, link: &mut RelayLink, authorized: &mut HashSet<String>) -> Result<(), String> {
    let clients = node_pairing::clients(directory)?;
    for client in &clients {
        if !authorized.contains(&client.identity.fingerprint) {
            if let Some(key) = client_key(client) {
                link.authorize(&key)?;
                authorized.insert(client.identity.fingerprint.clone());
            }
        }
    }
    let current: HashSet<&str> = clients.iter().map(|client| client.identity.fingerprint.as_str()).collect();
    let removed: Vec<String> = authorized.iter().filter(|fingerprint| !current.contains(fingerprint.as_str())).cloned().collect();
    for fingerprint in removed {
        link.revoke(&fingerprint)?;
        authorized.remove(&fingerprint);
    }
    Ok(())
}

/// Answers one message from a device with the frames to send back to it. It never writes to the Relay itself.
/// `replayed` marks a frame the Relay kept while this Node was offline: a handshake is then dropped, since the
/// device that sent it has given up on it, and answering it would end the device's current session.
fn handle_message(directory: &Path, identity: &NodeIdentity, peers: &mut Peers, from: &str, payload: &[u8], replayed: bool) -> Result<Vec<Outgoing>, String> {
    if payload.len() > MAX_INCOMING {
        return Err("a frame larger than any command was dropped".into());
    }
    match payload.first().copied() {
        Some(PAIR_REQUEST) => {
            let paired = pair_device(directory, identity, from, payload);
            let outcome = paired.as_ref().map(|_| "accepted".to_string()).unwrap_or_else(|error| node_audit::refused(error));
            audit(directory, from, paired.as_ref().ok().map(|(client, _)| client), "pair", &Value::Null, outcome);
            paired.map(|(_, outgoing)| outgoing)
        }
        Some(1) => {
            if replayed {
                return Err("a handshake kept by the Relay while the Node was offline was ignored".into());
            }
            let hello: [u8; 32] = Sha256::digest(payload).into();
            if peers.hellos.get(from) == Some(&hello) {
                return Err("a repeated handshake was ignored".into());
            }
            if !peers.limits(from).handshakes.take() {
                return Err("a handshake sooner than 2 s after the previous one was ignored".into());
            }
            let client = node_pairing::clients(directory)?.into_iter().find(|client| client.identity.fingerprint == from && client.scopes.contains(&NodeScope::Observe)).ok_or("the device is not paired")?;
            let key = client_key(&client).ok_or("the paired key is invalid")?;
            let (reply, session) = relay_e2e::respond(identity, &key, payload)?;
            // A new handshake from the device is a reconnection: the grant of the previous session ends with it.
            if peers.sessions.insert(from.to_string(), session).is_some() { node_auth::disconnect(directory, from); }
            peers.hellos.insert(from.to_string(), hello);
            Ok(vec![Outgoing::Message { id: "hs2", payload: reply }])
        }
        Some(3) => {
            let Some(client) = node_pairing::clients(directory)?.into_iter().find(|client| client.identity.fingerprint == from && client.scopes.contains(&NodeScope::Observe)) else {
                peers.forget(from);
                return Err("the device is no longer paired".into());
            };
            let Some(session) = peers.sessions.get_mut(from) else {
                // A frame kept while the Node was offline belongs to a connection the device already gave up.
                if replayed {
                    return Err("a frame kept by the Relay for an earlier session was ignored".into());
                }
                log(directory, "Relay: a device sent a frame for a session this Node does not have; asking it to reconnect");
                return Ok(vec![Outgoing::Message { id: "reset", payload: reset_frame(payload) }]);
            };
            let plaintext = zeroize::Zeroizing::new(session.open(payload)?);
            // From here the frame is authentic, so every outcome gets a sealed answer instead of a 15 s silence.
            let Ok(mut command) = serde_json::from_slice::<Value>(&plaintext) else {
                let answer = serde_json::to_vec(&json!({ "ok": false, "error": "unreadable command" })).map_err(io)?;
                return Ok(vec![Outgoing::Message { id: "reply", payload: session.seal(&answer)? }]);
            };
            let acting = matches!(command["cmd"].as_str(), Some("prompt" | "approve" | "interrupt" | "authenticate"));
            let limits = peers.limits.entry(from.to_string()).or_insert_with(Limits::new);
            let allowed = limits.commands.take() && (!acting || limits.acting.take());
            let mut answer = if allowed {
                answer_authenticated_command(directory, from, &command, &client.scopes)
            } else {
                json!({ "ok": false, "error": BUSY })
            };
            if answer["ok"] == true {
                let _ = node_pairing::touch_with_platform(directory, from, now_millis(), command["clientPlatform"].as_str());
            }
            if let Some(cmd @ ("prompt" | "approve" | "interrupt" | "authenticate")) = command["cmd"].as_str() {
                audit(directory, from, Some(&client), cmd, &command, node_audit::outcome_of(&answer));
            }
            let plaintext = zeroize::Zeroizing::new(serde_json::to_vec(&answer).map_err(io)?);
            scrub_sensitive_fields(&mut command);
            if let Some(result) = answer.get_mut("result") { scrub_sensitive_fields(result); }
            Ok(vec![Outgoing::Message { id: "reply", payload: session.seal(&plaintext)? }])
        }
        _ => Err("unexpected frame".into()),
    }
}

/// Writes what a device did to the audit file: who, which command, for which session and request, and how it ended.
/// A prompt contributes only its length.
fn audit(directory: &Path, fingerprint: &str, client: Option<&PairedNodeClient>, cmd: &str, command: &Value, outcome: String) {
    let bounded = |field: &str| command[field].as_str().map(|value| value.chars().take(512).collect::<String>());
    let record = node_audit::AuditRecord {
        at: now_millis(),
        device_id: client.map(|client| client.device_id.clone()).unwrap_or_default(),
        fingerprint: fingerprint.chars().take(64).collect(),
        device_name: client.map(|client| client.display_name.clone()).unwrap_or_default(),
        cmd: cmd.into(),
        session_id: bounded("id"),
        request_id: bounded("requestId"),
        route: "relay".into(),
        outcome,
        prompt_length: (cmd == "prompt").then(|| command["prompt"].as_str().map(|prompt| prompt.chars().count())).flatten(),
    };
    if let Err(error) = node_audit::record(directory, &record) {
        log(directory, &format!("Relay: could not write the audit record: {error}"));
    }
}

fn scrub_sensitive_fields(value: &mut Value) {
    for field in ["code", "authToken", "token"] {
        if let Some(Value::String(secret)) = value.get_mut(field) { secret.zeroize(); }
    }
}

/// The E2E handshake already authenticated `fingerprint`; TOTP adds a session gate before *any*
/// observation or action. The code and session grant are only present inside the encrypted frame.
fn answer_authenticated_command(directory: &Path, fingerprint: &str, command: &Value, scopes: &[NodeScope]) -> Value {
    let result = match command["cmd"].as_str() {
        Some("authenticate") => command["code"].as_str().ok_or_else(|| "Invalid authenticator code".to_string())
            .and_then(|code| node_auth::authenticate(directory, fingerprint, code))
            .and_then(|token| { node_auth::activate_anchor(directory, fingerprint, &token)?; Ok(token) })
            .map(|token| json!({ "token": token.as_str() })),
        Some("disconnect") => node_auth::authorize(directory, fingerprint, command["authToken"].as_str()).map(|_| {
            node_auth::disconnect(directory, fingerprint);
            json!({ "disconnected": true })
        }),
        _ => return match node_auth::authorize(directory, fingerprint, command["authToken"].as_str()) {
            Ok(()) => answer_command(directory, fingerprint, command, scopes),
            Err(error) => json!({ "ok": false, "error": error }),
        },
    };
    match result {
        Ok(value) => json!({ "ok": true, "result": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

fn pair_device(directory: &Path, identity: &NodeIdentity, from: &str, payload: &[u8]) -> Result<(PairedNodeClient, Vec<Outgoing>), String> {
    let stored: StoredRelayOffer = serde_json::from_slice(&fs::read(directory.join(OFFER_FILE)).map_err(|_| "no pairing offer is open".to_string())?).map_err(io)?;
    let request: NodePairingRequest = serde_json::from_slice(&open_box(&stored.secret, PAIR_REQUEST, payload)?).map_err(io)?;
    if request.proof.identity.fingerprint != from {
        return Err("the pairing request does not come from the connection that sent it".into());
    }
    let client = node_pairing::accept_pairing(directory, &request, &[NodeScope::Observe, NodeScope::ReadConversations, NodeScope::Prompt, NodeScope::Approve, NodeScope::Interrupt], now_millis())?;
    let _ = fs::remove_file(directory.join(OFFER_FILE));
    let key = client_key(&client).ok_or("the paired key is invalid")?;
    let answer = json!({ "client": client, "nodeIdentity": identity.public() });
    let response = seal_box(&stored.secret, PAIR_RESPONSE, &serde_json::to_vec(&answer).map_err(io)?)?;
    log(directory, "Relay: paired a new device");
    Ok((client, vec![Outgoing::Authorize(key), Outgoing::Message { id: "pair-response", payload: response }]))
}

/// What was said in one session: needs the owner's switch (off by default) and a device paired with the scope.
fn conversation_for(directory: &Path, command: &Value, scopes: &[NodeScope]) -> Result<Value, String> {
    if !scopes.contains(&NodeScope::ReadConversations) {
        return Err("this device was paired before conversations could be shared; pair it again".into());
    }
    if !node_service::load_or_create_config(directory)?.share_conversations {
        return Err("the owner of that computer has not allowed reading its conversations".into());
    }
    let id = command["id"].as_str().ok_or("a session id is required")?;
    // The running app knows what was said; its database only keeps a scrubbed copy, so that is the fallback.
    crate::node_bridge::live_session_conversation(directory, id).or_else(|error| {
        if !crate::node_bridge::is_app_closed(&error) {
            return Err(error);
        }
        crate::node_sessions::conversation(id).and_then(|conversation| serde_json::to_value(conversation).map_err(io))
    })
}

/// A prompt or an approval that is older than this when it arrives is refused: the Relay replays frames kept for an
/// offline Node, and a request that waited minutes must not act when nobody is watching.
const REQUEST_MAX_AGE_MS: i64 = 60_000;
/// How long a request id is remembered, so a repeated request is answered without acting twice, also after a restart.
const REQUEST_MEMORY_MS: i64 = 24 * 60 * 60_000;
const MAX_REMEMBERED_REQUESTS: usize = 4096;
const REQUESTS_FILE: &str = "relay-requests.json";

/// One acting request a device sent, kept in `relay-requests.json` (private to the owner).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RememberedRequest {
    device: String,
    request_id: String,
    at: i64,
    /// Digest of the command, its session and what it carries: the same id sent with another request is refused.
    digest: String,
    /// What the Node answered; `None` while it runs, or when the Node stopped before it could tell.
    #[serde(default)]
    outcome: Option<Value>,
}

static REMEMBERED: OnceLock<Mutex<HashMap<PathBuf, Vec<RememberedRequest>>>> = OnceLock::new();

#[derive(Debug, PartialEq)]
enum Seen {
    First,
    Repeated(Option<Value>),
    Conflict,
}

fn request_digest(command: &Value) -> String {
    let mut hasher = Sha256::new();
    for field in ["cmd", "id", "prompt", "permissionId", "action"] {
        let value = command[field].as_str().unwrap_or_default();
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Runs `change` on the requests remembered for `directory` (read from disk the first time), then saves them.
fn with_remembered<T>(directory: &Path, now: i64, change: impl FnOnce(&mut Vec<RememberedRequest>) -> T) -> Result<T, String> {
    let mut all = REMEMBERED.get_or_init(|| Mutex::new(HashMap::new())).lock().map_err(|_| "the request memory is unavailable")?;
    let requests = all.entry(directory.to_path_buf()).or_insert_with(|| {
        fs::read(directory.join(REQUESTS_FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    });
    requests.retain(|request| now - request.at < REQUEST_MEMORY_MS);
    // What could not be saved is not remembered either, so memory and disk never disagree about a request.
    let before = requests.clone();
    let result = change(requests);
    if requests.len() > MAX_REMEMBERED_REQUESTS {
        requests.sort_by_key(|request| request.at);
        let excess = requests.len() - MAX_REMEMBERED_REQUESTS;
        requests.drain(..excess);
    }
    // Written beside and then moved into place, so a Node stopped mid-write keeps the previous memory.
    let temporary = directory.join(format!("{REQUESTS_FILE}.tmp"));
    let saved = serde_json::to_vec(&*requests).map_err(io)
        .and_then(|bytes| write_private(&temporary, &bytes))
        .and_then(|()| fs::rename(&temporary, directory.join(REQUESTS_FILE)).map_err(io));
    if let Err(error) = saved {
        *requests = before;
        return Err(error);
    }
    Ok(result)
}

/// Records a request before it runs. A request id is per device: another device may use the same one.
fn remember_request(directory: &Path, device: &str, request_id: &str, digest: &str, now: i64) -> Result<Seen, String> {
    with_remembered(directory, now, |requests| {
        match requests.iter().find(|request| request.device == device && request.request_id == request_id) {
            Some(known) if known.digest != digest => Seen::Conflict,
            Some(known) => Seen::Repeated(known.outcome.clone()),
            None => {
                requests.push(RememberedRequest { device: device.into(), request_id: request_id.into(), at: now, digest: digest.into(), outcome: None });
                Seen::First
            }
        }
    })
}

/// Keeps what a request answered, or forgets a request whose attempt failed, so the device can retry it.
fn settle_request(directory: &Path, device: &str, request_id: &str, outcome: Option<&Value>, now: i64) -> Result<(), String> {
    with_remembered(directory, now, |requests| match outcome {
        Some(outcome) => {
            if let Some(known) = requests.iter_mut().find(|request| request.device == device && request.request_id == request_id) {
                known.outcome = Some(outcome.clone());
            }
        }
        None => requests.retain(|request| !(request.device == device && request.request_id == request_id)),
    })
}

/// The id the app sees: tied to the device, so two devices that pick the same id never share the app's answer to it.
/// The app accepts at most 128 characters; a longer combination carries a digest of the device's id instead.
fn bridge_request_id(device: &str, request_id: &str) -> String {
    let device = &device[..device.len().min(16)];
    let prefixed = format!("relay:{device}:{request_id}");
    if prefixed.len() <= 128 {
        return prefixed;
    }
    let digest: String = Sha256::digest(request_id.as_bytes()).iter().take(20).map(|byte| format!("{byte:02x}")).collect();
    format!("relay:{device}:h{digest}")
}

/// What a device needs before it may act on this computer: a pairing that granted `scope`, and the owner's switch
/// (`allowed`), which is off by default.
fn control_gate(directory: &Path, scopes: &[NodeScope], scope: NodeScope, allowed: fn(&node_service::NodeConfig) -> bool, what: &str) -> Result<(), String> {
    if !scopes.contains(&scope) {
        return Err(format!("this device was paired before {what} existed; pair it again"));
    }
    if !allowed(&node_service::load_or_create_config(directory)?) {
        return Err(format!("the owner of that computer has not allowed {what} from other devices"));
    }
    Ok(())
}

fn session_id_of(command: &Value) -> Result<&str, String> {
    command["id"].as_str().filter(|id| !id.is_empty() && id.len() <= 512).ok_or_else(|| "a session id is required".to_string())
}

/// The id and the freshness every acting request carries.
fn acting_request(command: &Value, now: i64) -> Result<&str, String> {
    let request_id = command["requestId"].as_str().filter(|id| (8..=128).contains(&id.len())).ok_or("a request id is required")?;
    let sent_at = command["sentAt"].as_i64().ok_or("the request has no time")?;
    if (now - sent_at).abs() > REQUEST_MAX_AGE_MS {
        return Err("the request is too old, or the two clocks differ by more than a minute".into());
    }
    Ok(request_id)
}

/// Runs an acting request once per device and request id, also across restarts of the Node. A repeat is answered with
/// what the first attempt answered, without acting again; the same id with a different request is refused; a failed
/// attempt can be retried. `act` receives the id to give the app.
fn once(directory: &Path, device: &str, command: &Value, request_id: &str, now: i64, act: impl FnOnce(&str) -> Result<Value, String>) -> Result<Value, String> {
    match remember_request(directory, device, request_id, &request_digest(command), now)? {
        Seen::Conflict => Err("that request id was already used for a different request".into()),
        // Without an outcome the first attempt was cut short (the Node stopped): it may have acted, so it is not run again.
        Seen::Repeated(outcome) => Ok(match outcome {
            Some(Value::Object(mut outcome)) => { outcome.insert("repeated".into(), Value::Bool(true)); Value::Object(outcome) }
            _ => json!({ "accepted": true, "repeated": true }),
        }),
        Seen::First => {
            let result = act(&bridge_request_id(device, request_id));
            if let Err(error) = settle_request(directory, device, request_id, result.as_ref().ok(), now) {
                log(directory, &format!("Relay: could not save the request memory: {error}"));
            }
            result
        }
    }
}

/// A prompt for one of this computer's sessions: needs the device's scope, the owner's switch (off by default), a fresh
/// request, and the Lume app open. A request id the Node already handled is answered without sending it again.
fn prompt_for(directory: &Path, device: &str, command: &Value, scopes: &[NodeScope], now: i64) -> Result<Value, String> {
    control_gate(directory, scopes, NodeScope::Prompt, |config| config.allow_prompts, "prompts")?;
    let session_id = session_id_of(command)?;
    let prompt = command["prompt"].as_str().unwrap_or_default();
    if prompt.trim().is_empty() || prompt.chars().count() > crate::node_bridge::MAX_PROMPT_CHARS {
        return Err("the prompt is empty or too long".into());
    }
    let request_id = acting_request(command, now)?;
    once(directory, device, command, request_id, now, |id| crate::node_bridge::send_prompt(directory, session_id, id, prompt))
}

/// Stops the turn a session is running. It goes with sending prompts: the same owner switch, and its own scope.
fn interrupt_for(directory: &Path, device: &str, command: &Value, scopes: &[NodeScope], now: i64) -> Result<Value, String> {
    control_gate(directory, scopes, NodeScope::Interrupt, |config| config.allow_prompts, "prompts")?;
    let session_id = session_id_of(command)?;
    let request_id = acting_request(command, now)?;
    once(directory, device, command, request_id, now, |id| crate::node_bridge::interrupt_session(directory, session_id, id))
}

/// What a session waits for the person to allow: the summary and the risk, so the decision is an informed one. It shows
/// more than the session list does, so it needs the same scope and switch as answering it.
fn permission_for(directory: &Path, command: &Value, scopes: &[NodeScope]) -> Result<Value, String> {
    control_gate(directory, scopes, NodeScope::Approve, |config| config.allow_approvals, "approvals")?;
    crate::node_bridge::live_permission(directory, session_id_of(command)?)
}

/// Allows once or denies what a session waits for. "Allow for the whole session" stays on the computer itself.
fn approval_for(directory: &Path, device: &str, command: &Value, scopes: &[NodeScope], now: i64) -> Result<Value, String> {
    control_gate(directory, scopes, NodeScope::Approve, |config| config.allow_approvals, "approvals")?;
    let session_id = session_id_of(command)?;
    let permission_id = command["permissionId"].as_str().filter(|id| !id.is_empty() && id.len() <= 512).ok_or("a permission id is required")?;
    let action = command["action"].as_str().filter(|action| matches!(*action, "allow_once" | "deny")).ok_or("the answer must be allow_once or deny")?;
    let request_id = acting_request(command, now)?;
    once(directory, device, command, request_id, now, |id| crate::node_bridge::answer_permission(directory, session_id, id, permission_id, action))
}

/// Read-only commands, the same observations the LAN transport offers, and the prompt the owner allows.
fn answer_command(directory: &Path, device: &str, command: &Value, scopes: &[NodeScope]) -> Value {
    let result = match command["cmd"].as_str() {
        Some("keepalive") => Ok(json!({ "connected": true })),
        Some("health") => node_service::health(directory).and_then(|health| serde_json::to_value(health).map_err(io)),
        Some("inventory") => crate::node_inventory::InventoryCache::default().snapshot(directory).and_then(|inventory| serde_json::to_value(inventory).map_err(io)),
        Some("session") => conversation_for(directory, command, scopes),
        Some("prompt") => prompt_for(directory, device, command, scopes, now_millis()),
        Some("permission") => permission_for(directory, command, scopes),
        Some("approve") => approval_for(directory, device, command, scopes, now_millis()),
        Some("interrupt") => interrupt_for(directory, device, command, scopes, now_millis()),
        Some("sessions") => crate::node_bridge::live_sessions_snapshot(directory).or_else(|error| {
            if !crate::node_bridge::is_app_closed(&error) {
                return Err(error);
            }
            crate::node_sessions::snapshot().and_then(|snapshot| serde_json::to_value(snapshot).map_err(io))
        }),
        _ => Err("unsupported command".into()),
    };
    match result {
        Ok(value) => json!({ "ok": true, "result": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

// ── Device side ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayRemote {
    pub node_id: String,
    pub relay_url: String,
    pub identity: PublicDeviceIdentity,
    pub device_id: String,
    pub paired_at: i64,
}

#[derive(Debug, PartialEq)]
struct RelayPairingUri {
    relay: String,
    node_id: String,
    offer_id: String,
    secret: String,
    fingerprint: String,
}

fn parse_uri(uri: &str) -> Result<RelayPairingUri, String> {
    let query = uri.trim().strip_prefix("lume://pair-relay?").ok_or("invalid Lume Relay pairing URI")?;
    let values: HashMap<&str, &str> = query.split('&').filter_map(|item| item.split_once('=')).collect();
    if values.get("v") != Some(&"1") {
        return Err("unsupported Lume Relay pairing version".into());
    }
    let get = |name: &str| values.get(name).copied().unwrap_or_default().to_string();
    let parsed = RelayPairingUri { relay: get("relay"), node_id: get("node"), offer_id: get("offer"), secret: get("secret"), fingerprint: get("fingerprint") };
    let token = |value: &str, max: usize| !value.is_empty() && value.len() <= max && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if !(parsed.relay.starts_with("https://") || parsed.relay.starts_with("wss://") || parsed.relay.starts_with("http://127.0.0.1") || parsed.relay.starts_with("http://localhost"))
        || !token(&parsed.node_id, 128) || !token(&parsed.offer_id, 128) || !token(&parsed.secret, 128)
        || parsed.fingerprint.len() != 64 || !parsed.fingerprint.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err("invalid Lume Relay pairing URI".into());
    }
    Ok(parsed)
}

fn remotes_path(directory: &Path) -> PathBuf {
    directory.join(REMOTES_FILE)
}

pub fn remotes(directory: &Path) -> Result<Vec<RelayRemote>, String> {
    match fs::read(remotes_path(directory)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io),
        Err(_) => Ok(Vec::new()),
    }
}

pub(crate) fn save_remote(directory: &Path, remote: RelayRemote) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(io)?;
    let mut all = remotes(directory)?;
    all.retain(|existing| existing.node_id != remote.node_id);
    all.push(remote);
    write_private(&remotes_path(directory), &serde_json::to_vec_pretty(&all).map_err(io)?)
}

fn wait_for<F: FnMut(&RelayEvent) -> Option<Vec<u8>>>(link: &mut RelayLink, seconds: u64, mut pick: F) -> Result<Vec<u8>, String> {
    link.set_read_timeout(Some(Duration::from_secs(1)))?;
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        let event = link.receive()?;
        if let Some(found) = pick(&event) {
            return Ok(found);
        }
        // A Relay that tells a joining device its Node is offline saves the wait for an answer that cannot come.
        if matches!(&event, RelayEvent::Presence { role, online: false, .. } if role == "node") {
            return Err(NODE_OFFLINE.into());
        }
    }
    Err("the Node did not answer in time (is it running and connected to the Relay?)".into())
}

fn payload_of(event: &RelayEvent, first: u8) -> Option<Vec<u8>> {
    match event {
        RelayEvent::Message { payload, .. } if payload.first() == Some(&first) => Some(payload.clone()),
        _ => None,
    }
}

/// Pairs this device with a Node through the Relay, using the URI the Node printed.
pub fn pair(directory: &Path, uri: &str) -> Result<RelayRemote, String> {
    let pairing = parse_uri(uri)?;
    let identity = NodeIdentity::load_or_create(&directory.join(CLIENT_IDENTITY_DIRECTORY))?;
    let mut link = RelayLink::connect(&pairing.relay, &pairing.fingerprint, &identity, Role::Client, Some(&relay_slot(&pairing.secret)))?;
    let offer = NodePairingOffer {
        offer_id: pairing.offer_id.clone(),
        node_id: pairing.node_id.clone(),
        node_identity: PublicDeviceIdentity { algorithm: "Ed25519".into(), public_key: String::new(), fingerprint: pairing.fingerprint.clone() },
        secret: pairing.secret.clone(),
        expires_at: 0,
        qr_payload: String::new(),
    };
    let name = sysinfo::System::host_name().unwrap_or_else(|| "Lume client".into());
    let request = node_pairing::pairing_request(&identity, &offer, &name, vec![NodeScope::Observe, NodeScope::ReadConversations, NodeScope::Prompt, NodeScope::Approve, NodeScope::Interrupt])?;
    link.send(None, "pair", &seal_box(&pairing.secret, PAIR_REQUEST, &serde_json::to_vec(&request).map_err(io)?)?)?;
    let frame = wait_for(&mut link, 20, |event| payload_of(event, PAIR_RESPONSE))?;
    let answer: Value = serde_json::from_slice(&open_box(&pairing.secret, PAIR_RESPONSE, &frame)?).map_err(io)?;
    let client: PairedNodeClient = serde_json::from_value(answer["client"].clone()).map_err(io)?;
    let node: PublicDeviceIdentity = serde_json::from_value(answer["nodeIdentity"].clone()).map_err(io)?;
    let node_key = URL_SAFE_NO_PAD.decode(&node.public_key).map_err(io)?;
    if fingerprint(&node_key) != pairing.fingerprint || node.fingerprint != pairing.fingerprint {
        return Err("the Node's identity does not match the pairing offer".into());
    }
    if client.identity != *identity.public() || !client.scopes.contains(&NodeScope::Observe) {
        return Err("the Node returned a different client identity or scope".into());
    }
    let remote = RelayRemote { node_id: pairing.node_id, relay_url: pairing.relay, identity: node, device_id: client.device_id, paired_at: client.paired_at };
    save_remote(directory, remote.clone())?;
    link.close();
    Ok(remote)
}

/// Runs one read-only command on a paired Node through the Relay.
pub fn query(directory: &Path, node_id: &str, command: &str) -> Result<Value, String> {
    query_many(directory, node_id, &[command])?.into_iter().next().ok_or("the Node did not answer")?
}

/// Runs several read-only commands over one connection and one encrypted session.
/// The outer error means the Node could not be reached; each item is that command's own result.
pub fn query_many(directory: &Path, node_id: &str, commands: &[&str]) -> Result<Vec<Result<Value, String>>, String> {
    let commands: Vec<Value> = commands.iter().map(|command| json!({ "cmd": command })).collect();
    query_commands(directory, node_id, &commands)
}

/// Sends one acting command. A transport failure (not an answer of the Node) is retried once with the same request
/// id, which the Node runs only once: if the first attempt acted and only its answer was lost, the retry gets it.
fn act(directory: &Path, node_id: &str, command: Value) -> Result<Value, String> {
    let answers = match query_commands(directory, node_id, std::slice::from_ref(&command)) {
        Ok(answers) => answers,
        Err(error) if error == NODE_OFFLINE => return Err(error),
        Err(_) => {
            let mut command = command;
            command["sentAt"] = json!(now_millis());
            query_commands(directory, node_id, &[command])?
        }
    };
    answers.into_iter().next().ok_or("the Node did not answer")?
}

/// Sends a prompt to a session of a computer paired through the Relay, when its owner allows it. Repeat the call with
/// the same `request_id` after a failure to be safe: the Node runs a request id only once.
pub fn send_prompt(directory: &Path, node_id: &str, session_id: &str, request_id: &str, prompt: &str) -> Result<Value, String> {
    let command = json!({ "cmd": "prompt", "id": session_id, "requestId": request_id, "prompt": prompt, "sentAt": now_millis() });
    act(directory, node_id, command)
}

/// What a session of a computer paired through the Relay waits for the person to allow, when its owner allows approvals.
pub fn query_permission(directory: &Path, node_id: &str, session_id: &str) -> Result<Value, String> {
    query_commands(directory, node_id, &[json!({ "cmd": "permission", "id": session_id })])?.into_iter().next().ok_or("the Node did not answer")?
}

/// Allows once or denies a session's pending permission on a computer paired through the Relay. Retrying with the same
/// `request_id` is safe.
pub fn answer_permission(directory: &Path, node_id: &str, session_id: &str, permission_id: &str, action: &str, request_id: &str) -> Result<Value, String> {
    let command = json!({ "cmd": "approve", "id": session_id, "permissionId": permission_id, "action": action, "requestId": request_id, "sentAt": now_millis() });
    act(directory, node_id, command)
}

/// Stops the turn a session of a computer paired through the Relay is running, when its owner allows prompts.
pub fn interrupt_session(directory: &Path, node_id: &str, session_id: &str, request_id: &str) -> Result<Value, String> {
    let command = json!({ "cmd": "interrupt", "id": session_id, "requestId": request_id, "sentAt": now_millis() });
    act(directory, node_id, command)
}

/// One session's conversation, if the owner of the Node allows it.
pub fn query_session(directory: &Path, node_id: &str, session_id: &str) -> Result<Value, String> {
    query_commands(directory, node_id, &[json!({ "cmd": "session", "id": session_id })])?.into_iter().next().ok_or("the Node did not answer")?
}

pub(crate) fn query_commands(directory: &Path, node_id: &str, commands: &[Value]) -> Result<Vec<Result<Value, String>>, String> {
    let remote = remotes(directory)?.into_iter().find(|remote| remote.node_id == node_id || remote.identity.fingerprint == node_id).ok_or("that Node is not paired through a Relay")?;
    let slot = connection_slot(directory, &remote.identity.fingerprint)?;
    let mut guard = slot.lock().map_err(|_| "Node connection is unavailable")?;
    let token = node_auth::client_token(&remote.identity.fingerprint);
    node_auth::touch_client_if_matches(&remote.identity.fingerprint, token.as_deref().map(String::as_str));
    if guard.is_none() {
        match connect_remote(directory, &remote) {
            Ok(connection) => *guard = Some(connection),
            Err(error) => {
                if node_auth::forget_client_if_matches(&remote.identity.fingerprint, token.as_deref().map(String::as_str)) {
                    crate::remote_sessions::connection_lost(&remote.node_id);
                }
                return Err(error);
            }
        }
    }
    let mut result = query_remote_commands(guard.as_mut().expect("connection created"), &remote, commands, token.as_deref().map(String::as_str));
    // The Node lost this session (it restarted or reconnected to the Relay) and did not act on the frame: one new
    // connection and one more try, never a loop.
    if result.as_ref().is_err_and(|error| error == NODE_RESET) {
        if let Some(connection) = guard.take() { connection.link.close(); }
        result = connect_remote(directory, &remote).and_then(|connection| {
            query_remote_commands(guard.insert(connection), &remote, commands, token.as_deref().map(String::as_str))
        });
    }
    if result.is_err() {
        if let Some(connection) = guard.take() { connection.link.close(); }
    }
    if result.is_err() || result.as_ref().is_ok_and(|answers| answers.iter().any(|answer| answer.as_ref().err().is_some_and(|error| error == node_auth::OTP_REQUIRED))) {
        if node_auth::forget_client_if_matches(&remote.identity.fingerprint, token.as_deref().map(String::as_str)) {
            crate::remote_sessions::connection_lost(&remote.node_id);
        }
    } else if result.as_ref().is_ok_and(|answers| answers.iter().any(Result::is_ok)) {
        node_auth::touch_client_if_matches(&remote.identity.fingerprint, token.as_deref().map(String::as_str));
    }
    result
}

fn connect_remote(directory: &Path, remote: &RelayRemote) -> Result<ClientConnection, String> {
    let identity = NodeIdentity::load_or_create(&directory.join(CLIENT_IDENTITY_DIRECTORY))?;
    let node_key: [u8; 32] = URL_SAFE_NO_PAD.decode(&remote.identity.public_key).map_err(io)?.try_into().map_err(|_| "invalid Node key".to_string())?;
    let mut link = RelayLink::connect(&remote.relay_url, &remote.identity.fingerprint, &identity, Role::Client, None)?;
    // A Relay that says the Node is offline saves a handshake that could only wait out its timeout.
    if link.node_online == Some(false) {
        link.close();
        return Err(NODE_OFFLINE.into());
    }
    let (state, hello) = relay_e2e::initiate(&identity)?;
    link.send(None, "hs1", &hello)?;
    let reply = wait_for(&mut link, 15, |event| payload_of(event, 2))?;
    let session = state.finish(&reply, &node_key)?;
    Ok(ClientConnection { link, session })
}

fn query_remote_commands(connection: &mut ClientConnection, remote: &RelayRemote, commands: &[Value], token: Option<&str>) -> Result<Vec<Result<Value, String>>, String> {
    let mut results = Vec::new();
    for command in commands {
        let mut command = command.clone();
        if let (Some(object), Some(platform)) = (command.as_object_mut(), node_pairing::current_platform()) {
            object.insert("clientPlatform".into(), Value::String(platform));
        }
        if let (Some(object), Some(token)) = (command.as_object_mut(), token) {
            object.insert("authToken".into(), Value::String(token.to_string()));
        }
        let plaintext = zeroize::Zeroizing::new(serde_json::to_vec(&command).map_err(io)?);
        scrub_sensitive_fields(&mut command);
        let sealed = connection.session.seal(&plaintext)?;
        connection.link.send(None, "cmd", &sealed)?;
        let reset = reset_frame(&sealed);
        let frame = wait_for(&mut connection.link, 15, |event| match event {
            RelayEvent::Message { payload, .. } if payload.first() == Some(&3) || *payload == reset => Some(payload.clone()),
            _ => None,
        })?;
        if frame == reset {
            return Err(NODE_RESET.into());
        }
        let plaintext = zeroize::Zeroizing::new(connection.session.open(&frame)?);
        let answer: Value = serde_json::from_slice(&plaintext).map_err(io)?;
        if answer["error"].as_str() != Some(node_auth::OTP_REQUIRED) {
            node_auth::touch_client_if_matches(&remote.identity.fingerprint, token);
        }
        results.push(if answer["ok"] == true {
            Ok(answer["result"].clone())
        } else {
            Err(answer["error"].as_str().unwrap_or("the Node refused the command").to_string())
        });
    }
    Ok(results)
}

/// Verifies a phone authenticator code over the pinned, encrypted Relay connection. The resulting
/// device-bound grant stays in memory and is reused only while this Lume process remains connected.
pub fn authenticate(directory: &Path, node_id: &str, code: &str) -> Result<(), String> {
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) { return Err("Invalid authenticator code".into()); }
    let remote = remotes(directory)?.into_iter().find(|remote| remote.node_id == node_id || remote.identity.fingerprint == node_id).ok_or("that Node is not paired through a Relay")?;
    let mut command = json!({ "cmd": "authenticate", "code": code });
    let answers = query_commands(directory, node_id, std::slice::from_ref(&command));
    scrub_sensitive_fields(&mut command);
    let mut result = answers?.into_iter().next().ok_or("the Node did not answer")??;
    let token = result["token"].as_str().filter(|value| value.len() == 43 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')).ok_or("the Node returned an invalid session")?;
    let token = zeroize::Zeroizing::new(token.to_string());
    scrub_sensitive_fields(&mut result);
    node_auth::remember_client(&remote.identity.fingerprint, token);
    start_client_heartbeat(directory, &remote)
}

pub fn disconnect(directory: &Path, node_id: &str) -> Result<(), String> {
    let remote = remotes(directory)?.into_iter().find(|remote| remote.node_id == node_id || remote.identity.fingerprint == node_id).ok_or("that Node is not paired through a Relay")?;
    stop_client_heartbeat(directory, &remote.identity.fingerprint);
    let result = query_commands(directory, node_id, &[json!({ "cmd": "disconnect" })]).and_then(|answers| answers.into_iter().next().ok_or("the Node did not answer".to_string())?).map(|_| ());
    node_auth::forget_client(&remote.identity.fingerprint);
    close_client_connection(directory, &remote.identity.fingerprint);
    result
}

/// Forgets a Node paired through a Relay on this device. The Node's authorization is revoked on the Node.
pub fn forget(directory: &Path, node_id: &str) -> Result<bool, String> {
    let mut all = remotes(directory)?;
    let fingerprint = all.iter().find(|remote| remote.node_id == node_id || remote.identity.fingerprint == node_id).map(|remote| remote.identity.fingerprint.clone());
    let before = all.len();
    all.retain(|remote| remote.node_id != node_id && remote.identity.fingerprint != node_id);
    if all.len() == before {
        return Ok(false);
    }
    write_private(&remotes_path(directory), &serde_json::to_vec_pretty(&all).map_err(io)?)?;
    if let Some(fingerprint) = fingerprint { node_auth::forget_client(&fingerprint); close_client_connection(directory, &fingerprint); }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_relay_slot_cannot_open_the_pairing_box() {
        let secret = "offer-secret-0123456789abcdef";
        let frame = seal_box(secret, PAIR_REQUEST, b"{\"hello\":1}").unwrap();
        assert_eq!(open_box(secret, PAIR_REQUEST, &frame).unwrap(), b"{\"hello\":1}");
        assert!(open_box(&relay_slot(secret), PAIR_REQUEST, &frame).is_err(), "what the Relay learns is not enough");
        assert!(open_box("another-secret", PAIR_REQUEST, &frame).is_err());
        assert!(open_box(secret, PAIR_RESPONSE, &frame).is_err());
        let mut tampered = frame.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(open_box(secret, PAIR_REQUEST, &tampered).is_err());
        assert!(!String::from_utf8_lossy(&frame).contains("hello"));
    }

    fn prompt_command(request_id: &str, sent_at: i64) -> Value {
        json!({ "cmd": "prompt", "id": "s1", "requestId": request_id, "prompt": "hello", "sentAt": sent_at })
    }

    #[test]
    fn a_prompt_needs_the_scope_the_switch_and_a_fresh_request() {
        let directory = std::env::temp_dir().join(format!("lume-prompt-gate-{}-{}", std::process::id(), now_millis()));
        let _ = node_service::load_or_create_config(&directory).unwrap();
        let now = now_millis();
        let with = [NodeScope::Observe, NodeScope::Prompt, NodeScope::Approve, NodeScope::Interrupt];
        // The device lacks the scope: it must pair again.
        let refused = prompt_for(&directory, "device-a", &prompt_command("request-001", now), &[NodeScope::Observe], now).unwrap_err();
        assert!(refused.contains("pair it again"), "{refused}");
        // The owner has not allowed prompts: the default.
        let refused = prompt_for(&directory, "device-a", &prompt_command("request-002", now), &with, now).unwrap_err();
        assert!(refused.contains("not allowed"), "{refused}");
        let mut config = node_service::load_or_create_config(&directory).unwrap();
        config.allow_prompts = true;
        node_service::save_config(&directory, &config).unwrap();
        // Allowed, but the checks on the request itself still apply.
        assert!(prompt_for(&directory, "device-a", &prompt_command("request-003", now - 2 * REQUEST_MAX_AGE_MS), &with, now).unwrap_err().contains("too old"));
        assert!(prompt_for(&directory, "device-a", &prompt_command("request-004", now + 2 * REQUEST_MAX_AGE_MS), &with, now).unwrap_err().contains("too old"));
        assert!(prompt_for(&directory, "device-a", &prompt_command("short", now), &with, now).unwrap_err().contains("request id"));
        assert!(prompt_for(&directory, "device-a", &json!({ "cmd": "prompt", "id": "s1", "requestId": "request-005", "prompt": " ", "sentAt": now }), &with, now).unwrap_err().contains("empty"));
        assert!(prompt_for(&directory, "device-a", &json!({ "cmd": "prompt", "requestId": "request-006", "prompt": "x", "sentAt": now }), &with, now).unwrap_err().contains("session id"));
        // Everything is right, but no Lume app is open on this computer.
        let unreachable = prompt_for(&directory, "device-a", &prompt_command("request-007", now), &with, now).unwrap_err();
        assert!(unreachable.contains("not open"), "{unreachable}");
        // The failed attempt did not use up its request id.
        assert!(prompt_for(&directory, "device-a", &prompt_command("request-007", now), &with, now).unwrap_err().contains("not open"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_approval_needs_its_own_scope_and_switch() {
        let directory = std::env::temp_dir().join(format!("lume-approve-gate-{}-{}", std::process::id(), now_millis()));
        let mut config = node_service::load_or_create_config(&directory).unwrap();
        config.allow_prompts = true;
        node_service::save_config(&directory, &config).unwrap();
        let now = now_millis();
        let approve = |request_id: &str, action: &str| json!({ "cmd": "approve", "id": "s1", "permissionId": "perm", "action": action, "requestId": request_id, "sentAt": now });
        let with = [NodeScope::Observe, NodeScope::Prompt, NodeScope::Approve, NodeScope::Interrupt];
        // Allowing prompts does not allow approvals, and an older pairing lacks the scope.
        assert!(approval_for(&directory, "device-a", &approve("approve-001", "allow_once"), &[NodeScope::Observe, NodeScope::Prompt], now).unwrap_err().contains("pair it again"));
        assert!(approval_for(&directory, "device-a", &approve("approve-002", "allow_once"), &with, now).unwrap_err().contains("not allowed"));
        assert!(permission_for(&directory, &json!({ "cmd": "permission", "id": "s1" }), &with).unwrap_err().contains("not allowed"));
        config.allow_approvals = true;
        node_service::save_config(&directory, &config).unwrap();
        assert!(approval_for(&directory, "device-a", &approve("approve-003", "allow_session"), &with, now).unwrap_err().contains("allow_once or deny"));
        assert!(approval_for(&directory, "device-a", &json!({ "cmd": "approve", "id": "s1", "action": "deny", "requestId": "approve-004", "sentAt": now }), &with, now).unwrap_err().contains("permission id"));
        assert!(approval_for(&directory, "device-a", &json!({ "cmd": "approve", "id": "s1", "permissionId": "perm", "action": "deny", "requestId": "approve-005", "sentAt": now - 2 * REQUEST_MAX_AGE_MS }), &with, now).unwrap_err().contains("too old"));
        // Everything is right, but no Lume app is open here; the id stays usable for a retry.
        assert!(approval_for(&directory, "device-a", &approve("approve-006", "deny"), &with, now).unwrap_err().contains("not open"));
        assert!(approval_for(&directory, "device-a", &approve("approve-006", "deny"), &with, now).unwrap_err().contains("not open"));
        assert!(permission_for(&directory, &json!({ "cmd": "permission", "id": "s1" }), &with).unwrap_err().contains("not open"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn stopping_an_agent_goes_with_the_prompts_switch_and_needs_its_scope() {
        let directory = std::env::temp_dir().join(format!("lume-interrupt-gate-{}-{}", std::process::id(), now_millis()));
        let _ = node_service::load_or_create_config(&directory).unwrap();
        let now = now_millis();
        let stop = |request_id: &str| json!({ "cmd": "interrupt", "id": "s1", "requestId": request_id, "sentAt": now });
        let with = [NodeScope::Observe, NodeScope::Prompt, NodeScope::Interrupt];
        assert!(interrupt_for(&directory, "device-a", &stop("interrupt-001"), &[NodeScope::Observe, NodeScope::Prompt], now).unwrap_err().contains("pair it again"));
        assert!(interrupt_for(&directory, "device-a", &stop("interrupt-002"), &with, now).unwrap_err().contains("not allowed"));
        let mut config = node_service::load_or_create_config(&directory).unwrap();
        config.allow_prompts = true;
        node_service::save_config(&directory, &config).unwrap();
        assert!(interrupt_for(&directory, "device-a", &stop("interrupt-003"), &with, now).unwrap_err().contains("not open"));
        assert!(interrupt_for(&directory, "device-a", &json!({ "cmd": "interrupt", "id": "s1", "requestId": "interrupt-004", "sentAt": now - 2 * REQUEST_MAX_AGE_MS }), &with, now).unwrap_err().contains("too old"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_request_id_runs_once_per_device_and_repeats_its_answer() {
        let directory = std::env::temp_dir().join(format!("lume-requests-once-{}", random_token_for_test()));
        fs::create_dir_all(&directory).unwrap();
        let now = now_millis();
        let runs = std::cell::Cell::new(0);
        let act = |id: &str| { runs.set(runs.get() + 1); Ok(json!({ "accepted": true, "requestId": id })) };
        let command = prompt_command("repeat-0001", now);
        let first = once(&directory, "device-a", &command, "repeat-0001", now, act).unwrap();
        assert_eq!(first, json!({ "accepted": true, "requestId": "relay:device-a:repeat-0001" }), "the app sees an id tied to the device");
        let again = once(&directory, "device-a", &command, "repeat-0001", now + 1_000, act).unwrap();
        assert_eq!(again, json!({ "accepted": true, "requestId": "relay:device-a:repeat-0001", "repeated": true }), "the first answer, not a second run");
        assert_eq!(runs.get(), 1);
        // Another device may pick the same id: it is its own request.
        assert_eq!(once(&directory, "device-b", &command, "repeat-0001", now, act).unwrap()["requestId"], "relay:device-b:repeat-0001");
        assert_eq!(runs.get(), 2);
        // The same id with another prompt is refused, not answered as a repeat.
        let other = json!({ "cmd": "prompt", "id": "s1", "requestId": "repeat-0001", "prompt": "something else", "sentAt": now });
        assert!(once(&directory, "device-a", &other, "repeat-0001", now, act).unwrap_err().contains("different request"));
        assert!(once(&directory, "device-a", &json!({ "cmd": "interrupt", "id": "s1" }), "repeat-0001", now, act).is_err());
        // A failed attempt leaves its id free for a retry.
        assert!(once(&directory, "device-a", &command, "repeat-0002", now, |_| Err("the app is closed".to_string())).is_err());
        assert!(once(&directory, "device-a", &command, "repeat-0002", now, act).unwrap().get("repeated").is_none());
        assert_eq!(runs.get(), 3);
        let _ = fs::remove_dir_all(&directory);
    }

    fn forget_cached_requests(directory: &Path) {
        REMEMBERED.get().unwrap().lock().unwrap().remove(directory);
    }

    #[test]
    fn remembered_requests_survive_a_restart_of_the_node() {
        let directory = std::env::temp_dir().join(format!("lume-requests-reload-{}", random_token_for_test()));
        fs::create_dir_all(&directory).unwrap();
        let now = now_millis();
        let command = prompt_command("reload-0001", now);
        once(&directory, "device-a", &command, "reload-0001", now, |_| Ok(json!({ "accepted": true }))).unwrap();
        // A request the Node stopped in the middle of: it may have reached the app, so it is not run again.
        assert_eq!(remember_request(&directory, "device-a", "reload-0002", &request_digest(&command), now).unwrap(), Seen::First);
        forget_cached_requests(&directory);
        let refuse = |_: &str| -> Result<Value, String> { panic!("a remembered request must not run again") };
        assert_eq!(once(&directory, "device-a", &command, "reload-0001", now, refuse).unwrap(), json!({ "accepted": true, "repeated": true }));
        assert_eq!(once(&directory, "device-a", &command, "reload-0002", now, refuse).unwrap(), json!({ "accepted": true, "repeated": true }));
        let stored: Vec<RememberedRequest> = serde_json::from_slice(&fs::read(directory.join(REQUESTS_FILE)).unwrap()).unwrap();
        assert_eq!(stored.len(), 2);
        assert!(!String::from_utf8(fs::read(directory.join(REQUESTS_FILE)).unwrap()).unwrap().contains("hello"), "the prompt itself is not kept");
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_request_memory_is_bounded_in_time_and_size() {
        let directory = std::env::temp_dir().join(format!("lume-requests-bound-{}", random_token_for_test()));
        fs::create_dir_all(&directory).unwrap();
        let now = now_millis();
        let digest = request_digest(&prompt_command("x", now));
        assert_eq!(remember_request(&directory, "device-a", "old-request", &digest, now - REQUEST_MEMORY_MS - 1).unwrap(), Seen::First);
        assert_eq!(remember_request(&directory, "device-a", "old-request", &digest, now).unwrap(), Seen::First, "a day later the id is free again");
        with_remembered(&directory, now, |requests| {
            requests.extend((0..MAX_REMEMBERED_REQUESTS + 10).map(|index| RememberedRequest {
                device: "device-b".into(), request_id: format!("bulk-{index:05}"), at: now - 10_000 + index as i64, digest: digest.clone(), outcome: None,
            }));
        }).unwrap();
        forget_cached_requests(&directory);
        let stored: Vec<RememberedRequest> = serde_json::from_slice(&fs::read(directory.join(REQUESTS_FILE)).unwrap()).unwrap();
        assert_eq!(stored.len(), MAX_REMEMBERED_REQUESTS);
        assert!(stored.iter().all(|request| request.request_id.as_str() > "bulk-00010"), "the oldest go first");
        assert!(stored.iter().any(|request| request.request_id == "old-request"), "the newest stays");
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_id_the_app_sees_names_the_device_and_fits_its_limit() {
        let device = "ab".repeat(32);
        assert_eq!(bridge_request_id(&device, "request-0001"), "relay:abababababababab:request-0001");
        let long = bridge_request_id(&device, &"r".repeat(128));
        assert!(long.len() <= 128 && long.starts_with("relay:abababababababab:h"), "{long}");
        assert_ne!(long, bridge_request_id(&device, &"s".repeat(128)));
    }

    #[test]
    fn prompts_approvals_and_stops_are_audited_without_the_prompt_text() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("audit", &listener);
        let from = fixture.device.public().fingerprint.clone();
        let node_key: [u8; 32] = URL_SAFE_NO_PAD.decode(&fixture.node.public().public_key).unwrap().try_into().unwrap();
        let mut peers = Peers::default();
        let (state, hello) = relay_e2e::initiate(&fixture.device).unwrap();
        let answered = handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &hello, false).unwrap();
        let [Outgoing::Message { payload: reply, .. }] = answered.as_slice() else { panic!() };
        let mut session = state.finish(reply, &node_key).unwrap();
        let now = now_millis();
        for command in [
            json!({ "cmd": "keepalive" }),
            json!({ "cmd": "prompt", "id": "s1", "requestId": "audit-0001", "prompt": "a secret plan", "sentAt": now }),
            json!({ "cmd": "approve", "id": "s1", "permissionId": "p1", "action": "deny", "requestId": "audit-0002", "sentAt": now }),
            json!({ "cmd": "interrupt", "id": "s1", "requestId": "audit-0003", "sentAt": now }),
        ] {
            let frame = session.seal(&serde_json::to_vec(&command).unwrap()).unwrap();
            let answered = handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &frame, false).unwrap();
            let [Outgoing::Message { payload, .. }] = answered.as_slice() else { panic!() };
            session.open(payload).unwrap();
        }
        let records = node_audit::recent(&fixture.node_dir, 10);
        assert_eq!(records.iter().map(|record| record.cmd.as_str()).collect::<Vec<_>>(), ["prompt", "approve", "interrupt"], "reads are not audited");
        let prompt = &records[0];
        assert_eq!((prompt.prompt_length, prompt.request_id.as_deref(), prompt.session_id.as_deref()), (Some(13), Some("audit-0001"), Some("s1")));
        assert_eq!((prompt.device_name.as_str(), prompt.fingerprint.as_str(), prompt.route.as_str()), ("Test laptop", from.as_str(), "relay"));
        assert!(records.iter().all(|record| record.outcome.starts_with("refused:the owner")), "{records:?}");
        assert!(!fs::read_to_string(fixture.node_dir.join("remote-audit.jsonl")).unwrap().contains("secret plan"));
        assert_eq!(records[1].prompt_length, None);
    }

    use std::net::TcpListener;
    use tungstenite::Message;

    /// A Node and a device in their own directories, the device paired (with every scope) and a stand-in Relay address.
    struct Fixture { node_dir: PathBuf, client_dir: PathBuf, node: NodeIdentity, device: NodeIdentity, remote: RelayRemote }

    fn fixture(name: &str, listener: &TcpListener) -> Fixture {
        let unique = format!("{name}-{}", random_token_for_test());
        let node_dir = std::env::temp_dir().join(format!("lume-relay-node-{unique}"));
        let client_dir = std::env::temp_dir().join(format!("lume-relay-client-{unique}"));
        let config = node_service::load_or_create_config(&node_dir).unwrap();
        let node = NodeIdentity::load_or_create(&node_dir).unwrap();
        let device = NodeIdentity::load_or_create(&client_dir.join(CLIENT_IDENTITY_DIRECTORY)).unwrap();
        let offer = node_pairing::begin_pairing(&node_dir, &config.node_id, &node).unwrap();
        let scopes = vec![NodeScope::Observe, NodeScope::ReadConversations, NodeScope::Prompt, NodeScope::Approve, NodeScope::Interrupt];
        let request = node_pairing::pairing_request(&device, &offer, "Test laptop", scopes.clone()).unwrap();
        let paired = node_pairing::accept_pairing(&node_dir, &request, &scopes, now_millis()).unwrap();
        let remote = RelayRemote { node_id: config.node_id, relay_url: format!("http://{}", listener.local_addr().unwrap()), identity: node.public().clone(), device_id: paired.device_id, paired_at: now_millis() };
        save_remote(&client_dir, remote.clone()).unwrap();
        Fixture { node_dir, client_dir, node, device, remote }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            close_client_connection(&self.client_dir, &self.remote.identity.fingerprint);
            let _ = fs::remove_dir_all(&self.node_dir);
            let _ = fs::remove_dir_all(&self.client_dir);
        }
    }

    /// Accepts one connection as the Relay would: a challenge, any hello, then `ready`.
    fn accept_client(listener: &TcpListener, ready: Value) -> tungstenite::WebSocket<std::net::TcpStream> {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        socket.send(Message::text(json!({ "t": "challenge", "nonce": "bm9uY2U" }).to_string())).unwrap();
        let _hello = socket.read().unwrap();
        socket.send(Message::text(ready.to_string())).unwrap();
        socket
    }

    /// The ciphertext of the next message a device sends, or `None` once it closes the connection.
    fn next_payload(socket: &mut tungstenite::WebSocket<std::net::TcpStream>) -> Option<Vec<u8>> {
        loop {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let frame: Value = serde_json::from_str(text.as_str()).unwrap();
                    return Some(URL_SAFE_NO_PAD.decode(frame["c"].as_str().unwrap()).unwrap());
                }
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }

    fn deliver(socket: &mut tungstenite::WebSocket<std::net::TcpStream>, from: &str, payload: &[u8]) {
        socket.send(Message::text(json!({ "t": "msg", "id": "m", "from": from, "c": URL_SAFE_NO_PAD.encode(payload) }).to_string())).unwrap();
    }

    fn device_key(device: &NodeIdentity) -> [u8; 32] {
        URL_SAFE_NO_PAD.decode(&device.public().public_key).unwrap().try_into().unwrap()
    }

    #[test]
    fn a_node_the_relay_reports_offline_fails_at_once_and_nothing_is_sent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("offline", &listener);
        let relay = std::thread::spawn(move || {
            let mut socket = accept_client(&listener, json!({ "t": "ready", "role": "client", "nodeOnline": false }));
            let mut sent = 0;
            while next_payload(&mut socket).is_some() { sent += 1; }
            sent
        });
        let started = Instant::now();
        let error = query(&fixture.client_dir, &fixture.remote.node_id, "health").unwrap_err();
        assert!(error.contains("offline"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(1), "it must not wait for a handshake answer");
        assert_eq!(relay.join().unwrap(), 0, "not even the handshake is sent to an offline Node");
    }

    #[test]
    fn a_replayed_handshake_leaves_the_live_session_intact() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("replayed-hello", &listener);
        let from = fixture.device.public().fingerprint.clone();
        let node_key: [u8; 32] = URL_SAFE_NO_PAD.decode(&fixture.node.public().public_key).unwrap().try_into().unwrap();
        let mut peers = Peers::default();
        let (state, hello) = relay_e2e::initiate(&fixture.device).unwrap();
        let answered = handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &hello, false).unwrap();
        let [Outgoing::Message { id: "hs2", payload: reply }] = answered.as_slice() else { panic!("{answered:?}") };
        let mut session = state.finish(reply, &node_key).unwrap();
        // The same handshake again, flagged by the Relay or not, and an older one the Relay kept: all dropped.
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &hello, true).is_err());
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &hello, false).is_err());
        let (_, older) = relay_e2e::initiate(&fixture.device).unwrap();
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &older, true).is_err());
        let frame = session.seal(&serde_json::to_vec(&json!({ "cmd": "keepalive" })).unwrap()).unwrap();
        let answered = handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &frame, false).unwrap();
        let [Outgoing::Message { id: "reply", payload }] = answered.as_slice() else { panic!("{answered:?}") };
        let answer: Value = serde_json::from_slice(&session.open(payload).unwrap()).unwrap();
        assert_eq!(answer["result"]["connected"], true, "the session the device holds still works");
    }

    #[test]
    fn a_sealed_frame_without_a_session_is_answered_with_a_reset_unless_it_was_replayed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("reset", &listener);
        let from = fixture.device.public().fingerprint.clone();
        let mut peers = Peers::default();
        let frame = [3u8; 40];
        let answered = handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &frame, false).unwrap();
        assert_eq!(answered, [Outgoing::Message { id: "reset", payload: reset_frame(&frame) }]);
        assert_eq!(reset_frame(&frame).len(), 17);
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &frame, true).is_err(), "a kept frame gets no answer");
    }

    #[test]
    fn a_reset_reconnects_and_retries_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("retry", &listener);
        let node = NodeIdentity::load_or_create(&fixture.node_dir).unwrap();
        let (client_key, node_fingerprint) = (device_key(&fixture.device), fixture.node.public().fingerprint.clone());
        let relay = std::thread::spawn(move || {
            // The first connection is reset; the second answers once and is reset on the next command; the third is reset.
            let mut answered = 0;
            for connection in 0..3 {
                let mut socket = accept_client(&listener, json!({ "t": "ready", "role": "client" }));
                let hello = next_payload(&mut socket).unwrap();
                let (reply, mut session) = relay_e2e::respond(&node, &client_key, &hello).unwrap();
                deliver(&mut socket, &node_fingerprint, &reply);
                let mut commands = 0;
                while let Some(frame) = next_payload(&mut socket) {
                    commands += 1;
                    if connection == 1 && commands == 1 {
                        session.open(&frame).unwrap();
                        let answer = session.seal(br#"{"ok":true,"result":{"answered":true}}"#).unwrap();
                        deliver(&mut socket, &node_fingerprint, &answer);
                        answered += 1;
                    } else {
                        // A reset of some other frame is ignored; the one naming this frame is not.
                        deliver(&mut socket, &node_fingerprint, &reset_frame(b"another frame"));
                        deliver(&mut socket, &node_fingerprint, &reset_frame(&frame));
                    }
                }
            }
            answered
        });
        let started = Instant::now();
        assert_eq!(query(&fixture.client_dir, &fixture.remote.node_id, "keepalive").unwrap()["answered"], true, "a reset is retried on a new connection");
        assert_eq!(query(&fixture.client_dir, &fixture.remote.node_id, "keepalive").unwrap_err(), NODE_RESET, "only once");
        assert!(started.elapsed() < Duration::from_secs(5), "a reset never waits for the answer timeout");
        assert_eq!(relay.join().unwrap(), 1);
    }

    #[test]
    fn a_lost_connection_retries_an_action_once_with_the_same_request_id() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("transport-retry", &listener);
        let node = NodeIdentity::load_or_create(&fixture.node_dir).unwrap();
        let (client_key, node_fingerprint) = (device_key(&fixture.device), fixture.node.public().fingerprint.clone());
        let relay = std::thread::spawn(move || {
            let mut ids = Vec::new();
            for connection in 0..2 {
                let mut socket = accept_client(&listener, json!({ "t": "ready", "role": "client" }));
                let hello = next_payload(&mut socket).unwrap();
                let (reply, mut session) = relay_e2e::respond(&node, &client_key, &hello).unwrap();
                deliver(&mut socket, &node_fingerprint, &reply);
                let frame = next_payload(&mut socket).unwrap();
                let command: Value = serde_json::from_slice(&session.open(&frame).unwrap()).unwrap();
                ids.push(command["requestId"].as_str().unwrap().to_string());
                if connection == 0 { drop(socket); continue; } // The connection drops before an answer.
                deliver(&mut socket, &node_fingerprint, &session.seal(br#"{"ok":true,"result":{"accepted":true}}"#).unwrap());
                while next_payload(&mut socket).is_some() {}
            }
            ids
        });
        assert_eq!(interrupt_session(&fixture.client_dir, &fixture.remote.node_id, "s1", "retry-0001").unwrap()["accepted"], true);
        close_client_connection(&fixture.client_dir, &fixture.remote.identity.fingerprint);
        assert_eq!(relay.join().unwrap(), ["retry-0001", "retry-0001"]);
    }

    /// Opens an E2E session with the Node of `fixture` through `handle_message`, as the device would.
    fn open_session(fixture: &Fixture, peers: &mut Peers) -> Session {
        let node_key: [u8; 32] = URL_SAFE_NO_PAD.decode(&fixture.node.public().public_key).unwrap().try_into().unwrap();
        let (state, hello) = relay_e2e::initiate(&fixture.device).unwrap();
        let answered = handle_message(&fixture.node_dir, &fixture.node, peers, &fixture.device.public().fingerprint, &hello, false).unwrap();
        let [Outgoing::Message { payload, .. }] = answered.as_slice() else { panic!("{answered:?}") };
        state.finish(payload, &node_key).unwrap()
    }

    fn send_frame(fixture: &Fixture, peers: &mut Peers, session: &mut Session, frame: &[u8]) -> Result<Value, String> {
        let answered = handle_message(&fixture.node_dir, &fixture.node, peers, &fixture.device.public().fingerprint, frame, false)?;
        let [Outgoing::Message { payload, .. }] = answered.as_slice() else { panic!("{answered:?}") };
        Ok(serde_json::from_slice(&session.open(payload).unwrap()).unwrap())
    }

    fn send_command(fixture: &Fixture, peers: &mut Peers, session: &mut Session, command: Value) -> Value {
        let frame = session.seal(&serde_json::to_vec(&command).unwrap()).unwrap();
        send_frame(fixture, peers, session, &frame).unwrap()
    }

    #[test]
    fn replayed_old_and_unreadable_frames_are_refused_and_the_session_goes_on() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("frames", &listener);
        let from = fixture.device.public().fingerprint.clone();
        let mut peers = Peers::default();
        let mut session = open_session(&fixture, &mut peers);
        let keepalive = serde_json::to_vec(&json!({ "cmd": "keepalive" })).unwrap();
        let frame = session.seal(&keepalive).unwrap();
        assert_eq!(send_frame(&fixture, &mut peers, &mut session, &frame).unwrap()["ok"], true);
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &frame, false).is_err(), "the same counter again");
        let truncated = session.seal(&keepalive).unwrap();
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &truncated[..12], false).is_err(), "truncated");
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &[], false).is_err(), "empty");
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &[0x7f, 1, 2], false).is_err(), "unknown type");
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &vec![3u8; MAX_INCOMING + 1], false).is_err(), "oversize");
        let garbage = session.seal(b"not json").unwrap();
        assert_eq!(send_frame(&fixture, &mut peers, &mut session, &garbage).unwrap()["error"], "unreadable command", "an authentic but unreadable command is answered");
        assert_eq!(send_command(&fixture, &mut peers, &mut session, json!({ "cmd": "keepalive" }))["ok"], true, "the session survives all of it");
        // A reconnection (after the handshake limit) makes the frames of the earlier session useless.
        let stale = session.seal(&keepalive).unwrap();
        std::thread::sleep(Duration::from_millis(2_100));
        let mut fresh = open_session(&fixture, &mut peers);
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &stale, false).is_err());
        assert_eq!(send_command(&fixture, &mut peers, &mut fresh, json!({ "cmd": "keepalive" }))["ok"], true);
    }

    #[test]
    fn acting_requests_outside_the_minute_are_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("expiry", &listener);
        let mut config = node_service::load_or_create_config(&fixture.node_dir).unwrap();
        (config.allow_prompts, config.allow_approvals) = (true, true);
        node_service::save_config(&fixture.node_dir, &config).unwrap();
        let mut peers = Peers::default();
        let mut session = open_session(&fixture, &mut peers);
        for (index, skew) in [-61_000, 61_000].into_iter().enumerate() {
            let at = now_millis() + skew;
            for command in [
                json!({ "cmd": "prompt", "id": "s1", "requestId": format!("expiry-p{index}"), "prompt": "hi", "sentAt": at }),
                json!({ "cmd": "approve", "id": "s1", "permissionId": "p", "action": "deny", "requestId": format!("expiry-a{index}"), "sentAt": at }),
                json!({ "cmd": "interrupt", "id": "s1", "requestId": format!("expiry-i{index}"), "sentAt": at }),
            ] {
                let answer = send_command(&fixture, &mut peers, &mut session, command);
                assert!(answer["error"].as_str().unwrap().contains("too old"), "{answer}");
            }
        }
    }

    #[test]
    fn each_command_needs_the_scope_it_was_paired_with() {
        let directory = std::env::temp_dir().join(format!("lume-scopes-{}", random_token_for_test()));
        let mut config = node_service::load_or_create_config(&directory).unwrap();
        (config.allow_prompts, config.allow_approvals, config.share_conversations) = (true, true, true);
        node_service::save_config(&directory, &config).unwrap();
        let now = now_millis();
        for command in [
            json!({ "cmd": "session", "id": "s1" }),
            json!({ "cmd": "permission", "id": "s1" }),
            json!({ "cmd": "prompt", "id": "s1", "requestId": "scope-0001", "prompt": "hi", "sentAt": now }),
            json!({ "cmd": "approve", "id": "s1", "permissionId": "p", "action": "deny", "requestId": "scope-0002", "sentAt": now }),
            json!({ "cmd": "interrupt", "id": "s1", "requestId": "scope-0003", "sentAt": now }),
        ] {
            let answer = answer_command(&directory, "device-a", &command, &[NodeScope::Observe]);
            assert!(answer["error"].as_str().unwrap().contains("pair it again"), "{command}: {answer}");
        }
        assert_eq!(answer_command(&directory, "device-a", &json!({ "cmd": "keepalive" }), &[NodeScope::Observe])["ok"], true);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_unpaired_sender_gets_nothing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("unpaired", &listener);
        let stranger_dir = std::env::temp_dir().join(format!("lume-stranger-{}", random_token_for_test()));
        let stranger = NodeIdentity::load_or_create(&stranger_dir).unwrap();
        let mut peers = Peers::default();
        let (_, hello) = relay_e2e::initiate(&stranger).unwrap();
        let from = stranger.public().fingerprint.clone();
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &hello, false).unwrap_err().contains("not paired"));
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &[3u8; 40], false).unwrap_err().contains("no longer paired"), "not even a reset");
        // A handshake signed by another key under a paired fingerprint fails too.
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &fixture.device.public().fingerprint, &hello, false).is_err());
        assert!(peers.sessions.is_empty());
        let _ = fs::remove_dir_all(&stranger_dir);
    }

    #[test]
    fn each_device_is_limited_on_commands_actions_and_handshakes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fixture = fixture("limits", &listener);
        let from = fixture.device.public().fingerprint.clone();
        let mut peers = Peers::default();
        let mut session = open_session(&fixture, &mut peers);
        let (_, again) = relay_e2e::initiate(&fixture.device).unwrap();
        assert!(handle_message(&fixture.node_dir, &fixture.node, &mut peers, &from, &again, false).unwrap_err().contains("2 s"));
        let now = now_millis();
        for index in 0..10 {
            let answer = send_command(&fixture, &mut peers, &mut session, json!({ "cmd": "interrupt", "id": "s1", "requestId": format!("limit-{index:04}"), "sentAt": now }));
            assert!(answer["error"].as_str().unwrap().contains("not allowed"), "within the limit the owner's switch answers: {answer}");
        }
        let answer = send_command(&fixture, &mut peers, &mut session, json!({ "cmd": "prompt", "id": "s1", "requestId": "limit-0010", "prompt": "hi", "sentAt": now }));
        assert_eq!(answer["error"], BUSY);
        assert_eq!(node_audit::recent(&fixture.node_dir, 1)[0].outcome, node_audit::refused(BUSY), "a refusal for the limit is audited");
        // 30 per 10 s refills 3 per second, so a slow machine may get one or two more before the limit.
        let accepted = (0..40).take_while(|_| send_command(&fixture, &mut peers, &mut session, json!({ "cmd": "keepalive" }))["ok"] == true).count();
        assert!((19..30).contains(&accepted), "the limit holds near 30 commands in 10 s: {accepted} more after 11");
        assert_eq!(peers.limits.len(), 1, "limits are per device");
    }

    #[test]
    fn pairing_uris_are_validated() {
        let fingerprint = "a".repeat(64);
        let good = format!("lume://pair-relay?v=1&relay=https://relay.lumeagents.dev&node=node-1&offer=abc_123&secret=s3cret-token&fingerprint={fingerprint}");
        let parsed = parse_uri(&good).unwrap();
        assert_eq!((parsed.relay.as_str(), parsed.node_id.as_str()), ("https://relay.lumeagents.dev", "node-1"));
        assert!(parse_uri(&good.replace("https://relay.lumeagents.dev", "http://evil.example")).is_err(), "plain http is only for loopback");
        assert!(parse_uri(&good.replace("v=1", "v=2")).is_err());
        assert!(parse_uri(&good.replace(&fingerprint, "short")).is_err());
        assert!(parse_uri("lume://pair-node?v=1").is_err());
    }

    #[test]
    #[ignore = "requires a loopback socket and a disposable unlocked OS credential store"]
    fn relay_otp_is_once_per_live_connection_and_not_per_command() {
        use ring::signature::{UnparsedPublicKey, ED25519};
        use std::{net::TcpListener, sync::atomic::{AtomicUsize, Ordering}};
        use tungstenite::Message;

        let unique = random_token_for_test();
        let node_dir = std::env::temp_dir().join(format!("lume-otp-relay-node-{unique}"));
        let client_dir = std::env::temp_dir().join(format!("lume-otp-relay-client-{unique}"));
        let health = node_service::health(&node_dir).unwrap();
        let node = NodeIdentity::load_or_create(&node_dir).unwrap();
        let device = NodeIdentity::load_or_create(&client_dir.join(CLIENT_IDENTITY_DIRECTORY)).unwrap();
        let offer = node_pairing::begin_pairing(&node_dir, &health.node_id, &node).unwrap();
        let pairing = node_pairing::pairing_request(&device, &offer, "Test laptop", vec![NodeScope::Observe]).unwrap();
        let paired = node_pairing::accept_pairing(&node_dir, &pairing, &[NodeScope::Observe], now_millis()).unwrap();
        let code = crate::node_auth::prepare_test_authenticator(&node_dir).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let remote = RelayRemote { node_id: health.node_id.clone(), relay_url: format!("http://{address}"), identity: node.public().clone(), device_id: paired.device_id, paired_at: now_millis() };
        save_remote(&client_dir, remote.clone()).unwrap();
        let handshakes = Arc::new(AtomicUsize::new(0));
        let auth_calls = Arc::new(AtomicUsize::new(0));
        let heartbeats = Arc::new(AtomicUsize::new(0));
        let (thread_handshakes, thread_auth_calls, thread_heartbeats) = (handshakes.clone(), auth_calls.clone(), heartbeats.clone());
        let server_dir = node_dir.clone();
        let server_node = NodeIdentity::load_or_create(&node_dir).unwrap();
        let client_key: [u8; 32] = URL_SAFE_NO_PAD.decode(&device.public().public_key).unwrap().try_into().unwrap();
        let client_fingerprint = device.public().fingerprint.clone();
        let node_fingerprint = node.public().fingerprint.clone();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                socket.send(Message::text(json!({ "t": "challenge", "nonce": "dGVzdC1ub25jZQ" }).to_string())).unwrap();
                let hello: Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                let public = URL_SAFE_NO_PAD.decode(hello["publicKey"].as_str().unwrap()).unwrap();
                assert_eq!(public, client_key);
                let signature = URL_SAFE_NO_PAD.decode(hello["signature"].as_str().unwrap()).unwrap();
                let statement = format!("LUME-RELAY-V1\n{}\ndGVzdC1ub25jZQ\nclient", node_fingerprint);
                UnparsedPublicKey::new(&ED25519, &public).verify(statement.as_bytes(), &signature).unwrap();
                socket.send(Message::text(json!({ "t": "ready", "role": "client" }).to_string())).unwrap();
                let hello: Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                let payload = URL_SAFE_NO_PAD.decode(hello["c"].as_str().unwrap()).unwrap();
                let (reply, mut session) = relay_e2e::respond(&server_node, &client_key, &payload).unwrap();
                thread_handshakes.fetch_add(1, Ordering::SeqCst);
                socket.send(Message::text(json!({ "t": "msg", "id": "hs2", "from": node_fingerprint, "c": URL_SAFE_NO_PAD.encode(reply) }).to_string())).unwrap();
                while let Ok(message) = socket.read() {
                    if message.is_close() { break; }
                    let Ok(frame) = serde_json::from_str::<Value>(message.to_text().unwrap_or("")) else { continue };
                    let payload = URL_SAFE_NO_PAD.decode(frame["c"].as_str().unwrap()).unwrap();
                    let command: Value = serde_json::from_slice(&session.open(&payload).unwrap()).unwrap();
                    if command["cmd"] == "test-drop" { break; } // Simulate a lost connection, without a close handshake.
                    if command["cmd"] == "authenticate" { thread_auth_calls.fetch_add(1, Ordering::SeqCst); }
                    if command["cmd"] == "keepalive" { thread_heartbeats.fetch_add(1, Ordering::SeqCst); }
                    let answer = answer_authenticated_command(&server_dir, &client_fingerprint, &command, &[NodeScope::Observe]);
                    let sealed = session.seal(&serde_json::to_vec(&answer).unwrap()).unwrap();
                    socket.send(Message::text(json!({ "t": "msg", "id": "reply", "from": node_fingerprint, "c": URL_SAFE_NO_PAD.encode(sealed) }).to_string())).unwrap();
                }
                node_auth::disconnect(&server_dir, &client_fingerprint);
            }
        });
        assert_eq!(query(&client_dir, &health.node_id, "health").unwrap_err(), node_auth::OTP_REQUIRED);
        assert_eq!(query(&client_dir, &health.node_id, "keepalive").unwrap_err(), node_auth::OTP_REQUIRED);
        authenticate(&client_dir, &health.node_id, &code).unwrap();
        assert!(query(&client_dir, &health.node_id, "health").is_ok());
        assert!(query(&client_dir, &health.node_id, "health").is_ok());
        assert_eq!(handshakes.load(Ordering::SeqCst), 1, "requests reuse one live encrypted connection");
        assert_eq!(auth_calls.load(Ordering::SeqCst), 1, "ordinary actions do not send an OTP");
        for _ in 0..50 {
            if heartbeats.load(Ordering::SeqCst) >= 2 { break; }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(heartbeats.load(Ordering::SeqCst) >= 2, "the active connection is kept alive without reusing the OTP");
        assert!(query(&client_dir, &health.node_id, "test-drop").is_err(), "a broken transport is detected");
        assert!(node_auth::client_token(&remote.identity.fingerprint).is_none(), "a lost transport discards its grant");
        assert_eq!(query(&client_dir, &health.node_id, "health").unwrap_err(), node_auth::OTP_REQUIRED);
        assert!(authenticate(&client_dir, &health.node_id, &code).is_err(), "a previously used code cannot reconnect");
        close_client_connection(&client_dir, &remote.identity.fingerprint);
        server.join().unwrap();
        crate::node_auth::disable(&node_dir).unwrap();
        let _ = fs::remove_dir_all(node_dir);
        let _ = fs::remove_dir_all(client_dir);
    }

    fn random_token_for_test() -> String {
        let mut bytes = [0u8; 12];
        getrandom::getrandom(&mut bytes).unwrap();
        URL_SAFE_NO_PAD.encode(bytes)
    }

    /// Full path through a real Relay: `LUME_RELAY_URL=http://127.0.0.1:8788 cargo test node_relay -- --ignored`.
    #[test]
    #[ignore = "needs a running Lume Relay"]
    fn a_device_pairs_and_reads_the_node_through_a_real_relay() {
        let url = std::env::var("LUME_RELAY_URL").expect("LUME_RELAY_URL");
        let unique = format!("{}-{}", std::process::id(), now_millis());
        let node_dir = std::env::temp_dir().join(format!("lume-relay-node-{unique}"));
        let client_dir = std::env::temp_dir().join(format!("lume-relay-client-{unique}"));
        let identity = NodeIdentity::load_or_create(&node_dir).unwrap();
        let config = node_service::load_or_create_config(&node_dir).unwrap();
        let offer = node_pairing::begin_pairing(&node_dir, &config.node_id, &identity).unwrap();
        let uri = publish_offer(&node_dir, &url, &offer).unwrap();
        let worker = RelayWorker::start(&node_dir, url.clone());
        // The worker connects and opens the pairing slot; the device retries until it is ready.
        let mut remote = Err(String::new());
        for _ in 0..20 {
            remote = pair(&client_dir, &uri);
            if remote.is_ok() { break; }
            thread::sleep(Duration::from_millis(500));
        }
        let remote = remote.expect("pairing through the Relay");
        assert_eq!(remote.identity.fingerprint, identity.public().fingerprint);
        assert_eq!(node_pairing::clients(&node_dir).unwrap().len(), 1);
        assert!(!node_dir.join(OFFER_FILE).exists(), "the offer is single use");
        let health = query(&client_dir, &config.node_id, "health").expect("health through the Relay");
        assert_eq!(health["nodeId"], config.node_id.as_str());
        // Conversations stay private until the owner turns the switch on.
        let refused = query_session(&client_dir, &config.node_id, "any").unwrap_err();
        assert!(refused.contains("not allowed"), "off by default: {refused}");
        let mut sharing = node_service::load_or_create_config(&node_dir).unwrap();
        sharing.share_conversations = true;
        node_service::save_config(&node_dir, &sharing).unwrap();
        let unknown = query_session(&client_dir, &config.node_id, "no-such-session").unwrap_err();
        assert!(!unknown.contains("not allowed") && !unknown.contains("pair it again"), "allowed now, the session just does not exist: {unknown}");
        // With a real session in the app's database, the conversation arrives through the encrypted session.
        let database = std::env::temp_dir().join(format!("lume-relay-app-{unique}.sqlite3"));
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE managed_sessions (session_id TEXT PRIMARY KEY, payload TEXT NOT NULL, updated_at INTEGER NOT NULL);").unwrap();
        let session = json!({
            "id": "s1", "agent": "codex", "agentLabel": "Codex", "sessionName": "Cart", "project": "/p/app", "source": "cli", "status": "running", "statusLabel": "Running",
            "startedAt": "1", "updatedAt": 5, "processId": null, "nativeSessionId": null, "workingDirectory": "/p/app",
            "permissionProfile": { "mode": "read_only", "label": "Read only", "approvalPolicy": "on-request", "canRespondFromLume": true, "availableActions": [] },
            "activities": [{ "id": "p1", "kind": "prompt", "title": "t", "detail": "Fix the total", "status": "completed", "createdAt": 1, "files": [] },
                           { "id": "m1", "kind": "message", "title": "t", "detail": "Fixed.", "status": "completed", "createdAt": 2, "files": [] }]
        });
        connection.execute("INSERT INTO managed_sessions VALUES ('s1', ?1, 5)", [session.to_string()]).unwrap();
        drop(connection);
        std::env::set_var("LUME_APP_DB", &database);
        let conversation = query_session(&client_dir, &config.node_id, "s1").expect("the conversation through the Relay");
        std::env::remove_var("LUME_APP_DB");
        let _ = fs::remove_file(&database);
        assert_eq!(conversation["messages"][0]["text"], "Fix the total");
        assert_eq!(conversation["messages"][1]["role"], "assistant");
        sharing.share_conversations = false;
        node_service::save_config(&node_dir, &sharing).unwrap();
        assert!(query_session(&client_dir, &config.node_id, "s1").is_err(), "turning the switch off takes effect at once");
        // Prompts: refused until the owner allows them, then they reach the app (a stand-in here) once, even when repeated.
        let refused = send_prompt(&client_dir, &config.node_id, "s1", "request-e2e-1", "Run the tests").unwrap_err();
        assert!(refused.contains("not allowed"), "off by default: {refused}");
        let mut allowing = node_service::load_or_create_config(&node_dir).unwrap();
        allowing.allow_prompts = true;
        node_service::save_config(&node_dir, &allowing).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        crate::node_bridge::publish_for_tests(&node_dir, listener.local_addr().unwrap().port(), "e2e-secret");
        let app = thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(&stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!((request["sessionId"].as_str(), request["prompt"].as_str()), (Some("s1"), Some("Run the tests")));
            stream.write_all(b"{\"ok\":true,\"result\":{\"accepted\":true}}\n").unwrap();
        });
        let sent = send_prompt(&client_dir, &config.node_id, "s1", "request-e2e-2", "Run the tests").expect("a prompt through the Relay");
        assert_eq!(sent["accepted"], true);
        app.join().unwrap();
        let repeated = send_prompt(&client_dir, &config.node_id, "s1", "request-e2e-2", "Run the tests").expect("a repeat is answered, not run again");
        assert_eq!(repeated["repeated"], true);
        allowing.allow_prompts = false;
        node_service::save_config(&node_dir, &allowing).unwrap();
        assert!(send_prompt(&client_dir, &config.node_id, "s1", "request-e2e-3", "Run the tests").is_err(), "turning the switch off takes effect at once");
        // Approvals: their own switch; the device reads what is asked, then allows it once.
        assert!(query_permission(&client_dir, &config.node_id, "s1").unwrap_err().contains("not allowed"), "approvals are off by default");
        let mut approving = node_service::load_or_create_config(&node_dir).unwrap();
        approving.allow_approvals = true;
        node_service::save_config(&node_dir, &approving).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        crate::node_bridge::publish_for_tests(&node_dir, listener.local_addr().unwrap().port(), "e2e-secret");
        let app = thread::spawn(move || {
            use std::io::{BufRead, Write};
            let mut seen = Vec::new();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                std::io::BufReader::new(&stream).read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(line.trim()).unwrap();
                let answer = if request["cmd"] == "permission" {
                    json!({ "ok": true, "result": { "permission": { "id": "perm-1", "kind": "command", "summary": "npm test", "risk": "low", "requestedAt": "now" } } })
                } else {
                    json!({ "ok": true, "result": { "accepted": true } })
                };
                seen.push((request["cmd"].as_str().unwrap_or_default().to_string(), request["prompt"].as_str().unwrap_or_default().to_string()));
                stream.write_all(format!("{answer}\n").as_bytes()).unwrap();
            }
            seen
        });
        let pending = query_permission(&client_dir, &config.node_id, "s1").expect("the pending permission through the Relay");
        assert_eq!(pending["permission"]["summary"], "npm test");
        let answered = answer_permission(&client_dir, &config.node_id, "s1", "perm-1", "allow_once", "approve-e2e-1").expect("an approval through the Relay");
        assert_eq!(answered["accepted"], true);
        let seen = app.join().unwrap();
        assert_eq!(seen, [("permission".to_string(), String::new()), ("approve".to_string(), "perm-1\nallow_once".to_string())]);
        assert!(answer_permission(&client_dir, &config.node_id, "s1", "perm-1", "allow_session", "approve-e2e-2").unwrap_err().contains("allow_once or deny"));
        // Stopping a running turn goes with the prompts switch.
        assert!(interrupt_session(&client_dir, &config.node_id, "s1", "interrupt-e2e-1").unwrap_err().contains("not allowed"), "prompts are off again");
        let mut stopping = node_service::load_or_create_config(&node_dir).unwrap();
        stopping.allow_prompts = true;
        node_service::save_config(&node_dir, &stopping).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        crate::node_bridge::publish_for_tests(&node_dir, listener.local_addr().unwrap().port(), "e2e-secret");
        let app = thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(&stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            stream.write_all(b"{\"ok\":true,\"result\":{\"accepted\":true}}\n").unwrap();
            (request["cmd"].as_str().unwrap_or_default().to_string(), request["sessionId"].as_str().unwrap_or_default().to_string())
        });
        assert_eq!(interrupt_session(&client_dir, &config.node_id, "s1", "interrupt-e2e-2").expect("a stop through the Relay")["accepted"], true);
        assert_eq!(app.join().unwrap(), ("interrupt".to_string(), "s1".to_string()));
        stopping.allow_prompts = false;
        node_service::save_config(&node_dir, &stopping).unwrap();
        let both = query_many(&client_dir, &config.node_id, &["health", "sessions", "launch-missiles"]).expect("several commands over one session");
        assert_eq!(both[0].as_ref().unwrap()["nodeId"], config.node_id.as_str());
        assert!(both[1].as_ref().unwrap()["sessions"].is_array());
        assert!(both[2].is_err(), "an unknown command fails alone");
        assert!(query(&client_dir, &config.node_id, "launch-missiles").is_err());
        // A device approved by code while the Node runs gets Relay access without a restart, and loses it when revoked.
        let late_dir = std::env::temp_dir().join(format!("lume-relay-late-{unique}"));
        let late = NodeIdentity::load_or_create(&late_dir.join(CLIENT_IDENTITY_DIRECTORY)).unwrap();
        let (statement, _) = node_pairing::pair_request_statement(&late, &config.node_id, "Late phone", &[NodeScope::Observe]).unwrap();
        let ticket = node_pairing::submit_request(&node_dir, &config.node_id, &statement, &[NodeScope::Observe], now_millis()).unwrap();
        let approved = node_pairing::decide(&node_dir, &ticket.request_id, true, now_millis()).unwrap().unwrap();
        save_remote(&late_dir, RelayRemote { node_id: config.node_id.clone(), relay_url: url.clone(), identity: identity.public().clone(), device_id: approved.device_id.clone(), paired_at: approved.paired_at }).unwrap();
        let mut reached = Err(String::new());
        for _ in 0..8 {
            reached = query(&late_dir, &config.node_id, "health");
            if reached.is_ok() { break; }
            thread::sleep(Duration::from_secs(1));
        }
        assert!(reached.is_ok(), "the approved device must reach the Node: {reached:?}");
        node_pairing::revoke(&node_dir, &approved.device_id).unwrap();
        let mut refused = false;
        for _ in 0..8 {
            thread::sleep(Duration::from_secs(1));
            if query(&late_dir, &config.node_id, "health").is_err() { refused = true; break; }
        }
        assert!(refused, "a revoked device must lose access");
        let _ = fs::remove_dir_all(&late_dir);
        // The offer cannot be reused by another device.
        assert!(pair(&std::env::temp_dir().join(format!("lume-relay-other-{unique}")), &uri).is_err());
        drop(worker);
        // With the Node gone, a device is told at once instead of waiting out the timeout.
        thread::sleep(Duration::from_secs(1));
        let started = Instant::now();
        let offline = query(&client_dir, &config.node_id, "health").unwrap_err();
        assert!(offline.contains("offline"), "{offline}");
        assert!(started.elapsed() < Duration::from_secs(6), "it must not wait for the answer timeout");
        for dir in [node_dir, client_dir] { let _ = fs::remove_dir_all(dir); }
    }
}
