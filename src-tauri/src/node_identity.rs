use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const PRIVATE_KEY_FILE: &str = "node-identity.pk8";
const PUBLIC_IDENTITY_FILE: &str = "node-identity.json";
const ALGORITHM: &str = "Ed25519";
const SIGNATURE_DOMAIN: &[u8] = b"lume-device-statement-v1\0";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicDeviceIdentity {
    pub algorithm: String,
    pub public_key: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedDeviceStatement {
    pub identity: PublicDeviceIdentity,
    pub payload: Value,
    pub signed_payload: String,
    pub signature: String,
}

pub struct NodeIdentity {
    key_pair: Ed25519KeyPair,
    public: PublicDeviceIdentity,
}

impl NodeIdentity {
    pub fn load_or_create(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        restrict_directory(directory)?;
        let private_path = directory.join(PRIVATE_KEY_FILE);
        if private_path.exists() {
            return Self::load(&private_path, &directory.join(PUBLIC_IDENTITY_FILE));
        }

        let generated = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| "could not generate the Node identity".to_string())?;
        match create_private_file(&private_path, generated.as_ref()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
        Self::load(&private_path, &directory.join(PUBLIC_IDENTITY_FILE))
    }

    fn load(private_path: &Path, public_path: &Path) -> Result<Self, String> {
        let private = fs::read(private_path)
            .map_err(|error| format!("could not read the Node identity: {error}"))?;
        let key_pair = Ed25519KeyPair::from_pkcs8(&private)
            .map_err(|_| "the persisted Node identity is invalid".to_string())?;
        let public_bytes = key_pair.public_key().as_ref();
        let public = PublicDeviceIdentity {
            algorithm: ALGORITHM.into(),
            public_key: URL_SAFE_NO_PAD.encode(public_bytes),
            fingerprint: fingerprint(public_bytes),
        };

        if public_path.exists() {
            let persisted = fs::read(public_path).map_err(|error| error.to_string())?;
            let persisted = serde_json::from_slice::<PublicDeviceIdentity>(&persisted)
                .map_err(|_| "the public Node identity is invalid".to_string())?;
            if persisted != public {
                return Err("the public Node identity does not match its private key".into());
            }
        } else {
            write_public_identity(public_path, &public)?;
        }

        Ok(Self { key_pair, public })
    }

    pub fn public(&self) -> &PublicDeviceIdentity {
        &self.public
    }

    pub fn sign(&self, payload: Value) -> Result<SignedDeviceStatement, String> {
        let payload_bytes = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
        let message = signature_message(&payload_bytes);
        Ok(SignedDeviceStatement {
            identity: self.public.clone(),
            payload,
            signed_payload: URL_SAFE_NO_PAD.encode(payload_bytes),
            signature: URL_SAFE_NO_PAD.encode(self.key_pair.sign(&message).as_ref()),
        })
    }
}

impl crate::relay_e2e::Signer for NodeIdentity {
    fn public_key(&self) -> [u8; 32] {
        let mut key = [0u8; 32];
        key.copy_from_slice(KeyPair::public_key(&self.key_pair).as_ref());
        key
    }

    fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.key_pair.sign(message).as_ref().to_vec()
    }
}

pub fn verify(statement: &SignedDeviceStatement) -> Result<(), String> {
    if statement.identity.algorithm != ALGORITHM {
        return Err("unsupported Node identity algorithm".into());
    }
    let public_key = URL_SAFE_NO_PAD
        .decode(&statement.identity.public_key)
        .map_err(|_| "invalid Node public key".to_string())?;
    if public_key.len() != 32 || fingerprint(&public_key) != statement.identity.fingerprint {
        return Err("invalid Node identity fingerprint".into());
    }
    let signature = URL_SAFE_NO_PAD
        .decode(&statement.signature)
        .map_err(|_| "invalid Node signature".to_string())?;
    let signed_payload = URL_SAFE_NO_PAD
        .decode(&statement.signed_payload)
        .map_err(|_| "invalid signed Node payload".to_string())?;
    let parsed_payload = serde_json::from_slice::<Value>(&signed_payload)
        .map_err(|_| "invalid signed Node payload".to_string())?;
    if parsed_payload != statement.payload {
        return Err("the visible Node payload does not match the signed payload".into());
    }
    let message = signature_message(&signed_payload);
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&message, &signature)
        .map_err(|_| "Node statement signature verification failed".to_string())
}

fn signature_message(payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(SIGNATURE_DOMAIN.len() + payload.len());
    message.extend_from_slice(SIGNATURE_DOMAIN);
    message.extend_from_slice(payload);
    message
}

fn fingerprint(public_key: &[u8]) -> String {
    Sha256::digest(public_key)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn write_public_identity(path: &Path, identity: &PublicDeviceIdentity) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(identity).map_err(|error| error.to_string())?;
    fs::write(path, payload).map_err(|error| error.to_string())?;
    restrict_file(path)
}

#[cfg(unix)]
fn create_private_file(path: &Path, value: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(value)?;
    file.sync_all()
}

#[cfg(not(unix))]
fn create_private_file(path: &Path, value: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
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
    use std::{env, path::PathBuf, process};

    fn test_directory(name: &str) -> PathBuf {
        let mut random = [0_u8; 8];
        getrandom::getrandom(&mut random).expect("random test directory");
        env::temp_dir().join(format!(
            "lume-node-identity-{name}-{}-{}",
            process::id(),
            random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ))
    }

    #[test]
    fn identity_is_persistent_and_has_a_sha256_fingerprint() {
        let directory = test_directory("persistent");
        let first = NodeIdentity::load_or_create(&directory).expect("first identity");
        let first_public = first.public().clone();
        drop(first);
        let second = NodeIdentity::load_or_create(&directory).expect("second identity");
        assert_eq!(&first_public, second.public());
        assert_eq!(second.public().fingerprint.len(), 64);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn signed_statements_reject_payload_tampering() {
        let directory = test_directory("signature");
        let identity = NodeIdentity::load_or_create(&directory).expect("identity");
        let mut statement = identity
            .sign(serde_json::json!({ "nodeId": "node-home", "sequence": 1 }))
            .expect("signed statement");
        assert_eq!(verify(&statement), Ok(()));
        statement.payload["sequence"] = serde_json::json!(2);
        assert_eq!(
            verify(&statement).err().as_deref(),
            Some("the visible Node payload does not match the signed payload")
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn corrupted_private_identity_fails_closed() {
        let directory = test_directory("corrupt");
        fs::create_dir_all(&directory).expect("directory");
        create_private_file(&directory.join(PRIVATE_KEY_FILE), b"not-a-private-key")
            .expect("private file");
        assert_eq!(
            NodeIdentity::load_or_create(&directory).err().as_deref(),
            Some("the persisted Node identity is invalid")
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn private_identity_is_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let directory = test_directory("permissions");
        NodeIdentity::load_or_create(&directory).expect("identity");
        let mode = fs::metadata(directory.join(PRIVATE_KEY_FILE))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(directory);
    }
}
