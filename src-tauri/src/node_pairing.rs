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
}
