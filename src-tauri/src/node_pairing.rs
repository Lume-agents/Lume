use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process, thread,
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{
    distributed_protocol::NodeScope,
    node_identity::{verify, NodeIdentity, PublicDeviceIdentity, SignedDeviceStatement},
    node_service::{process_instance_is_live, process_start_time},
    state::now_millis,
};

const PAIRING_STATE_FILE: &str = "pairing.json";
const PAIRING_LOCK_FILE: &str = "pairing.lock";
const PAIRING_TTL_MS: i64 = 5 * 60 * 1_000;
const MAX_PAIRING_ATTEMPTS: u8 = 5;
const LOCK_RETRY_COUNT: usize = 25;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(20);
const SECRET_DOMAIN: &[u8] = b"lume-node-pairing-secret-v1\0";
const REQUEST_KIND: &str = "pair-request-v1";
const STATUS_KIND: &str = "pair-status-v1";
const CODE_DOMAIN: &[u8] = b"lume-pair-code-v1\0";
const REQUEST_TTL_MS: i64 = 2 * 60 * 1_000;
const MAX_PENDING_REQUESTS: usize = 3;
const STATEMENT_CLOCK_WINDOW_MS: i64 = 60_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedNodeClient {
    pub device_id: String,
    pub display_name: String,
    pub identity: PublicDeviceIdentity,
    pub scopes: Vec<NodeScope>,
    pub paired_at: i64,
    pub last_seen_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePairingOffer {
    pub offer_id: String,
    pub node_id: String,
    pub node_identity: PublicDeviceIdentity,
    pub secret: String,
    pub expires_at: i64,
    pub qr_payload: String,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePairingRequest {
    pub offer_id: String,
    pub secret: String,
    pub proof: SignedDeviceStatement,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingProofPayload {
    offer_id: String,
    node_id: String,
    client_display_name: String,
    requested_scopes: Vec<NodeScope>,
    client_nonce: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct PairingState {
    active_offer: Option<StoredPairingOffer>,
    clients: Vec<PairedNodeClient>,
    requests: Vec<StoredPairRequest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredPairingOffer {
    offer_id: String,
    node_id: String,
    secret_hash: String,
    expires_at: i64,
    failed_attempts: u8,
}

pub fn begin_pairing(
    directory: &Path,
    node_id: &str,
    identity: &NodeIdentity,
) -> Result<NodePairingOffer, String> {
    validate_identifier(node_id, 128, "Node ID")?;
    let _lock = PairingLock::acquire(directory)?;
    let mut state = load_state(directory)?;
    let offer_id = random_token(16)?;
    let secret = random_token(32)?;
    let expires_at = now_millis().saturating_add(PAIRING_TTL_MS);
    state.active_offer = Some(StoredPairingOffer {
        offer_id: offer_id.clone(),
        node_id: node_id.into(),
        secret_hash: secret_hash(&secret),
        expires_at,
        failed_attempts: 0,
    });
    save_state(directory, &state)?;
    let fingerprint = &identity.public().fingerprint;
    let qr_payload = format!(
        "lume://pair-node?v=1&node={node_id}&offer={offer_id}&secret={secret}&fingerprint={fingerprint}"
    );
    Ok(NodePairingOffer {
        offer_id,
        node_id: node_id.into(),
        node_identity: identity.public().clone(),
        secret,
        expires_at,
        qr_payload,
    })
}

pub fn accept_pairing(
    directory: &Path,
    request: &NodePairingRequest,
    allowed_scopes: &[NodeScope],
    now: i64,
) -> Result<PairedNodeClient, String> {
    let _lock = PairingLock::acquire(directory)?;
    let mut state = load_state(directory)?;
    let Some(mut offer) = state.active_offer.clone() else {
        return Err("no active Node pairing offer".into());
    };
    if now > offer.expires_at {
        state.active_offer = None;
        save_state(directory, &state)?;
        return Err("the Node pairing offer expired".into());
    }
    let supplied_hash = secret_hash(&request.secret);
    let secret_matches = bool::from(supplied_hash.as_bytes().ct_eq(offer.secret_hash.as_bytes()));
    if request.offer_id != offer.offer_id || !secret_matches {
        register_failed_attempt(directory, &mut state, &mut offer)?;
        return Err("invalid Node pairing offer".into());
    }
    if let Err(error) = verify(&request.proof) {
        register_failed_attempt(directory, &mut state, &mut offer)?;
        return Err(format!("invalid client proof: {error}"));
    }
    let proof = serde_json::from_value::<PairingProofPayload>(request.proof.payload.clone())
        .map_err(|_| "invalid client pairing payload".to_string())?;
    validate_pairing_proof(&proof, &offer, allowed_scopes)?;

    let client = PairedNodeClient {
        device_id: format!("device-{}", request.proof.identity.fingerprint),
        display_name: proof.client_display_name,
        identity: request.proof.identity.clone(),
        scopes: normalized_scopes(proof.requested_scopes),
        paired_at: now,
        last_seen_at: None,
    };
    state
        .clients
        .retain(|existing| existing.identity.fingerprint != client.identity.fingerprint);
    state.clients.push(client.clone());
    state
        .clients
        .sort_by(|left, right| left.device_id.cmp(&right.device_id));
    state.active_offer = None;
    save_state(directory, &state)?;
    Ok(client)
}

pub fn pairing_request(
    identity: &NodeIdentity,
    offer: &NodePairingOffer,
    client_display_name: &str,
    requested_scopes: Vec<NodeScope>,
) -> Result<NodePairingRequest, String> {
    let proof = PairingProofPayload {
        offer_id: offer.offer_id.clone(),
        node_id: offer.node_id.clone(),
        client_display_name: client_display_name.into(),
        requested_scopes,
        client_nonce: random_token(18)?,
    };
    Ok(NodePairingRequest {
        offer_id: offer.offer_id.clone(),
        secret: offer.secret.clone(),
        proof: identity.sign(serde_json::to_value(proof).map_err(|error| error.to_string())?)?,
    })
}

pub fn clients(directory: &Path) -> Result<Vec<PairedNodeClient>, String> {
    let _lock = PairingLock::acquire(directory)?;
    Ok(load_state(directory)?.clients)
}

pub fn revoke(directory: &Path, device_id: &str) -> Result<bool, String> {
    validate_identifier(device_id, 256, "device ID")?;
    let _lock = PairingLock::acquire(directory)?;
    let mut state = load_state(directory)?;
    let previous = state.clients.len();
    state.clients.retain(|client| client.device_id != device_id);
    let removed = state.clients.len() != previous;
    if removed {
        save_state(directory, &state)?;
    }
    Ok(removed)
}

// ── Pairing by confirmation code ─────────────────────────────────────────────
//
// A device on the same network asks to pair; nobody holds a secret. Both screens show a short code that
// each side derives from what it saw (both identities, a nonce and the request id). Whoever sits at the
// Node approves only when the codes match, so a machine in the middle, which makes the two sides see
// different identities, produces different codes and gets refused.

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredPairRequest {
    request_id: String,
    identity: PublicDeviceIdentity,
    display_name: String,
    nonce: String,
    scopes: Vec<NodeScope>,
    created_at: i64,
    expires_at: i64,
    status: String,
    client: Option<PairedNodeClient>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequestTicket {
    pub request_id: String,
    pub expires_at: i64,
}

/// A pairing request waiting for the person at the Node to confirm.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPairRequest {
    pub request_id: String,
    pub display_name: String,
    pub client_fingerprint: String,
    pub code: String,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequestOutcome {
    /// `pending`, `approved`, `denied` or `expired`.
    pub status: String,
    pub client: Option<PairedNodeClient>,
}

/// The six digits both sides show. Each side feeds in the identities it believes it is talking to.
pub fn pair_code(node_fingerprint: &str, client_fingerprint: &str, nonce: &str, request_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(CODE_DOMAIN);
    for part in [node_fingerprint, client_fingerprint, nonce, request_id] {
        digest.update((part.len() as u32).to_be_bytes());
        digest.update(part.as_bytes());
    }
    let hash = digest.finalize();
    let number = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]) % 1_000_000;
    format!("{number:06}")
}

/// What the device signs to ask for pairing (the nonce is also part of the code).
pub fn pair_request_statement(identity: &NodeIdentity, node_id: &str, display_name: &str, scopes: &[NodeScope]) -> Result<(SignedDeviceStatement, String), String> {
    let nonce = random_token(18)?;
    let payload = serde_json::json!({ "kind": REQUEST_KIND, "nodeId": node_id, "displayName": display_name, "nonce": nonce, "scopes": scopes, "at": now_millis() });
    Ok((identity.sign(payload)?, nonce))
}

pub fn pair_status_statement(identity: &NodeIdentity, request_id: &str) -> Result<SignedDeviceStatement, String> {
    identity.sign(serde_json::json!({ "kind": STATUS_KIND, "requestId": request_id, "at": now_millis() }))
}

fn checked_statement(statement: &SignedDeviceStatement, kind: &str, now: i64) -> Result<serde_json::Value, String> {
    verify(statement).map_err(|error| format!("invalid signature: {error}"))?;
    let payload = &statement.payload;
    if payload["kind"] != kind {
        return Err("the statement is for something else".into());
    }
    let at = payload["at"].as_i64().ok_or("the statement has no time")?;
    if (now - at).abs() > STATEMENT_CLOCK_WINDOW_MS {
        return Err("the statement is too old or the clocks differ".into());
    }
    Ok(payload.clone())
}

/// Registers a request. It grants nothing until `decide` approves it.
pub fn submit_request(directory: &Path, node_id: &str, statement: &SignedDeviceStatement, allowed_scopes: &[NodeScope], now: i64) -> Result<PairRequestTicket, String> {
    let payload = checked_statement(statement, REQUEST_KIND, now)?;
    if payload["nodeId"] != node_id {
        return Err("the request is for a different Node".into());
    }
    let display_name = payload["displayName"].as_str().unwrap_or_default().to_string();
    if display_name.trim().is_empty() || display_name.chars().count() > 80 || display_name.chars().any(char::is_control) {
        return Err("invalid client display name".into());
    }
    let nonce = payload["nonce"].as_str().unwrap_or_default().to_string();
    let decoded = URL_SAFE_NO_PAD.decode(&nonce).map_err(|_| "invalid client pairing nonce".to_string())?;
    if decoded.len() < 16 || decoded.len() > 64 {
        return Err("invalid client pairing nonce".into());
    }
    let scopes = normalized_scopes(serde_json::from_value(payload["scopes"].clone()).map_err(|_| "invalid scopes".to_string())?);
    if scopes.is_empty() || scopes.iter().any(|scope| !allowed_scopes.contains(scope)) {
        return Err("the client requested a scope unavailable on this Node".into());
    }
    let _lock = PairingLock::acquire(directory)?;
    let mut state = load_state(directory)?;
    state.requests.retain(|request| request.expires_at > now);
    let fingerprint = &statement.identity.fingerprint;
    if state.requests.iter().any(|request| request.identity.fingerprint == *fingerprint && request.status == "denied") {
        return Err("this device was refused a moment ago; wait before asking again".into());
    }
    state.requests.retain(|request| request.identity.fingerprint != *fingerprint);
    if state.requests.iter().filter(|request| request.status == "pending").count() >= MAX_PENDING_REQUESTS {
        return Err("too many pairing requests are waiting; answer them first".into());
    }
    let request_id = random_token(16)?;
    let expires_at = now.saturating_add(REQUEST_TTL_MS);
    state.requests.push(StoredPairRequest { request_id: request_id.clone(), identity: statement.identity.clone(), display_name, nonce, scopes, created_at: now, expires_at, status: "pending".into(), client: None });
    save_state(directory, &state)?;
    Ok(PairRequestTicket { request_id, expires_at })
}

/// The requests the person at this Node can still answer, each with the code to compare.
pub fn pending_requests(directory: &Path, node_fingerprint: &str, now: i64) -> Result<Vec<PendingPairRequest>, String> {
    let _lock = PairingLock::acquire(directory)?;
    Ok(load_state(directory)?
        .requests
        .into_iter()
        .filter(|request| request.status == "pending" && request.expires_at > now)
        .map(|request| PendingPairRequest {
            code: pair_code(node_fingerprint, &request.identity.fingerprint, &request.nonce, &request.request_id),
            request_id: request.request_id,
            display_name: request.display_name,
            client_fingerprint: request.identity.fingerprint,
            created_at: request.created_at,
            expires_at: request.expires_at,
        })
        .collect())
}

/// Approves or refuses a pending request. Approval adds the device with the scopes it asked for.
pub fn decide(directory: &Path, request_id: &str, approve: bool, now: i64) -> Result<Option<PairedNodeClient>, String> {
    let _lock = PairingLock::acquire(directory)?;
    let mut state = load_state(directory)?;
    let index = state
        .requests
        .iter()
        .position(|request| request.request_id == request_id && request.status == "pending" && request.expires_at > now)
        .ok_or("that pairing request is not waiting any more")?;
    let request = state.requests[index].clone();
    if !approve {
        state.requests[index].status = "denied".into();
        state.requests[index].expires_at = now.saturating_add(REQUEST_TTL_MS);
        save_state(directory, &state)?;
        return Ok(None);
    }
    let client = PairedNodeClient {
        device_id: format!("device-{}", request.identity.fingerprint),
        display_name: request.display_name,
        identity: request.identity.clone(),
        scopes: request.scopes,
        paired_at: now,
        last_seen_at: None,
    };
    state.clients.retain(|existing| existing.identity.fingerprint != client.identity.fingerprint);
    state.clients.push(client.clone());
    state.clients.sort_by(|left, right| left.device_id.cmp(&right.device_id));
    state.requests[index].status = "approved".into();
    state.requests[index].client = Some(client.clone());
    state.requests[index].expires_at = now.saturating_add(REQUEST_TTL_MS);
    save_state(directory, &state)?;
    Ok(Some(client))
}

/// Lets the device that asked learn the answer. Only that device can read it.
pub fn request_status(directory: &Path, statement: &SignedDeviceStatement, now: i64) -> Result<PairRequestOutcome, String> {
    let payload = checked_statement(statement, STATUS_KIND, now)?;
    let request_id = payload["requestId"].as_str().unwrap_or_default();
    let _lock = PairingLock::acquire(directory)?;
    let state = load_state(directory)?;
    let request = state
        .requests
        .iter()
        .find(|request| request.request_id == request_id && request.identity.fingerprint == statement.identity.fingerprint)
        .ok_or("unknown pairing request")?;
    if request.expires_at <= now && request.status == "pending" {
        return Ok(PairRequestOutcome { status: "expired".into(), client: None });
    }
    Ok(PairRequestOutcome { status: request.status.clone(), client: request.client.clone() })
}

fn validate_pairing_proof(
    proof: &PairingProofPayload,
    offer: &StoredPairingOffer,
    allowed_scopes: &[NodeScope],
) -> Result<(), String> {
    if proof.offer_id != offer.offer_id || proof.node_id != offer.node_id {
        return Err("the client proof is bound to a different pairing offer".into());
    }
    if proof.client_display_name.trim().is_empty()
        || proof.client_display_name.chars().count() > 80
        || proof.client_display_name.chars().any(char::is_control)
    {
        return Err("invalid client display name".into());
    }
    let nonce = URL_SAFE_NO_PAD
        .decode(&proof.client_nonce)
        .map_err(|_| "invalid client pairing nonce".to_string())?;
    if nonce.len() < 16 || nonce.len() > 64 {
        return Err("invalid client pairing nonce".into());
    }
    let scopes = normalized_scopes(proof.requested_scopes.clone());
    if scopes.is_empty() || scopes.iter().any(|scope| !allowed_scopes.contains(scope)) {
        return Err("the client requested a scope unavailable on this Node".into());
    }
    Ok(())
}

fn register_failed_attempt(
    directory: &Path,
    state: &mut PairingState,
    offer: &mut StoredPairingOffer,
) -> Result<(), String> {
    offer.failed_attempts = offer.failed_attempts.saturating_add(1);
    state.active_offer = (offer.failed_attempts < MAX_PAIRING_ATTEMPTS).then_some(offer.clone());
    save_state(directory, state)
}

fn normalized_scopes(mut scopes: Vec<NodeScope>) -> Vec<NodeScope> {
    scopes.sort();
    scopes.dedup();
    scopes
}

fn random_token(bytes: usize) -> Result<String, String> {
    let mut value = vec![0_u8; bytes];
    getrandom::getrandom(&mut value).map_err(|error| error.to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

fn secret_hash(secret: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(SECRET_DOMAIN);
    digest.update(secret.as_bytes());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_identifier(value: &str, maximum: usize, label: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > maximum
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
    {
        return Err(format!("invalid {label}"));
    }
    Ok(())
}

fn load_state(directory: &Path) -> Result<PairingState, String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = directory.join(PAIRING_STATE_FILE);
    if !path.exists() {
        return Ok(PairingState::default());
    }
    let payload = fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&payload).map_err(|error| format!("invalid pairing state: {error}"))
}

fn save_state(directory: &Path, state: &PairingState) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = directory.join(PAIRING_STATE_FILE);
    let temporary = directory.join(format!("{PAIRING_STATE_FILE}.tmp-{}", process::id()));
    let payload = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    write_restricted(&temporary, &payload)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, &path).map_err(|error| error.to_string())?;
    Ok(())
}

struct PairingLock {
    path: PathBuf,
}

impl PairingLock {
    fn acquire(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        restrict_directory(directory)?;
        let path = directory.join(PAIRING_LOCK_FILE);
        for _ in 0..LOCK_RETRY_COUNT {
            match create_lock_file(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if !lock_owner_is_live(&path) {
                        let _ = fs::remove_file(&path);
                    } else {
                        thread::sleep(LOCK_RETRY_DELAY);
                    }
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("the Node pairing registry is busy".into())
    }
}

impl Drop for PairingLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_owner_is_live(path: &Path) -> bool {
    fs::read_to_string(path).ok().is_some_and(|value| {
        let mut parts = value.split_whitespace();
        let process_id = parts.next().and_then(|part| part.parse::<u32>().ok());
        let started_at = parts.next().and_then(|part| part.parse::<u64>().ok());
        process_id
            .zip(started_at)
            .is_some_and(|(id, start)| process_instance_is_live(id, start))
    })
}

fn create_lock_file(path: &Path) -> std::io::Result<()> {
    let started_at = process_start_time(process::id())
        .ok_or_else(|| std::io::Error::other("could not identify the pairing registry process"))?;
    let value = format!("{} {started_at}\n", process::id());
    create_restricted(path, value.as_bytes())
}

fn write_restricted(path: &Path, value: &[u8]) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    create_restricted(path, value).map_err(|error| error.to_string())
}

#[cfg(unix)]
fn create_restricted(path: &Path, value: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(value)?;
    file.sync_all()
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

#[cfg(not(unix))]
fn create_restricted(path: &Path, value: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(value)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn test_directory(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "lume-node-pairing-{name}-{}",
            random_token(8).expect("random test directory")
        ))
    }

    fn identities(directory: &Path) -> (NodeIdentity, NodeIdentity) {
        let node = NodeIdentity::load_or_create(&directory.join("node")).expect("node identity");
        let client =
            NodeIdentity::load_or_create(&directory.join("client")).expect("client identity");
        (node, client)
    }

    #[test]
    fn offer_persists_only_the_secret_hash() {
        let directory = test_directory("secret");
        let (node, _) = identities(&directory);
        let offer = begin_pairing(&directory, "node-home", &node).expect("offer");
        let persisted = fs::read_to_string(directory.join(PAIRING_STATE_FILE)).expect("state");
        assert!(!persisted.contains(&offer.secret));
        assert!(persisted.contains("secretHash"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn valid_proof_pairs_once_and_can_be_revoked() {
        let directory = test_directory("complete");
        let (node, client_identity) = identities(&directory);
        let offer = begin_pairing(&directory, "node-home", &node).expect("offer");
        let request = pairing_request(
            &client_identity,
            &offer,
            "Work laptop",
            vec![NodeScope::Observe],
        )
        .expect("request");
        let client = accept_pairing(&directory, &request, &[NodeScope::Observe], now_millis())
            .expect("pair client");
        assert_eq!(clients(&directory).expect("clients"), vec![client.clone()]);
        assert_eq!(
            accept_pairing(&directory, &request, &[NodeScope::Observe], now_millis())
                .err()
                .as_deref(),
            Some("no active Node pairing offer")
        );
        assert!(revoke(&directory, &client.device_id).expect("revoke"));
        assert!(clients(&directory).expect("clients").is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn pairing_rejects_unavailable_scopes() {
        let directory = test_directory("scopes");
        let (node, client_identity) = identities(&directory);
        let offer = begin_pairing(&directory, "node-home", &node).expect("offer");
        let request = pairing_request(
            &client_identity,
            &offer,
            "Work laptop",
            vec![NodeScope::Prompt],
        )
        .expect("request");
        assert_eq!(
            accept_pairing(&directory, &request, &[NodeScope::Observe], now_millis())
                .err()
                .as_deref(),
            Some("the client requested a scope unavailable on this Node")
        );
        assert!(clients(&directory).expect("clients").is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn expired_and_guessed_offers_fail_closed() {
        let directory = test_directory("expiry");
        let (node, client_identity) = identities(&directory);
        let offer = begin_pairing(&directory, "node-home", &node).expect("offer");
        let mut request = pairing_request(
            &client_identity,
            &offer,
            "Work laptop",
            vec![NodeScope::Observe],
        )
        .expect("request");
        request.secret = "wrong-secret".into();
        for _ in 0..MAX_PAIRING_ATTEMPTS {
            assert_eq!(
                accept_pairing(&directory, &request, &[NodeScope::Observe], now_millis())
                    .err()
                    .as_deref(),
                Some("invalid Node pairing offer")
            );
        }
        assert_eq!(
            accept_pairing(&directory, &request, &[NodeScope::Observe], now_millis())
                .err()
                .as_deref(),
            Some("no active Node pairing offer")
        );

        let fresh = begin_pairing(&directory, "node-home", &node).expect("fresh offer");
        let fresh_request = pairing_request(
            &client_identity,
            &fresh,
            "Work laptop",
            vec![NodeScope::Observe],
        )
        .expect("fresh request");
        assert_eq!(
            accept_pairing(
                &directory,
                &fresh_request,
                &[NodeScope::Observe],
                fresh.expires_at + 1
            )
            .err()
            .as_deref(),
            Some("the Node pairing offer expired")
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn a_request_waits_for_approval_and_both_sides_derive_the_same_code() {
        let directory = test_directory("request");
        let (node, device) = identities(&directory);
        let node_fp = node.public().fingerprint.clone();
        let (statement, nonce) = pair_request_statement(&device, "node-home", "Phone", &[NodeScope::Observe]).unwrap();
        let ticket = submit_request(&directory, "node-home", &statement, &[NodeScope::Observe], now_millis()).unwrap();
        assert!(clients(&directory).unwrap().is_empty(), "a request grants nothing");
        let pending = pending_requests(&directory, &node_fp, now_millis()).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].code, pair_code(&node_fp, &device.public().fingerprint, &nonce, &ticket.request_id));
        let poll = pair_status_statement(&device, &ticket.request_id).unwrap();
        assert_eq!(request_status(&directory, &poll, now_millis()).unwrap().status, "pending");
        let client = decide(&directory, &ticket.request_id, true, now_millis()).unwrap().unwrap();
        assert_eq!(clients(&directory).unwrap(), vec![client.clone()]);
        let outcome = request_status(&directory, &poll, now_millis()).unwrap();
        assert_eq!((outcome.status.as_str(), outcome.client), ("approved", Some(client)));
        assert!(decide(&directory, &ticket.request_id, true, now_millis()).is_err(), "an answered request cannot be answered again");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn a_machine_in_the_middle_gets_a_different_code() {
        let directory = test_directory("code");
        let (node, device) = identities(&directory);
        let other = NodeIdentity::load_or_create(&directory.join("other")).unwrap();
        let honest = pair_code(&node.public().fingerprint, &device.public().fingerprint, "n", "r");
        assert_ne!(honest, pair_code(&other.public().fingerprint, &device.public().fingerprint, "n", "r"));
        assert_ne!(honest, pair_code(&node.public().fingerprint, &other.public().fingerprint, "n", "r"));
        assert_eq!(honest.len(), 6);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn refused_requests_cannot_be_repeated_at_once_and_expire() {
        let directory = test_directory("denied");
        let (_, device) = identities(&directory);
        let (statement, _) = pair_request_statement(&device, "node-home", "Phone", &[NodeScope::Observe]).unwrap();
        let ticket = submit_request(&directory, "node-home", &statement, &[NodeScope::Observe], now_millis()).unwrap();
        assert!(decide(&directory, &ticket.request_id, false, now_millis()).unwrap().is_none());
        assert!(clients(&directory).unwrap().is_empty());
        let (again, _) = pair_request_statement(&device, "node-home", "Phone", &[NodeScope::Observe]).unwrap();
        assert!(submit_request(&directory, "node-home", &again, &[NodeScope::Observe], now_millis()).is_err());
        let late = now_millis() + REQUEST_TTL_MS + 1;
        assert!(decide(&directory, &ticket.request_id, true, late).is_err(), "an expired request cannot be approved");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn requests_are_bound_to_the_node_scope_and_signer() {
        let directory = test_directory("bound");
        let (_, device) = identities(&directory);
        let stranger = NodeIdentity::load_or_create(&directory.join("stranger")).unwrap();
        let (wrong_node, _) = pair_request_statement(&device, "other-node", "Phone", &[NodeScope::Observe]).unwrap();
        assert!(submit_request(&directory, "node-home", &wrong_node, &[NodeScope::Observe], now_millis()).is_err());
        let (greedy, _) = pair_request_statement(&device, "node-home", "Phone", &[NodeScope::Prompt]).unwrap();
        assert!(submit_request(&directory, "node-home", &greedy, &[NodeScope::Observe], now_millis()).is_err());
        let (statement, _) = pair_request_statement(&device, "node-home", "Phone", &[NodeScope::Observe]).unwrap();
        let ticket = submit_request(&directory, "node-home", &statement, &[NodeScope::Observe], now_millis()).unwrap();
        decide(&directory, &ticket.request_id, true, now_millis()).unwrap();
        let spy = pair_status_statement(&stranger, &ticket.request_id).unwrap();
        assert!(request_status(&directory, &spy, now_millis()).is_err(), "only the requester reads the answer");
        let mut stale = statement.clone();
        stale.payload["at"] = serde_json::json!(1);
        assert!(submit_request(&directory, "node-home", &stale, &[NodeScope::Observe], now_millis()).is_err());
        for index in 0..=MAX_PENDING_REQUESTS {
            let other = NodeIdentity::load_or_create(&directory.join(format!("crowd-{index}"))).unwrap();
            let (crowd, _) = pair_request_statement(&other, "node-home", "Crowd", &[NodeScope::Observe]).unwrap();
            let result = submit_request(&directory, "node-home", &crowd, &[NodeScope::Observe], now_millis());
            assert_eq!(result.is_ok(), index < MAX_PENDING_REQUESTS, "at most {MAX_PENDING_REQUESTS} requests wait at once");
        }
        let _ = fs::remove_dir_all(directory);
    }
}
