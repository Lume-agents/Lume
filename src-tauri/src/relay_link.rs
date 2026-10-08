//! A connection from a Lume Node (or a paired device) to the Lume Relay, speaking protocol v1 of
//! <https://github.com/Lume-agents/lume-relay>. The Relay is blind: payloads are ciphertext produced by
//! [`crate::relay_e2e`], and this module only authenticates, frames and forwards.

use std::{net::TcpStream, time::Duration};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tungstenite::{connect, stream::MaybeTlsStream, Message, WebSocket};

use crate::relay_e2e::{fingerprint, Signer};

const SIGNATURE_DOMAIN: &str = "LUME-RELAY-V1";
const MAX_FRAME: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Node,
    Client,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Role::Node => "node",
            Role::Client => "client",
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum RelayEvent {
    /// A message from a peer; `payload` is the ciphertext exactly as sent.
    Message { from: String, id: String, payload: Vec<u8>, replayed: bool },
    Presence { fingerprint: String, role: String, online: bool },
    Authorized { fingerprint: String },
    PairingOpen { hash: String },
    /// Keep-alive answer or another frame that needs no action.
    Idle,
}

pub struct RelayLink {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    pub room: String,
    pub fingerprint: String,
    pub role: String,
}

/// The value a Node registers (hashed) for a single-use pairing secret.
pub fn pairing_hash(secret: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(secret.as_bytes()))
}

fn websocket_url(base: &str, room: &str) -> Result<String, String> {
    let base = base.trim_end_matches('/');
    let rest = base
        .strip_prefix("https://")
        .map(|rest| format!("wss://{rest}"))
        .or_else(|| base.strip_prefix("http://").map(|rest| format!("ws://{rest}")))
        .or_else(|| (base.starts_with("wss://") || base.starts_with("ws://")).then(|| base.to_string()))
        .ok_or("the Relay address must start with https:// or wss://")?;
    Ok(format!("{rest}/v1/rooms/{room}/connect"))
}

fn set_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, timeout: Option<Duration>) -> Result<(), String> {
    let stream = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::Rustls(stream) => stream.get_mut(),
        _ => return Ok(()),
    };
    stream.set_read_timeout(timeout).map_err(|error| error.to_string())
}

impl RelayLink {
    /// Connects, answers the challenge with a signature and waits for `ready`. `pairing` is the secret of
    /// an offer the Node opened, for a device that is not authorized yet.
    pub fn connect(base_url: &str, room: &str, signer: &dyn Signer, role: Role, pairing: Option<&str>) -> Result<Self, String> {
        let (mut socket, _) = connect(websocket_url(base_url, room)?).map_err(|error| format!("could not reach the Relay: {error}"))?;
        set_timeout(&mut socket, Some(Duration::from_secs(10)))?;
        let challenge = Self::read_json(&mut socket)?;
        if challenge["t"] != "challenge" {
            return Err("the Relay did not send a challenge".into());
        }
        let nonce = challenge["nonce"].as_str().ok_or("the challenge has no nonce")?;
        let message = format!("{SIGNATURE_DOMAIN}\n{room}\n{nonce}\n{}", role.name());
        let mut hello = json!({
            "t": "hello",
            "role": role.name(),
            "publicKey": URL_SAFE_NO_PAD.encode(signer.public_key()),
            "signature": URL_SAFE_NO_PAD.encode(signer.sign(message.as_bytes())),
        });
        if let Some(secret) = pairing {
            hello["pairing"] = Value::String(secret.into());
        }
        socket.send(Message::text(hello.to_string())).map_err(|error| error.to_string())?;
        loop {
            let frame = Self::read_json(&mut socket)?;
            match frame["t"].as_str() {
                Some("ready") => {
                    let granted = frame["role"].as_str().unwrap_or(role.name()).to_string();
                    set_timeout(&mut socket, None)?;
                    return Ok(Self { socket, room: room.into(), fingerprint: fingerprint(&signer.public_key()), role: granted });
                }
                Some("error") => return Err(format!("the Relay refused the connection: {}", frame["code"].as_str().unwrap_or("unknown"))),
                _ => {}
            }
        }
    }

    fn read_json(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Result<Value, String> {
        loop {
            match socket.read().map_err(|error| error.to_string())? {
                Message::Text(text) => return serde_json::from_str(text.as_str()).map_err(|error| error.to_string()),
                Message::Close(frame) => return Err(format!("the Relay closed the connection ({})", frame.map(|frame| frame.reason.to_string()).unwrap_or_default())),
                _ => {}
            }
        }
    }

    /// Bounds how long [`RelayLink::receive`] waits; `None` waits forever.
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<(), String> {
        set_timeout(&mut self.socket, timeout)
    }

    fn send_json(&mut self, value: Value) -> Result<(), String> {
        let text = value.to_string();
        if text.len() > MAX_FRAME {
            return Err("the message is too large for the Relay".into());
        }
        self.socket.send(Message::text(text)).map_err(|error| error.to_string())
    }

    /// Sends ciphertext. A Node names the client fingerprint in `to`; a client always reaches the Node.
    pub fn send(&mut self, to: Option<&str>, id: &str, ciphertext: &[u8]) -> Result<(), String> {
        let mut frame = json!({ "t": "msg", "id": id, "c": URL_SAFE_NO_PAD.encode(ciphertext) });
        if let Some(to) = to {
            frame["to"] = Value::String(to.into());
        }
        self.send_json(frame)
    }

    pub fn authorize(&mut self, public_key: &[u8; 32]) -> Result<(), String> {
        self.send_json(json!({ "t": "authorize", "publicKey": URL_SAFE_NO_PAD.encode(public_key) }))
    }

    pub fn revoke(&mut self, fingerprint: &str) -> Result<(), String> {
        self.send_json(json!({ "t": "revoke", "fingerprint": fingerprint }))
    }

    pub fn open_pairing(&mut self, secret: &str, ttl_seconds: u32) -> Result<(), String> {
        self.send_json(json!({ "t": "open-pairing", "hash": pairing_hash(secret), "ttl": ttl_seconds }))
    }

    /// Keep-alive that the Relay answers without waking its room.
    pub fn heartbeat(&mut self) -> Result<(), String> {
        self.socket.send(Message::text("ping")).map_err(|error| error.to_string())
    }

    /// Waits for the next frame. An elapsed read timeout is reported as `Ok(RelayEvent::Idle)`.
    pub fn receive(&mut self) -> Result<RelayEvent, String> {
        let message = match self.socket.read() {
            Ok(message) => message,
            Err(tungstenite::Error::Io(error)) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Ok(RelayEvent::Idle),
            Err(error) => return Err(error.to_string()),
        };
        let text = match message {
            Message::Text(text) => text,
            Message::Close(frame) => return Err(format!("the Relay closed the connection ({})", frame.map(|frame| frame.reason.to_string()).unwrap_or_default())),
            _ => return Ok(RelayEvent::Idle),
        };
        if text.as_str() == "pong" {
            return Ok(RelayEvent::Idle);
        }
        let frame: Value = serde_json::from_str(text.as_str()).map_err(|error| error.to_string())?;
        let field = |name: &str| frame[name].as_str().unwrap_or_default().to_string();
        Ok(match frame["t"].as_str() {
            Some("msg") => RelayEvent::Message {
                from: field("from"),
                id: field("id"),
                payload: URL_SAFE_NO_PAD.decode(field("c")).map_err(|_| "the Relay sent invalid ciphertext".to_string())?,
                replayed: frame["replayed"].as_bool().unwrap_or(false),
            },
            Some("presence") => RelayEvent::Presence { fingerprint: field("fingerprint"), role: field("role"), online: frame["online"].as_bool().unwrap_or(false) },
            Some("authorized") => RelayEvent::Authorized { fingerprint: field("fingerprint") },
            Some("pairing-open") => RelayEvent::PairingOpen { hash: field("hash") },
            Some("error") => return Err(format!("the Relay reported an error: {}", field("code"))),
            _ => RelayEvent::Idle,
        })
    }

    pub fn close(mut self) {
        let _ = self.socket.close(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{rand::SystemRandom, signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519}};
    use std::net::TcpListener;

    fn identity() -> Ed25519KeyPair {
        Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap().as_ref()).unwrap()
    }

    #[test]
    fn the_pairing_hash_matches_the_relay_protocol() {
        // The Relay's `pairingHash("secret")`, so both sides hash a pairing secret identically.
        assert_eq!(pairing_hash("secret"), "K7gNU3sdo-OL0wNhqoVWhr3g6s1xYv72ol_pe_Unols");
    }

    #[test]
    fn urls_are_converted_to_the_websocket_scheme() {
        assert_eq!(websocket_url("https://relay.lumeagents.dev/", "ab").unwrap(), "wss://relay.lumeagents.dev/v1/rooms/ab/connect");
        assert_eq!(websocket_url("http://127.0.0.1:1", "ab").unwrap(), "ws://127.0.0.1:1/v1/rooms/ab/connect");
        assert!(websocket_url("ftp://x", "ab").is_err());
    }

    /// A stand-in Relay that checks the signed hello exactly as the real one does.
    #[test]
    fn the_hello_is_signed_over_the_challenge_and_messages_flow() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let node = identity();
        let node_public = KeyPair::public_key(&node).as_ref().to_vec();
        let room = fingerprint(&node_public);
        let expected_room = room.clone();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            socket.send(Message::text(json!({ "t": "challenge", "v": 1, "room": expected_room, "nonce": "bm9uY2U" }).to_string())).unwrap();
            let hello: Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            let public = URL_SAFE_NO_PAD.decode(hello["publicKey"].as_str().unwrap()).unwrap();
            let signature = URL_SAFE_NO_PAD.decode(hello["signature"].as_str().unwrap()).unwrap();
            let message = format!("LUME-RELAY-V1\n{expected_room}\nbm9uY2U\nnode");
            UnparsedPublicKey::new(&ED25519, &public).verify(message.as_bytes(), &signature).expect("a valid signature");
            socket.send(Message::text(json!({ "t": "ready", "role": "node", "fingerprint": expected_room, "node": expected_room }).to_string())).unwrap();
            let sent: Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            socket.send(Message::text(json!({ "t": "msg", "id": sent["id"], "from": "peer", "c": sent["c"], "replayed": true }).to_string())).unwrap();
        });
        let mut link = RelayLink::connect(&format!("http://{address}"), &room, &node, Role::Node, None).unwrap();
        assert_eq!((link.role.as_str(), link.fingerprint.as_str()), ("node", room.as_str()));
        link.send(Some("target"), "m1", b"ciphertext").unwrap();
        match link.receive().unwrap() {
            RelayEvent::Message { from, id, payload, replayed } => assert_eq!((from.as_str(), id.as_str(), payload.as_slice(), replayed), ("peer", "m1", &b"ciphertext"[..], true)),
            other => panic!("unexpected {other:?}"),
        }
        server.join().unwrap();
    }

    /// Runs the full path against a real Relay: `LUME_RELAY_URL=http://127.0.0.1:8788 cargo test relay_link -- --ignored`.
    #[test]
    #[ignore = "needs a running Lume Relay"]
    fn a_node_and_a_paired_device_talk_end_to_end_through_a_real_relay() {
        use crate::relay_e2e::{initiate, respond};
        let url = std::env::var("LUME_RELAY_URL").expect("LUME_RELAY_URL");
        let (node, device) = (identity(), identity());
        let room = fingerprint(KeyPair::public_key(&node).as_ref());
        let mut node_link = RelayLink::connect(&url, &room, &node, Role::Node, None).unwrap();
        let offer = "a-single-use-pairing-secret-0123456789";
        node_link.open_pairing(offer, 60).unwrap();
        assert!(matches!(node_link.receive().unwrap(), RelayEvent::PairingOpen { .. }));
        let mut device_link = RelayLink::connect(&url, &room, &device, Role::Client, Some(offer)).unwrap();
        assert_eq!(device_link.role, "pairing");
        let device_public: [u8; 32] = Signer::public_key(&device);
        node_link.authorize(&device_public).unwrap();
        // Handshake: the device speaks first; the Node answers; both derive the same session.
        let (state, hello) = initiate(&device).unwrap();
        device_link.send(None, "hs1", &hello).unwrap();
        let request = loop {
            if let RelayEvent::Message { payload, .. } = node_link.receive().unwrap() { break payload; }
        };
        let (reply, mut node_session) = respond(&node, &device_public, &request).unwrap();
        node_link.send(Some(&fingerprint(&device_public)), "hs2", &reply).unwrap();
        let answer = loop {
            if let RelayEvent::Message { payload, .. } = device_link.receive().unwrap() { break payload; }
        };
        let mut device_session = state.finish(&answer, &Signer::public_key(&node)).unwrap();
        device_link.send(None, "d1", &device_session.seal(b"approve").unwrap()).unwrap();
        let frame = loop {
            if let RelayEvent::Message { payload, .. } = node_link.receive().unwrap() { break payload; }
        };
        assert_eq!(node_session.open(&frame).unwrap(), b"approve");
        node_link.close();
        device_link.close();
    }
}
