//! A paired device proves its identity first, then TOTP opens a short-lived, in-memory access
//! session. The Node still checks the device's scopes and owner switches on every command.

use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::{node_pairing::PairingLock, node_totp::{self, OtpError, TotpVerifier}, node_totp_secret};

const POLICY_FILE: &str = "node-otp.json";
const ENROLLMENT_LIFETIME: Duration = Duration::from_secs(300);
const SESSION_IDLE: Duration = Duration::from_secs(30);
pub(crate) const OTP_REQUIRED: &str = "lume_node_otp_required";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Policy {
    version: u8,
    key_id: String,
    last_accepted_step: u64,
}

struct Enrollment {
    key_id: String,
    secret: Zeroizing<Vec<u8>>,
    verifier: TotpVerifier,
    created: Instant,
}

#[derive(Default)]
struct Runtime {
    epoch: String,
    verifier: TotpVerifier,
    sessions: HashMap<String, Grant>,
}

struct Grant {
    digest: [u8; 32],
    touched: Instant,
    anchored: bool,
}

struct ClientGrant {
    token: Zeroizing<String>,
    touched: Instant,
}

static ENROLLMENTS: OnceLock<Mutex<HashMap<PathBuf, Enrollment>>> = OnceLock::new();
static RUNTIMES: OnceLock<Mutex<HashMap<PathBuf, Runtime>>> = OnceLock::new();
static CLIENTS: OnceLock<Mutex<HashMap<String, ClientGrant>>> = OnceLock::new();

fn wall_time_ms() -> Result<u64, String> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "The Node clock is invalid".to_string())?.as_millis();
    u64::try_from(millis).map_err(|_| "The Node clock is invalid".into())
}

fn random_token() -> Result<Zeroizing<String>, String> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    getrandom::getrandom(&mut *bytes).map_err(|_| "Could not generate a secure session".to_string())?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(&*bytes)))
}

fn read_policy(directory: &Path) -> Result<Option<Policy>, String> {
    let bytes = match fs::read(directory.join(POLICY_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Could not read the Node authenticator configuration".into()),
    };
    if bytes.len() > 4096 { return Err("The Node authenticator configuration is invalid".into()); }
    let policy: Policy = serde_json::from_slice(&bytes).map_err(|_| "The Node authenticator configuration is invalid".to_string())?;
    if policy.version != 1 || policy.key_id.len() != 43 || !policy.key_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') {
        return Err("The Node authenticator configuration is invalid".into());
    }
    Ok(Some(policy))
}

/// The caller holds the cross-process registry lock. Never remove the old policy before replacing it:
/// readers must not mistake an in-progress update for a disabled authenticator.
fn write_policy(directory: &Path, policy: &Policy) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let suffix = random_token()?;
    let temporary = directory.join(format!("{POLICY_FILE}.tmp-{}-{}", process::id(), &suffix[..8]));
    let bytes = serde_json::to_vec(policy).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut file = options.open(&temporary).map_err(|error| error.to_string())?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, directory.join(POLICY_FILE)) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

pub(crate) fn enabled(directory: &Path) -> Result<bool, String> {
    read_policy(directory).map(|policy| policy.is_some())
}

/// This URI is returned only to the local Node owner for a QR code. It is never sent to the Relay.
pub(crate) fn begin_enrollment(directory: &Path) -> Result<String, String> {
    if enabled(directory)? { return Err("This Node already has an authenticator".into()); }
    let key_id = random_token()?.to_string();
    // Fail before displaying a QR code if the OS credential store cannot be reached.
    if node_totp_secret::load(directory, &key_id)?.is_some() { return Err("Could not create an authenticator".into()); }
    let mut secret = Zeroizing::new(vec![0u8; 20]);
    getrandom::getrandom(&mut secret).map_err(|_| "Could not generate an authenticator secret".to_string())?;
    let encoded = Zeroizing::new(node_totp::base32(&secret));
    let uri = format!("otpauth://totp/Lume:{}?secret={}&issuer=Lume&algorithm=SHA1&digits=6&period=30", &key_id[..8], encoded.as_str());
    let mut pending = ENROLLMENTS.get_or_init(|| Mutex::new(HashMap::new())).lock().map_err(|_| "Authenticator setup is unavailable")?;
    pending.insert(directory.to_path_buf(), Enrollment { key_id, secret, verifier: TotpVerifier::default(), created: Instant::now() });
    Ok(uri)
}

pub(crate) fn cancel_enrollment(directory: &Path) {
    if let Ok(mut pending) = ENROLLMENTS.get_or_init(|| Mutex::new(HashMap::new())).lock() { pending.remove(directory); }
}

pub(crate) fn finish_enrollment(directory: &Path, code: &str) -> Result<(), String> {
    let mut pending = ENROLLMENTS.get_or_init(|| Mutex::new(HashMap::new())).lock().map_err(|_| "Authenticator setup is unavailable")?;
    let enrollment = pending.get_mut(directory).ok_or("Start authenticator setup again")?;
    if enrollment.created.elapsed() >= ENROLLMENT_LIFETIME { pending.remove(directory); return Err("Authenticator setup expired; start again".into()); }
    let step = enrollment.verifier.verify(&enrollment.secret, "local-owner", code, wall_time_ms()?).map_err(otp_error)?;
    let _lock = PairingLock::acquire(directory)?;
    if enabled(directory)? { return Err("This Node already has an authenticator".into()); }
    node_totp_secret::store(directory, &enrollment.key_id, &enrollment.secret)?;
    let policy = Policy { version: 1, key_id: enrollment.key_id.clone(), last_accepted_step: step };
    if let Err(error) = write_policy(directory, &policy) {
        let _ = node_totp_secret::remove(directory, &enrollment.key_id);
        return Err(error);
    }
    pending.remove(directory);
    Ok(())
}

/// Only the local Node owner can disable the authenticator. Existing session grants are discarded.
pub(crate) fn disable(directory: &Path) -> Result<(), String> {
    let _lock = PairingLock::acquire(directory)?;
    let Some(policy) = read_policy(directory)? else { return Ok(()); };
    node_totp_secret::remove(directory, &policy.key_id)?;
    fs::remove_file(directory.join(POLICY_FILE)).map_err(|_| "Could not disable the Node authenticator".to_string())?;
    if let Ok(mut runtimes) = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new())).lock() { runtimes.remove(directory); }
    Ok(())
}

fn otp_error(error: OtpError) -> String {
    match error {
        OtpError::Invalid => "Invalid authenticator code".into(),
        OtpError::Replayed => "This code was already used; wait for a new one".into(),
        OtpError::RateLimited => "Too many attempts; wait a minute and try again".into(),
    }
}

fn runtime_for<'a>(runtimes: &'a mut HashMap<PathBuf, Runtime>, directory: &Path, policy: &Policy) -> &'a mut Runtime {
    let runtime = runtimes.entry(directory.to_path_buf()).or_default();
    if runtime.epoch != policy.key_id {
        runtime.epoch.clone_from(&policy.key_id);
        runtime.verifier = TotpVerifier::with_last_accepted_step(Some(policy.last_accepted_step));
        runtime.sessions.clear();
    }
    runtime
}

/// `fingerprint` must already be verified against the paired-device registry by the caller.
pub(crate) fn authenticate(directory: &Path, fingerprint: &str, code: &str) -> Result<Zeroizing<String>, String> {
    let _lock = PairingLock::acquire(directory)?;
    let policy = read_policy(directory)?.ok_or("The Node does not require an authenticator")?;
    let secret = node_totp_secret::load(directory, &policy.key_id)?.ok_or("The Node authenticator is unavailable; reset it locally")?;
    let token = random_token()?;
    let mut runtimes = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new())).lock().map_err(|_| "Node authentication is unavailable")?;
    let runtime = runtime_for(&mut runtimes, directory, &policy);
    let step = runtime.verifier.verify(&secret, fingerprint, code, wall_time_ms()?).map_err(otp_error)?;
    let updated = Policy { last_accepted_step: step, ..policy };
    write_policy(directory, &updated)?; // Durable single-use check before granting access.
    runtime.sessions.insert(fingerprint.to_string(), Grant { digest: Sha256::digest(token.as_bytes()).into(), touched: Instant::now(), anchored: false });
    Ok(token)
}

/// A grant is not usable until it is bound to a live transport. The transport must call
/// `disconnect_if_token` when it closes, and heartbeats keep the grant alive while idle.
pub(crate) fn activate_anchor(directory: &Path, fingerprint: &str, token: &str) -> Result<(), String> {
    let policy = read_policy(directory)?.ok_or(OTP_REQUIRED)?;
    let mut runtimes = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new())).lock().map_err(|_| "Node authentication is unavailable")?;
    let runtime = runtime_for(&mut runtimes, directory, &policy);
    let grant = runtime.sessions.get_mut(fingerprint).ok_or(OTP_REQUIRED)?;
    let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    if token.len() != 43 || !bool::from(digest.ct_eq(&grant.digest)) || grant.touched.elapsed() >= SESSION_IDLE { return Err(OTP_REQUIRED.into()); }
    grant.anchored = true;
    grant.touched = Instant::now();
    Ok(())
}

/// Returns the same required-authentication error for an absent, expired, or mismatched grant.
pub(crate) fn authorize(directory: &Path, fingerprint: &str, token: Option<&str>) -> Result<(), String> {
    let Some(policy) = read_policy(directory)? else { return Ok(()); };
    let mut runtimes = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new())).lock().map_err(|_| "Node authentication is unavailable")?;
    let runtime = runtime_for(&mut runtimes, directory, &policy);
    let Some(grant) = runtime.sessions.get_mut(fingerprint) else { return Err(OTP_REQUIRED.into()); };
    let valid = token.filter(|token| token.len() == 43).map(|token| <[u8; 32]>::from(Sha256::digest(token.as_bytes())))
        .is_some_and(|digest| bool::from(digest.ct_eq(&grant.digest)));
    if grant.touched.elapsed() >= SESSION_IDLE {
        runtime.sessions.remove(fingerprint);
        return Err(OTP_REQUIRED.into());
    }
    if !valid || !grant.anchored { return Err(OTP_REQUIRED.into()); }
    grant.touched = Instant::now();
    Ok(())
}

pub(crate) fn disconnect(directory: &Path, fingerprint: &str) {
    if let Ok(mut runtimes) = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        if let Some(runtime) = runtimes.get_mut(directory) { runtime.sessions.remove(fingerprint); }
    }
}

/// Closing an older connection cannot revoke a newer login from the same device.
pub(crate) fn disconnect_if_token(directory: &Path, fingerprint: &str, token: &str) {
    if let Ok(mut runtimes) = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        if let Some(runtime) = runtimes.get_mut(directory) {
            let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
            if runtime.sessions.get(fingerprint).is_some_and(|grant| bool::from(digest.ct_eq(&grant.digest))) {
                runtime.sessions.remove(fingerprint);
            }
        }
    }
}

/// Client grants are process-only and bound to the pinned Node fingerprint; they are not persisted.
pub(crate) fn remember_client(node_fingerprint: &str, token: Zeroizing<String>) {
    if let Ok(mut clients) = CLIENTS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        clients.insert(node_fingerprint.into(), ClientGrant { token, touched: Instant::now() });
    }
}

pub(crate) fn client_token(node_fingerprint: &str) -> Option<Zeroizing<String>> {
    let mut clients = CLIENTS.get_or_init(|| Mutex::new(HashMap::new())).lock().ok()?;
    let expired = clients.get(node_fingerprint).is_some_and(|grant| grant.touched.elapsed() >= SESSION_IDLE);
    if expired { clients.remove(node_fingerprint); }
    clients.get(node_fingerprint).map(|grant| Zeroizing::new(grant.token.to_string()))
}

pub(crate) fn touch_client_if_matches(node_fingerprint: &str, expected: Option<&str>) {
    if let Ok(mut clients) = CLIENTS.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        if let (Some(client), Some(expected)) = (clients.get_mut(node_fingerprint), expected) {
            if bool::from(client.token.as_bytes().ct_eq(expected.as_bytes())) { client.touched = Instant::now(); }
        }
    }
}

pub(crate) fn forget_client_if_matches(node_fingerprint: &str, expected: Option<&str>) -> bool {
    if let (Ok(mut clients), Some(expected)) = (CLIENTS.get_or_init(|| Mutex::new(HashMap::new())).lock(), expected) {
        if clients.get(node_fingerprint).is_some_and(|client| bool::from(client.token.as_bytes().ct_eq(expected.as_bytes()))) {
            clients.remove(node_fingerprint);
            return true;
        }
    }
    false
}

pub(crate) fn forget_client(node_fingerprint: &str) {
    if let Ok(mut clients) = CLIENTS.get_or_init(|| Mutex::new(HashMap::new())).lock() { clients.remove(node_fingerprint); }
}

#[cfg(test)]
pub(crate) fn prepare_test_authenticator(directory: &Path) -> Result<String, String> {
    let _uri = begin_enrollment(directory)?;
    let code = {
        let pending = ENROLLMENTS.get().ok_or("no test enrollment")?.lock().map_err(|_| "test enrollment lock")?;
        let secret = &pending.get(directory).ok_or("no test secret")?.secret;
        node_totp::test_code(secret, wall_time_ms()?)
    };
    finish_enrollment(directory, &code)?;
    let mut policy = read_policy(directory)?.ok_or("no test policy")?;
    policy.last_accepted_step = policy.last_accepted_step.saturating_sub(1);
    write_policy(directory, &policy)?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        let token = random_token().unwrap();
        std::env::temp_dir().join(format!("lume-node-auth-{}", &token[..12]))
    }

    fn policy(key: &str, step: u64) -> Policy {
        Policy { version: 1, key_id: key.repeat(43), last_accepted_step: step }
    }

    #[test]
    fn grants_are_bound_to_the_device_expire_and_rotate_with_the_policy() {
        let dir = directory();
        assert_eq!(authorize(&dir, "device", None), Ok(())); // Opt-in; existing Nodes remain unchanged.
        let first = policy("a", 12);
        write_policy(&dir, &first).unwrap();
        assert_eq!(authorize(&dir, "device", None).unwrap_err(), OTP_REQUIRED);
        let token = random_token().unwrap();
        {
            let mut all = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
            let runtime = runtime_for(&mut all, &dir, &first);
            runtime.sessions.insert("device".into(), Grant { digest: Sha256::digest(token.as_bytes()).into(), touched: Instant::now(), anchored: false });
        }
        assert_eq!(authorize(&dir, "device", Some(&token)).unwrap_err(), OTP_REQUIRED, "a grant is unusable until a transport is live");
        activate_anchor(&dir, "device", &token).unwrap();
        assert_eq!(authorize(&dir, "other-device", Some(&token)).unwrap_err(), OTP_REQUIRED);
        assert_eq!(authorize(&dir, "device", Some("wrong")).unwrap_err(), OTP_REQUIRED);
        assert_eq!(authorize(&dir, "device", Some(&token)), Ok(()), "a malformed request does not terminate the genuine session");
        {
            let mut all = RUNTIMES.get().unwrap().lock().unwrap();
            all.get_mut(&dir).unwrap().sessions.get_mut("device").unwrap().touched = Instant::now() - SESSION_IDLE;
        }
        assert_eq!(authorize(&dir, "device", Some(&token)).unwrap_err(), OTP_REQUIRED);
        {
            let mut all = RUNTIMES.get().unwrap().lock().unwrap();
            all.get_mut(&dir).unwrap().sessions.insert("device".into(), Grant { digest: Sha256::digest(token.as_bytes()).into(), touched: Instant::now(), anchored: true });
        }
        write_policy(&dir, &policy("b", 13)).unwrap();
        assert_eq!(authorize(&dir, "device", Some(&token)).unwrap_err(), OTP_REQUIRED, "rotation invalidates old grants");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_policy_fails_closed_and_client_grants_are_memory_only() {
        let dir = directory();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(POLICY_FILE), b"not a policy").unwrap();
        assert!(authorize(&dir, "device", None).is_err());
        let fingerprint = format!("node-{}", &random_token().unwrap()[..12]);
        let token = random_token().unwrap();
        remember_client(&fingerprint, Zeroizing::new(token.to_string()));
        assert_eq!(client_token(&fingerprint).as_deref().map(String::as_str), Some(token.as_str()));
        forget_client(&fingerprint);
        assert!(client_token(&fingerprint).is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    #[ignore = "requires a disposable unlocked OS credential store"]
    fn enrollment_and_reconnection_require_a_live_anchor_and_single_use_code() {
        let dir = directory();
        let _uri = begin_enrollment(&dir).unwrap();
        let secret = {
            let pending = ENROLLMENTS.get().unwrap().lock().unwrap();
            Zeroizing::new(pending.get(&dir).unwrap().secret.to_vec())
        };
        let now = wall_time_ms().unwrap();
        let code = node_totp::test_code(&secret, now);
        finish_enrollment(&dir, &code).unwrap();
        assert!(enabled(&dir).unwrap());
        let mut policy = read_policy(&dir).unwrap().unwrap();
        policy.last_accepted_step = policy.last_accepted_step.saturating_sub(1);
        write_policy(&dir, &policy).unwrap(); // Simulate the next step without waiting on a real clock.
        let token = authenticate(&dir, "device", &code).unwrap();
        assert_eq!(authorize(&dir, "device", Some(&token)).unwrap_err(), OTP_REQUIRED);
        activate_anchor(&dir, "device", &token).unwrap();
        assert_eq!(authorize(&dir, "device", Some(&token)), Ok(()));
        assert!(authenticate(&dir, "other-device", &code).is_err(), "one code cannot open two sessions");
        disconnect_if_token(&dir, "device", &token);
        assert_eq!(authorize(&dir, "device", Some(&token)).unwrap_err(), OTP_REQUIRED);
        let config = fs::read_to_string(dir.join(POLICY_FILE)).unwrap();
        assert!(!config.contains(&node_totp::base32(&secret)), "the seed is not stored in the policy");
        disable(&dir).unwrap();
        let _ = fs::remove_dir_all(dir);
    }
}
