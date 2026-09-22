use std::collections::{BTreeSet, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MIN_PROTOCOL_VERSION: u16 = 1;
pub const MAX_PROTOCOL_VERSION: u16 = 1;
pub const MAX_CONTROL_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_STATE_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_STREAM_PAYLOAD_BYTES: usize = 256 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 100 * 1024 * 1024;
pub const ARTIFACT_CHUNK_BYTES: u32 = 256 * 1024;
pub const MAX_PENDING_COMMANDS: usize = 128;
pub const MAX_EVENT_JOURNAL_ENTRIES: usize = 10_000;
pub const EVENT_RETENTION_MS: i64 = 24 * 60 * 60 * 1_000;
pub const COMMAND_TTL_MS: i64 = 60 * 1_000;
pub const IDEMPOTENCY_RETENTION_MS: i64 = 24 * 60 * 60 * 1_000;

pub const BASE_FEATURES: &[&str] = &[
    "capability_advertisement",
    "command_idempotency",
    "event_cursors",
    "execution_ownership",
    "resumable_snapshots",
    "scoped_devices",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Node,
    Desktop,
    Mobile,
    Web,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeScope {
    Observe,
    Prompt,
    Approve,
    Interrupt,
    ReadFiles,
    DownloadArtifacts,
    RunTools,
    Administer,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeCapability {
    SessionMonitoring,
    ManagedPrompts,
    Approvals,
    LocalInference,
    CodingRunner,
    ArtifactTransfer,
    ResourceTelemetry,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOrigin {
    NodeManaged,
    ExternalObserved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageChannel {
    Control,
    State,
    Stream,
    Artifact,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    LanTlsWebSocket,
    WebRtcDataChannel,
    RelayTlsWebSocket,
    PrivateNetworkTls,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentity {
    pub device_id: String,
    pub display_name: String,
    pub kind: DeviceKind,
    pub public_key_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolHello {
    pub minimum_version: u16,
    pub maximum_version: u16,
    pub features: BTreeSet<String>,
    pub identity: DeviceIdentity,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolAgreement {
    pub protocol_version: u16,
    pub features: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOwnership {
    pub execution_id: String,
    pub node_id: String,
    pub controller_device_id: Option<String>,
    pub origin: ExecutionOrigin,
    pub generation: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandEnvelope {
    pub protocol_version: u16,
    pub command_id: String,
    pub idempotency_key: String,
    pub node_id: String,
    pub client_id: String,
    pub execution_id: Option<String>,
    pub required_scope: NodeScope,
    pub issued_at: i64,
    pub expires_at: i64,
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub protocol_version: u16,
    pub event_id: String,
    pub sequence: u64,
    pub occurred_at: i64,
    pub node_id: String,
    pub execution_id: Option<String>,
    pub channel: MessageChannel,
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeAdvertisement {
    pub identity: DeviceIdentity,
    pub protocol_minimum: u16,
    pub protocol_maximum: u16,
    pub capabilities: Vec<NodeCapability>,
    pub transports: Vec<TransportKind>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactManifest {
    pub artifact_id: String,
    pub name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub chunk_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolViolation {
    UnsupportedVersion,
    InvalidIdentity,
    InvalidCommand,
    InvalidEvent,
    ExpiredCommand,
    PayloadTooLarge,
    Replay,
    SequenceGap { expected: u64, received: u64 },
    ArtifactTooLarge,
}

pub fn local_hello(identity: DeviceIdentity) -> ProtocolHello {
    ProtocolHello {
        minimum_version: MIN_PROTOCOL_VERSION,
        maximum_version: MAX_PROTOCOL_VERSION,
        features: BASE_FEATURES
            .iter()
            .map(|feature| (*feature).to_string())
            .collect(),
        identity,
    }
}

pub fn negotiate(
    local: &ProtocolHello,
    remote: &ProtocolHello,
) -> Result<ProtocolAgreement, ProtocolViolation> {
    validate_identity(&local.identity)?;
    validate_identity(&remote.identity)?;
    let minimum = local.minimum_version.max(remote.minimum_version);
    let maximum = local.maximum_version.min(remote.maximum_version);
    if minimum > maximum {
        return Err(ProtocolViolation::UnsupportedVersion);
    }
    Ok(ProtocolAgreement {
        protocol_version: maximum,
        features: local
            .features
            .intersection(&remote.features)
            .cloned()
            .collect(),
    })
}

pub fn validate_command(command: &CommandEnvelope, now: i64) -> Result<(), ProtocolViolation> {
    if command.protocol_version < MIN_PROTOCOL_VERSION
        || command.protocol_version > MAX_PROTOCOL_VERSION
    {
        return Err(ProtocolViolation::UnsupportedVersion);
    }
    if !valid_identifier(&command.command_id, 128)
        || !valid_identifier(&command.idempotency_key, 128)
        || !valid_identifier(&command.node_id, 128)
        || !valid_identifier(&command.client_id, 128)
        || command
            .execution_id
            .as_deref()
            .is_some_and(|value| !valid_identifier(value, 256))
        || command.expires_at < command.issued_at
        || command.expires_at.saturating_sub(command.issued_at) > COMMAND_TTL_MS
    {
        return Err(ProtocolViolation::InvalidCommand);
    }
    if now > command.expires_at {
        return Err(ProtocolViolation::ExpiredCommand);
    }
    let payload_size = serde_json::to_vec(&command.payload)
        .map_err(|_| ProtocolViolation::InvalidCommand)?
        .len();
    if payload_size > MAX_CONTROL_PAYLOAD_BYTES {
        return Err(ProtocolViolation::PayloadTooLarge);
    }
    Ok(())
}

pub fn validate_artifact(artifact: &ArtifactManifest) -> Result<(), ProtocolViolation> {
    if !valid_identifier(&artifact.artifact_id, 128)
        || artifact.name.trim().is_empty()
        || artifact.name.chars().count() > 255
        || artifact.name.contains(['/', '\\'])
        || artifact.name == "."
        || artifact.name == ".."
        || artifact.name.chars().any(char::is_control)
        || artifact.content_type.trim().is_empty()
        || artifact.content_type.chars().count() > 127
        || artifact.sha256.len() != 64
        || !artifact
            .sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        || artifact.chunk_bytes == 0
        || artifact.chunk_bytes > ARTIFACT_CHUNK_BYTES
    {
        return Err(ProtocolViolation::InvalidCommand);
    }
    if artifact.size_bytes > MAX_ARTIFACT_BYTES {
        return Err(ProtocolViolation::ArtifactTooLarge);
    }
    Ok(())
}

pub fn validate_event(event: &EventEnvelope) -> Result<(), ProtocolViolation> {
    if event.protocol_version < MIN_PROTOCOL_VERSION
        || event.protocol_version > MAX_PROTOCOL_VERSION
    {
        return Err(ProtocolViolation::UnsupportedVersion);
    }
    if !valid_identifier(&event.event_id, 128)
        || !valid_identifier(&event.node_id, 128)
        || event
            .execution_id
            .as_deref()
            .is_some_and(|value| !valid_identifier(value, 256))
    {
        return Err(ProtocolViolation::InvalidEvent);
    }
    let maximum = match event.channel {
        MessageChannel::Control => MAX_CONTROL_PAYLOAD_BYTES,
        MessageChannel::State => MAX_STATE_PAYLOAD_BYTES,
        MessageChannel::Stream => MAX_STREAM_PAYLOAD_BYTES,
        MessageChannel::Artifact => MAX_CONTROL_PAYLOAD_BYTES,
    };
    let payload_size = serde_json::to_vec(&event.payload)
        .map_err(|_| ProtocolViolation::InvalidEvent)?
        .len();
    if payload_size > maximum {
        return Err(ProtocolViolation::PayloadTooLarge);
    }
    Ok(())
}

fn validate_identity(identity: &DeviceIdentity) -> Result<(), ProtocolViolation> {
    if !valid_identifier(&identity.device_id, 128)
        || identity.display_name.trim().is_empty()
        || identity.display_name.chars().count() > 80
        || identity.public_key_fingerprint.len() != 64
        || !identity
            .public_key_fingerprint
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(ProtocolViolation::InvalidIdentity);
    }
    Ok(())
}

fn valid_identifier(value: &str, maximum: usize) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= maximum
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
}

#[derive(Debug)]
pub struct EventCursor {
    last_sequence: u64,
    recent_ids: VecDeque<String>,
    recent_id_set: HashSet<String>,
    capacity: usize,
}

impl EventCursor {
    pub fn new(sequence: u64, capacity: usize) -> Self {
        Self {
            last_sequence: sequence,
            recent_ids: VecDeque::with_capacity(capacity.max(1)),
            recent_id_set: HashSet::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
        }
    }

    pub fn accept(&mut self, event: &EventEnvelope) -> Result<(), ProtocolViolation> {
        validate_event(event)?;
        if event.sequence <= self.last_sequence || self.recent_id_set.contains(&event.event_id) {
            return Err(ProtocolViolation::Replay);
        }
        let expected = self.last_sequence.saturating_add(1);
        if event.sequence != expected {
            return Err(ProtocolViolation::SequenceGap {
                expected,
                received: event.sequence,
            });
        }
        self.last_sequence = event.sequence;
        self.recent_ids.push_back(event.event_id.clone());
        self.recent_id_set.insert(event.event_id.clone());
        while self.recent_ids.len() > self.capacity {
            if let Some(expired) = self.recent_ids.pop_front() {
                self.recent_id_set.remove(&expired);
            }
        }
        Ok(())
    }

    pub fn reset_from_snapshot(&mut self, sequence: u64) {
        self.last_sequence = sequence;
        self.recent_ids.clear();
        self.recent_id_set.clear();
    }

    pub fn sequence(&self) -> u64 {
        self.last_sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(id: &str, kind: DeviceKind) -> DeviceIdentity {
        DeviceIdentity {
            device_id: id.into(),
            display_name: id.into(),
            kind,
            public_key_fingerprint: "a".repeat(64),
        }
    }

    fn event(sequence: u64, id: &str) -> EventEnvelope {
        EventEnvelope {
            protocol_version: MAX_PROTOCOL_VERSION,
            event_id: id.into(),
            sequence,
            occurred_at: 1,
            node_id: "node-home".into(),
            execution_id: Some("execution-1".into()),
            channel: MessageChannel::Stream,
            payload: serde_json::json!({ "delta": "ok" }),
        }
    }

    #[test]
    fn negotiation_uses_the_highest_shared_version_and_feature_intersection() {
        let local = local_hello(identity("desktop-work", DeviceKind::Desktop));
        let mut remote = local_hello(identity("node-home", DeviceKind::Node));
        remote.features.insert("local_inference".into());

        let agreement = negotiate(&local, &remote).expect("compatible protocol");

        assert_eq!(agreement.protocol_version, MAX_PROTOCOL_VERSION);
        assert_eq!(agreement.features, local.features);
    }

    #[test]
    fn incompatible_protocol_versions_fail_closed() {
        let local = local_hello(identity("desktop-work", DeviceKind::Desktop));
        let mut remote = local_hello(identity("node-home", DeviceKind::Node));
        remote.minimum_version = 2;
        remote.maximum_version = 2;

        assert_eq!(
            negotiate(&local, &remote),
            Err(ProtocolViolation::UnsupportedVersion)
        );
    }

    #[test]
    fn mutation_commands_require_bounded_identity_ttl_and_payload() {
        let mut command = CommandEnvelope {
            protocol_version: MAX_PROTOCOL_VERSION,
            command_id: "command-1".into(),
            idempotency_key: "request-1".into(),
            node_id: "node-home".into(),
            client_id: "desktop-work".into(),
            execution_id: Some("execution-1".into()),
            required_scope: NodeScope::Prompt,
            issued_at: 1_000,
            expires_at: 61_000,
            payload: serde_json::json!({ "prompt": "Continue" }),
        };
        assert_eq!(validate_command(&command, 2_000), Ok(()));
        command.idempotency_key.clear();
        assert_eq!(
            validate_command(&command, 2_000),
            Err(ProtocolViolation::InvalidCommand)
        );
    }

    #[test]
    fn event_cursor_rejects_replay_and_requires_snapshot_after_a_gap() {
        let mut cursor = EventCursor::new(40, 8);
        assert_eq!(cursor.accept(&event(41, "event-41")), Ok(()));
        assert_eq!(
            cursor.accept(&event(41, "event-41-copy")),
            Err(ProtocolViolation::Replay)
        );
        assert_eq!(
            cursor.accept(&event(43, "event-43")),
            Err(ProtocolViolation::SequenceGap {
                expected: 42,
                received: 43,
            })
        );
        cursor.reset_from_snapshot(43);
        assert_eq!(cursor.sequence(), 43);
        assert_eq!(cursor.accept(&event(44, "event-44")), Ok(()));
    }

    #[test]
    fn artifacts_are_explicit_bounded_and_integrity_addressed() {
        let mut artifact = ArtifactManifest {
            artifact_id: "artifact-1".into(),
            name: "report.pdf".into(),
            content_type: "application/pdf".into(),
            size_bytes: 2 * 1024 * 1024,
            sha256: "f".repeat(64),
            chunk_bytes: ARTIFACT_CHUNK_BYTES,
        };
        assert_eq!(validate_artifact(&artifact), Ok(()));
        artifact.size_bytes = MAX_ARTIFACT_BYTES + 1;
        assert_eq!(
            validate_artifact(&artifact),
            Err(ProtocolViolation::ArtifactTooLarge)
        );
        artifact.size_bytes = 1;
        artifact.name = "../secret.txt".into();
        assert_eq!(
            validate_artifact(&artifact),
            Err(ProtocolViolation::InvalidCommand)
        );
    }

    #[test]
    fn event_payload_limits_follow_the_selected_channel() {
        let mut bounded = event(1, "event-1");
        bounded.payload = Value::String("x".repeat(MAX_STREAM_PAYLOAD_BYTES));
        assert_eq!(
            validate_event(&bounded),
            Err(ProtocolViolation::PayloadTooLarge)
        );

        bounded.channel = MessageChannel::State;
        assert_eq!(validate_event(&bounded), Ok(()));
    }
}
