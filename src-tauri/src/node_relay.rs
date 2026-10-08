//! Lume Node access through the Relay: the Node side (a worker thread inside `lume node run`) and the
//! device side (`lume node relay-pair`, `relay-health`, `relay-inventory`).
//!
//! The Relay only forwards ciphertext. Pairing is protected by a "pairing box" keyed from the offer's secret
//! (which travels in the QR code, never through the Relay), and everything after pairing runs inside the
//! authenticated end-to-end session of [`crate::relay_e2e`].

use std::{
    collections::HashMap,
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
}

impl RelayWorker {
    pub fn start(directory: &Path, relay_url: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let (directory, stop) = (directory.to_path_buf(), stop.clone());
            thread::spawn(move || run_worker(&directory, &relay_url, &stop))
        };
        Self { stop, handle: Some(handle) }
    }
}

impl Drop for RelayWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
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
    for client in node_pairing::clients(directory)? {
        if let Some(key) = client_key(&client) {
            link.authorize(&key)?;
        }
    }
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
            let session = sessions.get_mut(from).ok_or("no session: the device must start a handshake")?;
            let command: Value = serde_json::from_slice(&session.open(payload)?).map_err(io)?;
            let answer = answer_command(directory, &command);
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
    let client = node_pairing::accept_pairing(directory, &request, &[NodeScope::Observe], now_millis())?;
    let _ = fs::remove_file(directory.join(OFFER_FILE));
    link.authorize(&client_key(&client).ok_or("the paired key is invalid")?)?;
    let answer = json!({ "client": client, "nodeIdentity": identity.public() });
    link.send(Some(from), "pair-response", &seal_box(&stored.secret, PAIR_RESPONSE, &serde_json::to_vec(&answer).map_err(io)?)?)?;
    log(directory, "Relay: paired a new device");
    Ok(())
}

/// Read-only commands, the same observations the LAN transport offers.
fn answer_command(directory: &Path, command: &Value) -> Value {
    let result = match command["cmd"].as_str() {
        Some("health") => node_service::health(directory).and_then(|health| serde_json::to_value(health).map_err(io)),
        Some("inventory") => crate::node_inventory::InventoryCache::default().snapshot(directory).and_then(|inventory| serde_json::to_value(inventory).map_err(io)),
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

fn save_remote(directory: &Path, remote: RelayRemote) -> Result<(), String> {
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
    let request = node_pairing::pairing_request(&identity, &offer, &name, vec![NodeScope::Observe])?;
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
    let remote = remotes(directory)?.into_iter().find(|remote| remote.node_id == node_id || remote.identity.fingerprint == node_id).ok_or("that Node is not paired through a Relay")?;
    let identity = NodeIdentity::load_or_create(&directory.join(CLIENT_IDENTITY_DIRECTORY))?;
    let node_key: [u8; 32] = URL_SAFE_NO_PAD.decode(&remote.identity.public_key).map_err(io)?.try_into().map_err(|_| "invalid Node key".to_string())?;
    let mut link = RelayLink::connect(&remote.relay_url, &remote.identity.fingerprint, &identity, Role::Client, None)?;
    let (state, hello) = relay_e2e::initiate(&identity)?;
    link.send(None, "hs1", &hello)?;
    let reply = wait_for(&mut link, 15, |event| payload_of(event, 2))?;
    let mut session = state.finish(&reply, &node_key)?;
    link.send(None, "cmd", &session.seal(&serde_json::to_vec(&json!({ "cmd": command })).map_err(io)?)?)?;
    let frame = wait_for(&mut link, 15, |event| payload_of(event, 3))?;
    let answer: Value = serde_json::from_slice(&session.open(&frame)?).map_err(io)?;
    link.close();
    if answer["ok"] == true {
        Ok(answer["result"].clone())
    } else {
        Err(answer["error"].as_str().unwrap_or("the Node refused the command").to_string())
    }
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
        assert!(query(&client_dir, &config.node_id, "launch-missiles").is_err());
        // The offer cannot be reused by another device.
        assert!(pair(&std::env::temp_dir().join(format!("lume-relay-other-{unique}")), &uri).is_err());
        drop(worker);
        for dir in [node_dir, client_dir] { let _ = fs::remove_dir_all(dir); }
    }
}
