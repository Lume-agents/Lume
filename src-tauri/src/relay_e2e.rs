//! End-to-end encryption between a Lume Node and a paired device, carried through the (blind) Relay.
//!
//! Each side signs an ephemeral X25519 key with its long-term Ed25519 identity, so a Relay or any other
//! party in the middle can neither read nor impersonate. Traffic keys come from HKDF over the shared secret
//! and the handshake transcript, one key per direction, and every frame carries a counter the receiver
//! requires to increase (replay and reordering protection).

use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305},
    agreement::{agree_ephemeral, EphemeralPrivateKey, UnparsedPublicKey as AgreementKey, X25519},
    hkdf::{Salt, HKDF_SHA256},
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519},
};
use sha2::{Digest, Sha256};

const HS1: u8 = 1;
const HS2: u8 = 2;
const DATA: u8 = 3;
const DOMAIN: &[u8] = b"LUME-E2E-V1";
const EPHEMERAL: usize = 32;
const SIGNATURE: usize = 64;
const TAG: usize = 16;
/// Frames travel in a Relay message of at most 256 KiB; leave room for the envelope.
pub const MAX_PLAINTEXT: usize = 200 * 1024;

/// What the handshake needs from a long-term identity.
pub trait Signer {
    fn public_key(&self) -> [u8; 32];
    fn sign(&self, message: &[u8]) -> Vec<u8>;
}

impl Signer for Ed25519KeyPair {
    fn public_key(&self) -> [u8; 32] {
        let mut key = [0u8; 32];
        key.copy_from_slice(KeyPair::public_key(self).as_ref());
        key
    }
    fn sign(&self, message: &[u8]) -> Vec<u8> {
        Ed25519KeyPair::sign(self, message).as_ref().to_vec()
    }
}

pub fn fingerprint(public_key: &[u8]) -> String {
    Sha256::digest(public_key).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn transcript(kind: u8, initiator: &[u8], responder: &[u8], first: &[u8], second: &[u8]) -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(DOMAIN);
    message.push(kind);
    for part in [initiator, responder, first, second] {
        message.extend_from_slice(&(part.len() as u32).to_be_bytes());
        message.extend_from_slice(part);
    }
    message
}

fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), String> {
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(message, signature)
        .map_err(|_| "the handshake signature is invalid".to_string())
}

/// The party that starts the conversation (a paired device).
pub struct Initiator {
    private: EphemeralPrivateKey,
    ephemeral: Vec<u8>,
    local_public: [u8; 32],
}

/// First handshake frame, plus the state needed to finish it.
pub fn initiate(local: &dyn Signer) -> Result<(Initiator, Vec<u8>), String> {
    let private = EphemeralPrivateKey::generate(&X25519, &SystemRandom::new()).map_err(|_| "could not generate a key".to_string())?;
    let ephemeral = private.compute_public_key().map_err(|_| "could not derive a key".to_string())?.as_ref().to_vec();
    let local_public = local.public_key();
    let signature = local.sign(&transcript(HS1, &local_public, &[], &ephemeral, &[]));
    let mut frame = vec![HS1];
    frame.extend_from_slice(&ephemeral);
    frame.extend_from_slice(&signature);
    Ok((Initiator { private, ephemeral, local_public }, frame))
}

impl Initiator {
    /// Completes the handshake with the responder's reply, whose identity must be the expected Node.
    pub fn finish(self, reply: &[u8], remote_public: &[u8; 32]) -> Result<Session, String> {
        if reply.len() != 1 + EPHEMERAL + SIGNATURE || reply[0] != HS2 {
            return Err("unexpected handshake reply".into());
        }
        let remote_ephemeral = &reply[1..1 + EPHEMERAL];
        let signature = &reply[1 + EPHEMERAL..];
        verify(remote_public, &transcript(HS2, &self.local_public, remote_public, &self.ephemeral, remote_ephemeral), signature)?;
        let secret = agree_ephemeral(self.private, &AgreementKey::new(&X25519, remote_ephemeral), |secret| secret.to_vec())
            .map_err(|_| "key agreement failed".to_string())?;
        Session::new(&secret, [&self.local_public, remote_public, &self.ephemeral, remote_ephemeral], true)
    }
}

/// The party that answers (the Node). `remote_public` is the paired device's authorized key.
pub fn respond(local: &dyn Signer, remote_public: &[u8; 32], request: &[u8]) -> Result<(Vec<u8>, Session), String> {
    if request.len() != 1 + EPHEMERAL + SIGNATURE || request[0] != HS1 {
        return Err("unexpected handshake request".into());
    }
    let remote_ephemeral = &request[1..1 + EPHEMERAL];
    verify(remote_public, &transcript(HS1, remote_public, &[], remote_ephemeral, &[]), &request[1 + EPHEMERAL..])?;
    let private = EphemeralPrivateKey::generate(&X25519, &SystemRandom::new()).map_err(|_| "could not generate a key".to_string())?;
    let ephemeral = private.compute_public_key().map_err(|_| "could not derive a key".to_string())?.as_ref().to_vec();
    let local_public = local.public_key();
    let signature = local.sign(&transcript(HS2, remote_public, &local_public, remote_ephemeral, &ephemeral));
    let secret = agree_ephemeral(private, &AgreementKey::new(&X25519, remote_ephemeral), |secret| secret.to_vec())
        .map_err(|_| "key agreement failed".to_string())?;
    let session = Session::new(&secret, [remote_public, &local_public, remote_ephemeral, &ephemeral], false)?;
    let mut reply = vec![HS2];
    reply.extend_from_slice(&ephemeral);
    reply.extend_from_slice(&signature);
    Ok((reply, session))
}

/// Authenticated encryption in both directions.
pub struct Session {
    send: LessSafeKey,
    receive: LessSafeKey,
    next_send: u64,
    last_received: Option<u64>,
    aad: Vec<u8>,
}

impl Session {
    fn new(secret: &[u8], parties: [&[u8]; 4], is_initiator: bool) -> Result<Self, String> {
        let [initiator, responder, first, second] = parties;
        let salt = Sha256::digest(transcript(0, initiator, responder, first, second));
        let mut keys = [0u8; 64];
        struct Length(usize);
        impl ring::hkdf::KeyType for Length {
            fn len(&self) -> usize {
                self.0
            }
        }
        Salt::new(HKDF_SHA256, &salt)
            .extract(secret)
            .expand(&[DOMAIN, b" keys"], Length(64))
            .and_then(|okm| okm.fill(&mut keys))
            .map_err(|_| "key derivation failed".to_string())?;
        let key = |bytes: &[u8]| UnboundKey::new(&CHACHA20_POLY1305, bytes).map(LessSafeKey::new).map_err(|_| "invalid key".to_string());
        let (to_responder, to_initiator) = (key(&keys[..32])?, key(&keys[32..])?);
        let mut aad = DOMAIN.to_vec();
        aad.extend_from_slice(&salt);
        let (send, receive) = if is_initiator { (to_responder, to_initiator) } else { (to_initiator, to_responder) };
        Ok(Self { send, receive, next_send: 0, last_received: None, aad })
    }

    fn nonce(counter: u64) -> Nonce {
        let mut bytes = [0u8; 12];
        bytes[4..].copy_from_slice(&counter.to_be_bytes());
        Nonce::assume_unique_for_key(bytes)
    }

    /// Encrypts one message into a frame: type, counter, ciphertext and tag.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        if plaintext.len() > MAX_PLAINTEXT {
            return Err("the message is too large".into());
        }
        let counter = self.next_send;
        self.next_send = counter.checked_add(1).ok_or("the session counter is exhausted")?;
        let mut buffer = plaintext.to_vec();
        self.send
            .seal_in_place_append_tag(Self::nonce(counter), Aad::from(&self.aad), &mut buffer)
            .map_err(|_| "encryption failed".to_string())?;
        let mut frame = vec![DATA];
        frame.extend_from_slice(&counter.to_be_bytes());
        frame.extend_from_slice(&buffer);
        Ok(frame)
    }

    /// Decrypts a frame, rejecting forgeries, replays and out-of-order counters.
    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>, String> {
        if frame.len() < 1 + 8 + TAG || frame[0] != DATA {
            return Err("not a data frame".into());
        }
        let counter = u64::from_be_bytes(frame[1..9].try_into().map_err(|_| "bad counter".to_string())?);
        if self.last_received.is_some_and(|last| counter <= last) {
            return Err("replayed or reordered frame".into());
        }
        let mut buffer = frame[9..].to_vec();
        let plain = self
            .receive
            .open_in_place(Self::nonce(counter), Aad::from(&self.aad), &mut buffer)
            .map_err(|_| "the frame could not be authenticated".to_string())?
            .to_vec();
        self.last_received = Some(counter);
        Ok(plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Ed25519KeyPair {
        Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap().as_ref()).unwrap()
    }

    fn pair() -> (Ed25519KeyPair, Ed25519KeyPair, Session, Session) {
        let (device, node) = (identity(), identity());
        let (state, hello) = initiate(&device).unwrap();
        let (reply, node_session) = respond(&node, &Signer::public_key(&device), &hello).unwrap();
        let device_session = state.finish(&reply, &Signer::public_key(&node)).unwrap();
        (device, node, device_session, node_session)
    }

    #[test]
    fn both_sides_read_what_the_other_sends() {
        let (_, _, mut device, mut node) = pair();
        assert_eq!(node.open(&device.seal(b"approve the build").unwrap()).unwrap(), b"approve the build");
        assert_eq!(device.open(&node.seal(b"{\"status\":\"running\"}").unwrap()).unwrap(), b"{\"status\":\"running\"}");
        assert_eq!(node.open(&device.seal(b"again").unwrap()).unwrap(), b"again");
    }

    #[test]
    fn the_wire_never_contains_the_plaintext() {
        let (_, _, mut device, _) = pair();
        let frame = device.seal(b"super secret prompt").unwrap();
        assert!(!frame.windows(5).any(|window| window == b"super"));
    }

    #[test]
    fn replayed_tampered_and_misdirected_frames_are_rejected() {
        let (_, _, mut device, mut node) = pair();
        let frame = device.seal(b"one").unwrap();
        assert!(node.open(&frame).is_ok());
        assert!(node.open(&frame).is_err(), "replay");
        let mut tampered = device.seal(b"two").unwrap();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(node.open(&tampered).is_err(), "tampering");
        // A frame sent by the node cannot be fed back to the node as if the device had sent it.
        let from_node = node.seal(b"echo").unwrap();
        assert!(node.open(&from_node).is_err(), "reflection");
        let (_, _, mut other_device, _) = pair();
        assert!(node.open(&other_device.seal(b"three").unwrap()).is_err(), "another session");
    }

    #[test]
    fn a_handshake_signed_by_the_wrong_key_fails() {
        let (device, node, intruder) = (identity(), identity(), identity());
        let (state, hello) = initiate(&intruder).unwrap();
        assert!(respond(&node, &Signer::public_key(&device), &hello).is_err(), "an unauthorized device");
        let (state2, hello2) = initiate(&device).unwrap();
        let (reply, _) = respond(&node, &Signer::public_key(&device), &hello2).unwrap();
        assert!(state2.finish(&reply, &Signer::public_key(&intruder)).is_err(), "an impostor Node");
        drop(state);
    }

    #[test]
    fn oversized_messages_are_refused() {
        let (_, _, mut device, _) = pair();
        assert!(device.seal(&vec![0u8; MAX_PLAINTEXT + 1]).is_err());
    }
}
