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
        atomic::{AtomicBool, Ordering},
        Arc,
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

use crate::{
    distributed_protocol::NodeScope,
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
const HEARTBEAT: Duration = Duration::from_secs(20);
const STATUS_FILE: &str = "relay-status.json";

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
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut opened_offer = String::new();
    let mut last_heartbeat = Instant::now();
    loop {
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
            RelayEvent::Message { from, payload, .. } => {
                if let Err(error) = handle_message(directory, identity, &mut link, &mut sessions, &from, &payload) {
                    log(directory, &format!("Relay message from {}…: {error}", &from[..from.len().min(8)]));
                }
            }
            RelayEvent::Presence { fingerprint, online: false, .. } => {
                sessions.remove(&fingerprint);
            }
            _ => {}
        }
    }
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

fn handle_message(directory: &Path, identity: &NodeIdentity, link: &mut RelayLink, sessions: &mut HashMap<String, Session>, from: &str, payload: &[u8]) -> Result<(), String> {
    match payload.first().copied() {
        Some(PAIR_REQUEST) => pair_device(directory, identity, link, from, payload),
        Some(1) => {
            let client = node_pairing::clients(directory)?.into_iter().find(|client| client.identity.fingerprint == from && client.scopes.contains(&NodeScope::Observe)).ok_or("the device is not paired")?;
            let key = client_key(&client).ok_or("the paired key is invalid")?;
            let (reply, session) = relay_e2e::respond(identity, &key, payload)?;
            sessions.insert(from.to_string(), session);
            link.send(Some(from), "hs2", &reply)
        }
        Some(3) => {
            let Some(client) = node_pairing::clients(directory)?.into_iter().find(|client| client.identity.fingerprint == from && client.scopes.contains(&NodeScope::Observe)) else {
                sessions.remove(from);
                return Err("the device is no longer paired".into());
            };
            let session = sessions.get_mut(from).ok_or("no session: the device must start a handshake")?;
            let command: Value = serde_json::from_slice(&session.open(payload)?).map_err(io)?;
            let answer = answer_command(directory, &command, &client.scopes);
            let frame = session.seal(&serde_json::to_vec(&answer).map_err(io)?)?;
            link.send(Some(from), "reply", &frame)
        }
        _ => Err("unexpected frame".into()),
    }
}

fn pair_device(directory: &Path, identity: &NodeIdentity, link: &mut RelayLink, from: &str, payload: &[u8]) -> Result<(), String> {
    let stored: StoredRelayOffer = serde_json::from_slice(&fs::read(directory.join(OFFER_FILE)).map_err(|_| "no pairing offer is open".to_string())?).map_err(io)?;
    let request: NodePairingRequest = serde_json::from_slice(&open_box(&stored.secret, PAIR_REQUEST, payload)?).map_err(io)?;
    if request.proof.identity.fingerprint != from {
        return Err("the pairing request does not come from the connection that sent it".into());
    }
    let client = node_pairing::accept_pairing(directory, &request, &[NodeScope::Observe, NodeScope::ReadConversations, NodeScope::Prompt, NodeScope::Approve, NodeScope::Interrupt], now_millis())?;
    let _ = fs::remove_file(directory.join(OFFER_FILE));
    link.authorize(&client_key(&client).ok_or("the paired key is invalid")?)?;
    let answer = json!({ "client": client, "nodeIdentity": identity.public() });
    link.send(Some(from), "pair-response", &seal_box(&stored.secret, PAIR_RESPONSE, &serde_json::to_vec(&answer).map_err(io)?)?)?;
    log(directory, "Relay: paired a new device");
    Ok(())
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
/// How long a request id is remembered, so a repeated request is answered without acting twice.
const REQUEST_MEMORY_MS: i64 = 10 * 60_000;

static SEEN_REQUESTS: std::sync::Mutex<Vec<(String, i64)>> = std::sync::Mutex::new(Vec::new());

/// Remembers a request id; false when it was already seen.
fn first_time(request_id: &str, now: i64) -> bool {
    let Ok(mut seen) = SEEN_REQUESTS.lock() else { return false };
    seen.retain(|(_, at)| now - at < REQUEST_MEMORY_MS);
    if seen.iter().any(|(id, _)| id == request_id) {
        return false;
    }
    seen.push((request_id.to_string(), now));
    true
}

/// Forgets a request id whose attempt failed, so the device can retry it.
fn forget_request(request_id: &str) {
    if let Ok(mut seen) = SEEN_REQUESTS.lock() {
        seen.retain(|(id, _)| id != request_id);
    }
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

/// Runs an acting request once per request id; a repeat is answered without acting again, a failure can be retried.
fn once(request_id: &str, now: i64, act: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
    if !first_time(request_id, now) {
        return Ok(json!({ "accepted": true, "repeated": true }));
    }
    act().inspect_err(|_| forget_request(request_id))
}

/// A prompt for one of this computer's sessions: needs the device's scope, the owner's switch (off by default), a fresh
/// request, and the Lume app open. A request id the Node already handled is answered without sending it again.
fn prompt_for(directory: &Path, command: &Value, scopes: &[NodeScope], now: i64) -> Result<Value, String> {
    control_gate(directory, scopes, NodeScope::Prompt, |config| config.allow_prompts, "prompts")?;
    let session_id = session_id_of(command)?;
    let prompt = command["prompt"].as_str().unwrap_or_default();
    if prompt.trim().is_empty() || prompt.chars().count() > crate::node_bridge::MAX_PROMPT_CHARS {
        return Err("the prompt is empty or too long".into());
    }
    let request_id = acting_request(command, now)?;
    once(request_id, now, || crate::node_bridge::send_prompt(directory, session_id, request_id, prompt))
}

/// Stops the turn a session is running. It goes with sending prompts: the same owner switch, and its own scope.
fn interrupt_for(directory: &Path, command: &Value, scopes: &[NodeScope], now: i64) -> Result<Value, String> {
    control_gate(directory, scopes, NodeScope::Interrupt, |config| config.allow_prompts, "prompts")?;
    let session_id = session_id_of(command)?;
    let request_id = acting_request(command, now)?;
    once(request_id, now, || crate::node_bridge::interrupt_session(directory, session_id, request_id))
}

/// What a session waits for the person to allow: the summary and the risk, so the decision is an informed one. It shows
/// more than the session list does, so it needs the same scope and switch as answering it.
fn permission_for(directory: &Path, command: &Value, scopes: &[NodeScope]) -> Result<Value, String> {
    control_gate(directory, scopes, NodeScope::Approve, |config| config.allow_approvals, "approvals")?;
    crate::node_bridge::live_permission(directory, session_id_of(command)?)
}

/// Allows once or denies what a session waits for. "Allow for the whole session" stays on the computer itself.
fn approval_for(directory: &Path, command: &Value, scopes: &[NodeScope], now: i64) -> Result<Value, String> {
    control_gate(directory, scopes, NodeScope::Approve, |config| config.allow_approvals, "approvals")?;
    let session_id = session_id_of(command)?;
    let permission_id = command["permissionId"].as_str().filter(|id| !id.is_empty() && id.len() <= 512).ok_or("a permission id is required")?;
    let action = command["action"].as_str().filter(|action| matches!(*action, "allow_once" | "deny")).ok_or("the answer must be allow_once or deny")?;
    let request_id = acting_request(command, now)?;
    once(request_id, now, || crate::node_bridge::answer_permission(directory, session_id, request_id, permission_id, action))
}

/// Read-only commands, the same observations the LAN transport offers, and the prompt the owner allows.
fn answer_command(directory: &Path, command: &Value, scopes: &[NodeScope]) -> Value {
    let result = match command["cmd"].as_str() {
        Some("health") => node_service::health(directory).and_then(|health| serde_json::to_value(health).map_err(io)),
        Some("inventory") => crate::node_inventory::InventoryCache::default().snapshot(directory).and_then(|inventory| serde_json::to_value(inventory).map_err(io)),
        Some("session") => conversation_for(directory, command, scopes),
        Some("prompt") => prompt_for(directory, command, scopes, now_millis()),
        Some("permission") => permission_for(directory, command, scopes),
        Some("approve") => approval_for(directory, command, scopes, now_millis()),
        Some("interrupt") => interrupt_for(directory, command, scopes, now_millis()),
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
            return Err("that computer is offline: its Lume Node is not connected to the Relay".into());
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

/// Sends a prompt to a session of a computer paired through the Relay, when its owner allows it. Repeat the call with
/// the same `request_id` after a failure to be safe: the Node runs a request id only once.
pub fn send_prompt(directory: &Path, node_id: &str, session_id: &str, request_id: &str, prompt: &str) -> Result<Value, String> {
    let command = json!({ "cmd": "prompt", "id": session_id, "requestId": request_id, "prompt": prompt, "sentAt": now_millis() });
    query_commands(directory, node_id, &[command])?.into_iter().next().ok_or("the Node did not answer")?
}

/// What a session of a computer paired through the Relay waits for the person to allow, when its owner allows approvals.
pub fn query_permission(directory: &Path, node_id: &str, session_id: &str) -> Result<Value, String> {
    query_commands(directory, node_id, &[json!({ "cmd": "permission", "id": session_id })])?.into_iter().next().ok_or("the Node did not answer")?
}

/// Allows once or denies a session's pending permission on a computer paired through the Relay. Retrying with the same
/// `request_id` is safe.
pub fn answer_permission(directory: &Path, node_id: &str, session_id: &str, permission_id: &str, action: &str, request_id: &str) -> Result<Value, String> {
    let command = json!({ "cmd": "approve", "id": session_id, "permissionId": permission_id, "action": action, "requestId": request_id, "sentAt": now_millis() });
    query_commands(directory, node_id, &[command])?.into_iter().next().ok_or("the Node did not answer")?
}

/// Stops the turn a session of a computer paired through the Relay is running, when its owner allows prompts.
pub fn interrupt_session(directory: &Path, node_id: &str, session_id: &str, request_id: &str) -> Result<Value, String> {
    let command = json!({ "cmd": "interrupt", "id": session_id, "requestId": request_id, "sentAt": now_millis() });
    query_commands(directory, node_id, &[command])?.into_iter().next().ok_or("the Node did not answer")?
}

/// One session's conversation, if the owner of the Node allows it.
pub fn query_session(directory: &Path, node_id: &str, session_id: &str) -> Result<Value, String> {
    query_commands(directory, node_id, &[json!({ "cmd": "session", "id": session_id })])?.into_iter().next().ok_or("the Node did not answer")?
}

pub(crate) fn query_commands(directory: &Path, node_id: &str, commands: &[Value]) -> Result<Vec<Result<Value, String>>, String> {
    let remote = remotes(directory)?.into_iter().find(|remote| remote.node_id == node_id || remote.identity.fingerprint == node_id).ok_or("that Node is not paired through a Relay")?;
    let identity = NodeIdentity::load_or_create(&directory.join(CLIENT_IDENTITY_DIRECTORY))?;
    let node_key: [u8; 32] = URL_SAFE_NO_PAD.decode(&remote.identity.public_key).map_err(io)?.try_into().map_err(|_| "invalid Node key".to_string())?;
    let mut link = RelayLink::connect(&remote.relay_url, &remote.identity.fingerprint, &identity, Role::Client, None)?;
    let (state, hello) = relay_e2e::initiate(&identity)?;
    link.send(None, "hs1", &hello)?;
    let reply = wait_for(&mut link, 15, |event| payload_of(event, 2))?;
    let mut session = state.finish(&reply, &node_key)?;
    let mut results = Vec::new();
    for command in commands {
        link.send(None, "cmd", &session.seal(&serde_json::to_vec(command).map_err(io)?)?)?;
        let frame = wait_for(&mut link, 15, |event| payload_of(event, 3))?;
        let answer: Value = serde_json::from_slice(&session.open(&frame)?).map_err(io)?;
        results.push(if answer["ok"] == true {
            Ok(answer["result"].clone())
        } else {
            Err(answer["error"].as_str().unwrap_or("the Node refused the command").to_string())
        });
    }
    link.close();
    Ok(results)
}

/// Forgets a Node paired through a Relay on this device. The Node's authorization is revoked on the Node.
pub fn forget(directory: &Path, node_id: &str) -> Result<bool, String> {
    let mut all = remotes(directory)?;
    let before = all.len();
    all.retain(|remote| remote.node_id != node_id && remote.identity.fingerprint != node_id);
    if all.len() == before {
        return Ok(false);
    }
    write_private(&remotes_path(directory), &serde_json::to_vec_pretty(&all).map_err(io)?)?;
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
        let refused = prompt_for(&directory, &prompt_command("request-001", now), &[NodeScope::Observe], now).unwrap_err();
        assert!(refused.contains("pair it again"), "{refused}");
        // The owner has not allowed prompts: the default.
        let refused = prompt_for(&directory, &prompt_command("request-002", now), &with, now).unwrap_err();
        assert!(refused.contains("not allowed"), "{refused}");
        let mut config = node_service::load_or_create_config(&directory).unwrap();
        config.allow_prompts = true;
        node_service::save_config(&directory, &config).unwrap();
        // Allowed, but the checks on the request itself still apply.
        assert!(prompt_for(&directory, &prompt_command("request-003", now - 2 * REQUEST_MAX_AGE_MS), &with, now).unwrap_err().contains("too old"));
        assert!(prompt_for(&directory, &prompt_command("request-004", now + 2 * REQUEST_MAX_AGE_MS), &with, now).unwrap_err().contains("too old"));
        assert!(prompt_for(&directory, &prompt_command("short", now), &with, now).unwrap_err().contains("request id"));
        assert!(prompt_for(&directory, &json!({ "cmd": "prompt", "id": "s1", "requestId": "request-005", "prompt": " ", "sentAt": now }), &with, now).unwrap_err().contains("empty"));
        assert!(prompt_for(&directory, &json!({ "cmd": "prompt", "requestId": "request-006", "prompt": "x", "sentAt": now }), &with, now).unwrap_err().contains("session id"));
        // Everything is right, but no Lume app is open on this computer.
        let unreachable = prompt_for(&directory, &prompt_command("request-007", now), &with, now).unwrap_err();
        assert!(unreachable.contains("not open"), "{unreachable}");
        // The failed attempt did not use up its request id.
        assert!(prompt_for(&directory, &prompt_command("request-007", now), &with, now).unwrap_err().contains("not open"));
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
        assert!(approval_for(&directory, &approve("approve-001", "allow_once"), &[NodeScope::Observe, NodeScope::Prompt], now).unwrap_err().contains("pair it again"));
        assert!(approval_for(&directory, &approve("approve-002", "allow_once"), &with, now).unwrap_err().contains("not allowed"));
        assert!(permission_for(&directory, &json!({ "cmd": "permission", "id": "s1" }), &with).unwrap_err().contains("not allowed"));
        config.allow_approvals = true;
        node_service::save_config(&directory, &config).unwrap();
        assert!(approval_for(&directory, &approve("approve-003", "allow_session"), &with, now).unwrap_err().contains("allow_once or deny"));
        assert!(approval_for(&directory, &json!({ "cmd": "approve", "id": "s1", "action": "deny", "requestId": "approve-004", "sentAt": now }), &with, now).unwrap_err().contains("permission id"));
        assert!(approval_for(&directory, &json!({ "cmd": "approve", "id": "s1", "permissionId": "perm", "action": "deny", "requestId": "approve-005", "sentAt": now - 2 * REQUEST_MAX_AGE_MS }), &with, now).unwrap_err().contains("too old"));
        // Everything is right, but no Lume app is open here; the id stays usable for a retry.
        assert!(approval_for(&directory, &approve("approve-006", "deny"), &with, now).unwrap_err().contains("not open"));
        assert!(approval_for(&directory, &approve("approve-006", "deny"), &with, now).unwrap_err().contains("not open"));
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
        assert!(interrupt_for(&directory, &stop("interrupt-001"), &[NodeScope::Observe, NodeScope::Prompt], now).unwrap_err().contains("pair it again"));
        assert!(interrupt_for(&directory, &stop("interrupt-002"), &with, now).unwrap_err().contains("not allowed"));
        let mut config = node_service::load_or_create_config(&directory).unwrap();
        config.allow_prompts = true;
        node_service::save_config(&directory, &config).unwrap();
        assert!(interrupt_for(&directory, &stop("interrupt-003"), &with, now).unwrap_err().contains("not open"));
        assert!(interrupt_for(&directory, &json!({ "cmd": "interrupt", "id": "s1", "requestId": "interrupt-004", "sentAt": now - 2 * REQUEST_MAX_AGE_MS }), &with, now).unwrap_err().contains("too old"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_request_id_runs_once() {
        let now = now_millis();
        assert!(first_time("repeat-0001", now));
        assert!(!first_time("repeat-0001", now + 1_000));
        assert!(first_time("repeat-0002", now + 1_000));
        // After the memory window the id may be used again.
        assert!(first_time("repeat-0001", now + REQUEST_MEMORY_MS + 1));
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
