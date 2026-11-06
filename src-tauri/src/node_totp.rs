//! TOTP verification for Node reconnections. This module does not itself authorize any transport.
//! Callers must authenticate the paired device, protect the code in transit, and serialize calls
//! to the verifier across every transport before treating a successful result as authorization.

use std::collections::HashMap;

use ring::hmac;
use subtle::ConstantTimeEq;

const STEP_SECONDS: u64 = 30;
const DIGITS: usize = 6;
const ATTEMPT_WINDOW_MS: u64 = 60_000;
const DEVICE_ATTEMPTS: usize = 5;
const GLOBAL_ATTEMPTS: usize = 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OtpError {
    Invalid,
    Replayed,
    RateLimited,
}

/// Verifies all paired devices against the same authenticator. The accepted time step is shared
/// across devices so that a code cannot be used to authorize a second connection.
#[derive(Default)]
pub(crate) struct TotpVerifier {
    last_accepted_step: Option<u64>,
    failures: HashMap<String, Vec<u64>>,
    global_failures: Vec<u64>,
}

impl TotpVerifier {
    pub(crate) fn with_last_accepted_step(step: Option<u64>) -> Self {
        Self { last_accepted_step: step, ..Self::default() }
    }

    /// `device_id` must already be authenticated, and `now_ms` comes from the Node's clock.
    pub(crate) fn verify(&mut self, secret: &[u8], device_id: &str, candidate: &str, now_ms: u64) -> Result<u64, OtpError> {
        self.global_failures.retain(|at| now_ms.saturating_sub(*at) < ATTEMPT_WINDOW_MS);
        self.failures.retain(|_, failures| {
            failures.retain(|at| now_ms.saturating_sub(*at) < ATTEMPT_WINDOW_MS);
            !failures.is_empty()
        });
        if self.global_failures.len() >= GLOBAL_ATTEMPTS
            || self.failures.get(device_id).is_some_and(|failures| failures.len() >= DEVICE_ATTEMPTS)
        {
            return Err(OtpError::RateLimited);
        }
        let step = now_ms / 1_000 / STEP_SECONDS;
        let matched = if secret.len() >= 20 && candidate.len() == DIGITS && candidate.bytes().all(|byte| byte.is_ascii_digit()) {
            [Some(step), step.checked_sub(1), step.checked_add(1)]
                .into_iter()
                .flatten()
                .find(|counter| {
                    let expected = format!("{:06}", hotp(secret, *counter, DIGITS as u32));
                    bool::from(expected.as_bytes().ct_eq(candidate.as_bytes()))
                })
        } else {
            None
        };
        match matched {
            Some(counter) if self.last_accepted_step.is_some_and(|last| counter <= last) => {
                self.failures.entry(device_id.to_string()).or_default().push(now_ms);
                self.global_failures.push(now_ms);
                Err(OtpError::Replayed)
            }
            Some(counter) => {
                self.last_accepted_step = Some(counter);
                self.failures.remove(device_id);
                self.global_failures.clear();
                Ok(counter)
            }
            None => {
                self.failures.entry(device_id.to_string()).or_default().push(now_ms);
                self.global_failures.push(now_ms);
                Err(OtpError::Invalid)
            }
        }
    }
}

fn hotp(secret: &[u8], counter: u64, digits: u32) -> u32 {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let tag = hmac::sign(&key, &counter.to_be_bytes());
    let bytes = tag.as_ref();
    let offset = usize::from(bytes[bytes.len() - 1] & 0x0f);
    let value = (u32::from(bytes[offset] & 0x7f) << 24)
        | (u32::from(bytes[offset + 1]) << 16)
        | (u32::from(bytes[offset + 2]) << 8)
        | u32::from(bytes[offset + 3]);
    value % 10u32.pow(digits)
}

#[cfg(test)]
pub(crate) fn test_code(secret: &[u8], now_ms: u64) -> String {
    format!("{:06}", hotp(secret, now_ms / 1_000 / STEP_SECONDS, DIGITS as u32))
}

/// Base32 without padding, as required by the authenticator provisioning URI.
pub(crate) fn base32(secret: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut result = String::with_capacity((secret.len() * 8 + 4) / 5);
    let mut bits = 0u32;
    let mut count = 0u8;
    for byte in secret {
        bits = (bits << 8) | u32::from(*byte);
        count += 8;
        while count >= 5 {
            count -= 5;
            result.push(ALPHABET[((bits >> count) & 31) as usize] as char);
        }
    }
    if count > 0 {
        result.push(ALPHABET[((bits << (5 - count)) & 31) as usize] as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rfc_6238_sha1_vectors() {
        let key = b"12345678901234567890";
        for (seconds, expected) in [(59, 94_287_082), (1_111_111_109, 7_081_804), (1_111_111_111, 14_050_471), (1_234_567_890, 89_005_924), (2_000_000_000, 69_279_037), (20_000_000_000, 65_353_130)] {
            assert_eq!(hotp(key, seconds / STEP_SECONDS, 8), expected);
        }
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn code_is_single_use_across_devices_and_time_windows() {
        let key = b"12345678901234567890";
        let mut verifier = TotpVerifier::default();
        let code = format!("{:06}", hotp(key, 100, 6));
        assert_eq!(verifier.verify(key, "laptop", &code, 3_000_000), Ok(100));
        assert_eq!(verifier.verify(key, "other", &code, 3_000_001), Err(OtpError::Replayed));
        assert_eq!(verifier.verify(key, "laptop", &code, 3_030_000), Err(OtpError::Replayed));
        let next = format!("{:06}", hotp(key, 101, 6));
        assert_eq!(verifier.verify(key, "laptop", &next, 3_030_000), Ok(101));
        assert_eq!(TotpVerifier::with_last_accepted_step(Some(101)).verify(key, "laptop", &next, 3_030_001), Err(OtpError::Replayed));
    }

    #[test]
    fn failures_are_limited_by_device_and_globally() {
        let key = b"12345678901234567890";
        let mut verifier = TotpVerifier::default();
        for _ in 0..DEVICE_ATTEMPTS {
            assert_eq!(verifier.verify(key, "laptop", "bad", 3_000_000), Err(OtpError::Invalid));
        }
        assert_eq!(verifier.verify(key, "laptop", "bad", 3_000_001), Err(OtpError::RateLimited));
        assert_eq!(verifier.verify(key, "laptop", "bad", 3_060_000), Err(OtpError::Invalid));
        let mut verifier = TotpVerifier::default();
        for n in 0..GLOBAL_ATTEMPTS {
            assert_eq!(verifier.verify(key, &format!("device-{n}"), "bad", 3_000_000), Err(OtpError::Invalid));
        }
        assert_eq!(verifier.verify(key, "another", "bad", 3_000_001), Err(OtpError::RateLimited));
    }

    #[test]
    fn rejects_bad_shapes_short_keys_and_old_or_future_codes() {
        let key = b"12345678901234567890";
        for candidate in ["", "1", "12345a", "１２３４５６", "1234567"] {
            assert_eq!(TotpVerifier::default().verify(key, "laptop", candidate, 3_000_000), Err(OtpError::Invalid));
        }
        let code = format!("{:06}", hotp(key, 98, 6));
        assert_eq!(TotpVerifier::default().verify(key, "laptop", &code, 3_000_000), Err(OtpError::Invalid));
        assert_eq!(TotpVerifier::default().verify(b"short", "laptop", "000000", 3_000_000), Err(OtpError::Invalid));
    }
}
