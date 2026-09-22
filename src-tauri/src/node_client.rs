use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    path::Path,
    process,
    sync::Arc,
    time::{Duration, Instant},
};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms},
    pki_types::{CertificateDer, ServerName, UnixTime},
    CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, Error as TlsError,
    SignatureScheme, StreamOwned,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    distributed_protocol::{NodeScope, MAX_PROTOCOL_VERSION, MIN_PROTOCOL_VERSION},
    node_identity::{verify, NodeIdentity, PublicDeviceIdentity},
    node_network::{authenticated_request, NodeServerIdentity},
    node_pairing::{self, NodePairingOffer, PairedNodeClient},
    node_service::NodeHealth,
    state::now_millis,
};

const MDNS_SERVICE_TYPE: &str = "_lume-node._tcp.local.";
const REMOTE_STATE_FILE: &str = "remote-nodes.json";
const CLIENT_IDENTITY_DIRECTORY: &str = "client-identity";
const MAX_HTTP_RESPONSE_BYTES: usize = 3 * 1024 * 1024;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredNode {
    pub node_id: String,
    pub address: String,
    pub port: u16,
    pub identity_fingerprint: String,
    pub certificate_sha256: String,
    pub protocol_minimum: u16,
    pub protocol_maximum: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteNode {
    pub node_id: String,
    pub address: String,
    pub port: u16,
    pub identity: PublicDeviceIdentity,
    pub certificate_sha256: String,
    pub device_id: String,
    pub paired_at: i64,
    pub last_seen_at: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RemoteNodeState {
    nodes: Vec<RemoteNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PairingUri {
    node_id: String,
    offer_id: String,
    secret: String,
    identity_fingerprint: String,
}

pub fn discover(timeout: Duration) -> Result<Vec<DiscoveredNode>, String> {
    let daemon = ServiceDaemon::new().map_err(|error| error.to_string())?;
    let receiver = daemon
        .browse(MDNS_SERVICE_TYPE)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + timeout;
    let mut nodes = HashMap::<(String, String, u16), DiscoveredNode>::new();
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let event = match receiver.recv_timeout(remaining.min(Duration::from_millis(250))) {
            Ok(event) => event,
            Err(_) if Instant::now() < deadline => continue,
            Err(_) => break,
        };
        let ServiceEvent::ServiceResolved(service) = event else {
            continue;
        };
        let Some(node_id) = service.get_property_val_str("id") else {
            continue;
        };
        let Some(identity_fingerprint) = service.get_property_val_str("fingerprint") else {
            continue;
        };
        let Some(certificate_sha256) = service.get_property_val_str("certificate") else {
            continue;
        };
        let protocol_minimum = service
            .get_property_val_str("protocolMin")
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(0);
        let protocol_maximum = service
            .get_property_val_str("protocolMax")
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(0);
        if !valid_identifier(node_id, 128)
            || !valid_fingerprint(identity_fingerprint)
            || !valid_fingerprint(certificate_sha256)
            || protocol_minimum > MAX_PROTOCOL_VERSION
            || protocol_maximum < MIN_PROTOCOL_VERSION
        {
            continue;
        }
        for address in service
            .get_addresses()
            .iter()
            .map(|address| address.to_ip_addr())
            .filter(|address| address.is_ipv4() && !address.is_unspecified())
        {
            let node = DiscoveredNode {
                node_id: node_id.into(),
                address: address.to_string(),
                port: service.get_port(),
                identity_fingerprint: identity_fingerprint.into(),
                certificate_sha256: certificate_sha256.into(),
                protocol_minimum,
                protocol_maximum,
            };
            nodes.insert(
                (node.node_id.clone(), node.address.clone(), node.port),
                node,
            );
        }
    }
    let _ = daemon.stop_browse(MDNS_SERVICE_TYPE);
    let _ = daemon.shutdown();
    let mut nodes = nodes.into_values().collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        left.node_id
            .cmp(&right.node_id)
            .then_with(|| left.address.cmp(&right.address))
    });
    Ok(nodes)
}

pub fn pair(
    directory: &Path,
    pairing_uri: &str,
    discovery_timeout: Duration,
) -> Result<RemoteNode, String> {
    let pairing = parse_pairing_uri(pairing_uri)?;
    let discovered = discover(discovery_timeout)?
        .into_iter()
        .find(|node| {
            node.node_id == pairing.node_id
                && node.identity_fingerprint == pairing.identity_fingerprint
        })
        .ok_or_else(|| "the paired Lume Node was not found on this LAN".to_string())?;
    pair_at(directory, &pairing, &discovered)
}

fn pair_at(
    directory: &Path,
    pairing: &PairingUri,
    discovered: &DiscoveredNode,
) -> Result<RemoteNode, String> {
    if pairing.node_id != discovered.node_id
        || pairing.identity_fingerprint != discovered.identity_fingerprint
    {
        return Err("the discovered Node does not match the pairing offer".into());
    }
    let identity_response = tls_json_request(
        discovered,
        "GET",
        "/v1/identity",
        None,
        &discovered.certificate_sha256,
    )?;
    let server_identity = serde_json::from_value::<NodeServerIdentity>(identity_response)
        .map_err(|_| "invalid Node identity response".to_string())?;
    validate_server_identity(pairing, discovered, &server_identity)?;

    let client_identity = NodeIdentity::load_or_create(&directory.join(CLIENT_IDENTITY_DIRECTORY))?;
    let offer = NodePairingOffer {
        offer_id: pairing.offer_id.clone(),
        node_id: pairing.node_id.clone(),
        node_identity: server_identity.identity.clone(),
        secret: pairing.secret.clone(),
        expires_at: 0,
        qr_payload: String::new(),
    };
    let display_name = sysinfo::System::host_name().unwrap_or_else(|| "Lume client".into());
    let request = node_pairing::pairing_request(
        &client_identity,
        &offer,
        &display_name,
        vec![NodeScope::Observe],
    )?;
    let response = tls_json_request(
        discovered,
        "POST",
        "/v1/pair",
        Some(serde_json::to_value(request).map_err(|error| error.to_string())?),
        &server_identity.certificate_sha256,
    )?;
    let paired = response
        .get("client")
        .cloned()
        .ok_or_else(|| "the Node did not return paired client credentials".to_string())?;
    let paired = serde_json::from_value::<PairedNodeClient>(paired)
        .map_err(|_| "invalid paired client response".to_string())?;
    if paired.identity != *client_identity.public() || !paired.scopes.contains(&NodeScope::Observe)
    {
        return Err("the Node returned a different client identity or scope".into());
    }
    let remote = RemoteNode {
        node_id: server_identity.node_id,
        address: discovered.address.clone(),
        port: discovered.port,
        identity: server_identity.identity,
        certificate_sha256: server_identity.certificate_sha256,
        device_id: paired.device_id,
        paired_at: paired.paired_at,
        last_seen_at: None,
    };
    upsert_remote(directory, remote.clone())?;
    Ok(remote)
}

pub fn remote_health(
    directory: &Path,
    node_id: &str,
    rediscovery_timeout: Duration,
) -> Result<NodeHealth, String> {
    let mut remote = remotes(directory)?
        .into_iter()
        .find(|remote| remote.node_id == node_id)
        .ok_or_else(|| "remote Node is not paired on this client".to_string())?;
    match health_at(directory, &remote) {
        Ok(health) => {
            remote.last_seen_at = Some(now_millis());
            upsert_remote(directory, remote)?;
            Ok(health)
        }
        Err(first_error) => {
            let Some(discovered) = discover(rediscovery_timeout)?.into_iter().find(|node| {
                node.node_id == remote.node_id
                    && node.identity_fingerprint == remote.identity.fingerprint
                    && node.certificate_sha256 == remote.certificate_sha256
            }) else {
                return Err(first_error);
            };
            remote.address = discovered.address;
            remote.port = discovered.port;
            let health = health_at(directory, &remote)?;
            remote.last_seen_at = Some(now_millis());
            upsert_remote(directory, remote)?;
            Ok(health)
        }
    }
}

fn health_at(directory: &Path, remote: &RemoteNode) -> Result<NodeHealth, String> {
    let identity = NodeIdentity::load_or_create(&directory.join(CLIENT_IDENTITY_DIRECTORY))?;
    let request = authenticated_request(
        &identity,
        &remote.device_id,
        &remote.node_id,
        "POST",
        "/v1/health",
        now_millis(),
    )?;
    let discovered = DiscoveredNode {
        node_id: remote.node_id.clone(),
        address: remote.address.clone(),
        port: remote.port,
        identity_fingerprint: remote.identity.fingerprint.clone(),
        certificate_sha256: remote.certificate_sha256.clone(),
        protocol_minimum: MIN_PROTOCOL_VERSION,
        protocol_maximum: MAX_PROTOCOL_VERSION,
    };
    let response = tls_json_request(
        &discovered,
        "POST",
        "/v1/health",
        Some(serde_json::to_value(request).map_err(|error| error.to_string())?),
        &remote.certificate_sha256,
    )?;
    let health = serde_json::from_value::<NodeHealth>(response)
        .map_err(|_| "invalid remote Node health response".to_string())?;
    verify(&health.identity_attestation)?;
    if health.node_id != remote.node_id
        || health.identity_fingerprint != remote.identity.fingerprint
        || health.identity_attestation.identity != remote.identity
        || health.identity_attestation.payload["nodeId"] != remote.node_id
    {
        return Err("remote Node health identity mismatch".into());
    }
    Ok(health)
}

pub fn remotes(directory: &Path) -> Result<Vec<RemoteNode>, String> {
    let mut nodes = load_remote_state(directory)?.nodes;
    nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    Ok(nodes)
}

pub fn forget(directory: &Path, node_id: &str) -> Result<bool, String> {
    let mut state = load_remote_state(directory)?;
    let previous = state.nodes.len();
    state.nodes.retain(|node| node.node_id != node_id);
    let removed = state.nodes.len() != previous;
    if removed {
        save_remote_state(directory, &state)?;
    }
    Ok(removed)
}

fn validate_server_identity(
    pairing: &PairingUri,
    discovered: &DiscoveredNode,
    server: &NodeServerIdentity,
) -> Result<(), String> {
    verify(&server.binding_attestation)?;
    let payload = &server.binding_attestation.payload;
    if server.node_id != pairing.node_id
        || server.node_id != discovered.node_id
        || server.identity.fingerprint != pairing.identity_fingerprint
        || server.identity.fingerprint != discovered.identity_fingerprint
        || server.certificate_sha256 != discovered.certificate_sha256
        || server.binding_attestation.identity != server.identity
        || payload["nodeId"] != server.node_id
        || payload["certificateSha256"] != server.certificate_sha256
        || payload["protocolMinimum"].as_u64() != Some(server.protocol_minimum.into())
        || payload["protocolMaximum"].as_u64() != Some(server.protocol_maximum.into())
        || server.protocol_minimum > MAX_PROTOCOL_VERSION
        || server.protocol_maximum < MIN_PROTOCOL_VERSION
    {
        return Err("the Node identity or TLS binding does not match the pairing offer".into());
    }
    Ok(())
}

fn parse_pairing_uri(uri: &str) -> Result<PairingUri, String> {
    let query = uri
        .trim()
        .strip_prefix("lume://pair-node?")
        .ok_or_else(|| "invalid Lume Node pairing URI".to_string())?;
    let values = query
        .split('&')
        .filter_map(|item| item.split_once('='))
        .collect::<HashMap<_, _>>();
    if values.get("v") != Some(&"1") {
        return Err("unsupported Lume Node pairing version".into());
    }
    let node_id = values.get("node").copied().unwrap_or_default();
    let offer_id = values.get("offer").copied().unwrap_or_default();
    let secret = values.get("secret").copied().unwrap_or_default();
    let identity_fingerprint = values.get("fingerprint").copied().unwrap_or_default();
    if !valid_identifier(node_id, 128)
        || !valid_token(offer_id, 128)
        || !valid_token(secret, 128)
        || !valid_fingerprint(identity_fingerprint)
    {
        return Err("invalid Lume Node pairing URI".into());
    }
    Ok(PairingUri {
        node_id: node_id.into(),
        offer_id: offer_id.into(),
        secret: secret.into(),
        identity_fingerprint: identity_fingerprint.into(),
    })
}

fn tls_json_request(
    node: &DiscoveredNode,
    method: &str,
    path: &str,
    body: Option<Value>,
    certificate_sha256: &str,
) -> Result<Value, String> {
    let address = node
        .address
        .parse::<IpAddr>()
        .map_err(|_| "invalid Node address".to_string())?;
    let socket = SocketAddr::new(address, node.port);
    let stream = TcpStream::connect_timeout(&socket, CONNECTION_TIMEOUT)
        .map_err(|error| format!("could not connect to the Lume Node: {error}"))?;
    stream
        .set_read_timeout(Some(CONNECTION_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(CONNECTION_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let verifier = Arc::new(PinnedCertificateVerifier::new(certificate_sha256)?);
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let server_name = ServerName::try_from("localhost")
        .map_err(|_| "invalid Node TLS server name".to_string())?
        .to_owned();
    let connection =
        ClientConnection::new(Arc::new(config), server_name).map_err(|error| error.to_string())?;
    let mut stream = StreamOwned::new(connection, stream);
    let body = body
        .map(|body| serde_json::to_vec(&body).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.write_all(&body))
        .and_then(|_| stream.flush())
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .take((MAX_HTTP_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    if response.len() > MAX_HTTP_RESPONSE_BYTES {
        return Err("Node response is too large".into());
    }
    parse_http_response(&response)
}

fn parse_http_response(response: &[u8]) -> Result<Value, String> {
    let header_end = find_bytes(response, b"\r\n\r\n")
        .map(|index| index + 4)
        .ok_or_else(|| "invalid Node HTTP response".to_string())?;
    let headers = std::str::from_utf8(&response[..header_end])
        .map_err(|_| "invalid Node HTTP response".to_string())?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "invalid Node HTTP status".to_string())?;
    let body = serde_json::from_slice::<Value>(&response[header_end..])
        .map_err(|_| "invalid Node JSON response".to_string())?;
    if !(200..300).contains(&status) {
        let message = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Node request failed");
        return Err(format!("{message} (HTTP {status})"));
    }
    Ok(body)
}

#[derive(Debug)]
struct PinnedCertificateVerifier {
    expected_sha256: String,
    algorithms: WebPkiSupportedAlgorithms,
}

impl PinnedCertificateVerifier {
    fn new(expected_sha256: &str) -> Result<Self, String> {
        if !valid_fingerprint(expected_sha256) {
            return Err("invalid pinned Node certificate fingerprint".into());
        }
        Ok(Self {
            expected_sha256: expected_sha256.into(),
            algorithms: rustls::crypto::ring::default_provider().signature_verification_algorithms,
        })
    }
}

impl ServerCertVerifier for PinnedCertificateVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        if sha256_hex(end_entity.as_ref()) != self.expected_sha256 {
            return Err(TlsError::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, certificate, signature, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, certificate, signature, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

fn upsert_remote(directory: &Path, remote: RemoteNode) -> Result<(), String> {
    let mut state = load_remote_state(directory)?;
    state.nodes.retain(|node| node.node_id != remote.node_id);
    state.nodes.push(remote);
    state
        .nodes
        .sort_by(|left, right| left.node_id.cmp(&right.node_id));
    save_remote_state(directory, &state)
}

fn load_remote_state(directory: &Path) -> Result<RemoteNodeState, String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    restrict_directory(directory)?;
    let path = directory.join(REMOTE_STATE_FILE);
    if !path.exists() {
        return Ok(RemoteNodeState::default());
    }
    let payload = fs::read(path).map_err(|error| error.to_string())?;
    serde_json::from_slice(&payload).map_err(|error| format!("invalid remote Node state: {error}"))
}

fn save_remote_state(directory: &Path, state: &RemoteNodeState) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    restrict_directory(directory)?;
    let path = directory.join(REMOTE_STATE_FILE);
    let temporary = directory.join(format!("{REMOTE_STATE_FILE}.tmp-{}", process::id()));
    let payload = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    write_restricted(&temporary, &payload)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn valid_identifier(value: &str, maximum: usize) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= maximum
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
}

fn valid_token(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
}

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
    use std::{env, path::PathBuf};

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    use crate::node_network::NodeNetworkServer;

    fn test_directory(name: &str) -> PathBuf {
        let mut random = [0_u8; 8];
        getrandom::getrandom(&mut random).expect("random test directory");
        env::temp_dir().join(format!(
            "lume-node-client-{name}-{}",
            URL_SAFE_NO_PAD.encode(random)
        ))
    }

    #[test]
    fn pairing_uri_requires_all_high_entropy_fields() {
        let parsed = parse_pairing_uri(
            "lume://pair-node?v=1&node=node-home&offer=abc_DEF-123&secret=secret_ABC-123&fingerprint=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .expect("pairing URI");
        assert_eq!(parsed.node_id, "node-home");
        assert!(parse_pairing_uri("lume://pair-node?v=1&node=node-home").is_err());
    }

    #[test]
    fn pinned_verifier_rejects_a_different_certificate() {
        let verifier = PinnedCertificateVerifier::new(&"a".repeat(64)).expect("verifier");
        let certificate = CertificateDer::from(vec![1, 2, 3]);
        assert!(verifier
            .verify_server_cert(
                &certificate,
                &[],
                &ServerName::try_from("localhost").expect("server name"),
                &[],
                UnixTime::since_unix_epoch(Duration::from_secs(1))
            )
            .is_err());
    }

    #[test]
    fn http_errors_do_not_hide_the_remote_status() {
        let response = b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\r\n{\"error\":\"Node authentication failed\"}";
        assert_eq!(
            parse_http_response(response).err().as_deref(),
            Some("Node authentication failed (HTTP 401)")
        );
    }

    #[test]
    #[ignore = "requires a loopback socket"]
    fn client_pairs_and_reads_remote_health() {
        let server_directory = test_directory("server");
        let client_directory = test_directory("client");
        let initial_health = crate::node_service::health(&server_directory).expect("Node health");
        let identity = NodeIdentity::load_or_create(&server_directory).expect("Node identity");
        let server =
            NodeNetworkServer::start(&server_directory, &initial_health.node_id, 0, &identity)
                .expect("Node server");
        let offer =
            node_pairing::begin_pairing(&server_directory, &initial_health.node_id, &identity)
                .expect("pairing offer");
        let pairing = PairingUri {
            node_id: offer.node_id,
            offer_id: offer.offer_id,
            secret: offer.secret,
            identity_fingerprint: offer.node_identity.fingerprint,
        };
        let discovered = DiscoveredNode {
            node_id: server.identity.node_id.clone(),
            address: "127.0.0.1".into(),
            port: server.port,
            identity_fingerprint: server.identity.identity.fingerprint.clone(),
            certificate_sha256: server.identity.certificate_sha256.clone(),
            protocol_minimum: server.identity.protocol_minimum,
            protocol_maximum: server.identity.protocol_maximum,
        };

        let remote = pair_at(&client_directory, &pairing, &discovered).expect("paired Node");
        let health = remote_health(&client_directory, &remote.node_id, Duration::ZERO)
            .expect("authenticated remote health");
        assert_eq!(health.node_id, remote.node_id);
        assert_eq!(health.identity_fingerprint, remote.identity.fingerprint);

        drop(server);
        let _ = fs::remove_dir_all(server_directory);
        let _ = fs::remove_dir_all(client_directory);
    }
}
