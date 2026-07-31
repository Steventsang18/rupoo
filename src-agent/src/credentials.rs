//! Credential encryption at rest.
//!
//! `credentials.toml` traditionally holds API keys in plaintext, protected
//! only by file permissions (0600). This module adds optional value-level
//! encryption for those keys:
//!
//! - Encrypted values are stored as `enc:v1:<hex(nonce || ciphertext)>`
//!   (AES-256-GCM, random 12-byte nonce per value).
//! - The master key is resolved from `$RUPOO_MASTER_KEY` (64 hex chars),
//!   falling back to the system keyring entry `rupoo/master-key`.
//! - If no master key is available the vault reports `available() == false`
//!   and plaintext credentials keep working — encryption is an explicit
//!   choice, never a hard dependency.
//!
//! The keyring fallback intentionally uses the synchronous `keyring` API:
//! credential loading happens on cold paths (config load, one-off commands)
//! where blocking briefly is acceptable and avoids smuggling async through
//! the sync config layer.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use tracing::warn;
use zeroize::Zeroizing;

use crate::error::{AgentError, AgentResult};

/// Prefix marking an encrypted credential value.
pub const ENC_PREFIX: &str = "enc:v1:";

/// AES-256-GCM nonce length (12 bytes).
const NONCE_LEN: usize = 12;
/// Master key length in bytes (32 = AES-256).
const KEY_LEN: usize = 32;

/// Keyring service/account used for the master key when the environment
/// variable is not set.
const KEYRING_SERVICE: &str = "rupoo";
const KEYRING_ACCOUNT: &str = "master-key";

/// Resolves the master key and encrypts/decrypts credential values.
pub struct CredentialVault {
    key: Option<Zeroizing<[u8; KEY_LEN]>>,
}

impl CredentialVault {
    /// Load the master key from `$RUPOO_MASTER_KEY`, then the system keyring.
    ///
    /// Returns an empty vault when neither source yields a valid 32-byte key;
    /// callers should check [`CredentialVault::available`] before encrypting.
    pub fn try_load() -> Self {
        if let Some(key) = master_key_from_env() {
            return Self { key: Some(key) };
        }
        #[cfg(feature = "keyring")]
        if let Some(key) = master_key_from_keyring() {
            return Self { key: Some(key) };
        }
        Self { key: None }
    }

    /// Whether a master key is available for encryption.
    pub fn available(&self) -> bool {
        self.key.is_some()
    }

    /// Encrypt a plaintext value into the `enc:v1:` format.
    ///
    /// Fails when no master key is available. Never includes the plaintext
    /// in error messages.
    pub fn encrypt(&self, plaintext: &str) -> AgentResult<String> {
        let key = self.key.as_ref().ok_or_else(|| {
            AgentError::Other(
                "no master key available — set RUPOO_MASTER_KEY (64 hex chars)".into(),
            )
        })?;

        let cipher = Aes256Gcm::new(aes_gcm::Key::<Aes256Gcm>::from_slice(key.as_ref()));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| AgentError::Other("credential encryption failed".into()))?;

        let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ciphertext);

        Ok(format!("{ENC_PREFIX}{}", hex_encode(&blob)))
    }

    /// Decrypt an `enc:v1:` value.
    ///
    /// Returns `None` for plaintext values (no prefix), when no master key
    /// is available, or when the value is corrupted — a corrupted value is
    /// never silently replaced, the caller decides how to handle it.
    pub fn decrypt(&self, value: &str) -> Option<String> {
        let blob = value.strip_prefix(ENC_PREFIX)?;

        let raw = match hex_decode(blob) {
            Some(raw) => raw,
            None => {
                warn!("credential value has invalid enc:v1: encoding");
                return None;
            }
        };
        if raw.len() < NONCE_LEN {
            warn!("credential value is too short to be valid enc:v1: data");
            return None;
        }

        let key = self.key.as_ref()?;
        let (nonce, ciphertext) = raw.split_at(NONCE_LEN);
        let cipher = Aes256Gcm::new(aes_gcm::Key::<Aes256Gcm>::from_slice(key.as_ref()));

        let plain = cipher.decrypt(Nonce::from_slice(nonce), ciphertext).ok()?;
        String::from_utf8(plain).ok()
    }
}

// ---------------------------------------------------------------------------
// Master key resolution
// ---------------------------------------------------------------------------

fn master_key_from_env() -> Option<Zeroizing<[u8; KEY_LEN]>> {
    let value = std::env::var("RUPOO_MASTER_KEY").ok()?;
    match parse_hex_key(&value) {
        Some(key) => Some(key),
        None => {
            warn!(
                "RUPOO_MASTER_KEY must be exactly {} hex chars, ignoring",
                KEY_LEN * 2
            );
            None
        }
    }
}

#[cfg(feature = "keyring")]
fn master_key_from_keyring() -> Option<Zeroizing<[u8; KEY_LEN]>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).ok()?;
    let value = entry.get_password().ok()?;
    parse_hex_key(&value)
}

fn parse_hex_key(value: &str) -> Option<Zeroizing<[u8; KEY_LEN]>> {
    let raw = hex_decode(value)?;
    if raw.len() != KEY_LEN {
        return None;
    }
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    key.copy_from_slice(&raw);
    Some(key)
}

// ---------------------------------------------------------------------------
// Minimal hex encoding (avoids a dependency for ~30 lines)
// ---------------------------------------------------------------------------

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let hi = hex_val(chunk[0])?;
        let lo = hex_val(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_with_key() -> CredentialVault {
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        key.copy_from_slice(b"0123456789abcdef0123456789abcdef");
        CredentialVault { key: Some(key) }
    }

    #[test]
    fn test_hex_round_trip() {
        let data = [0x00, 0x0f, 0x10, 0xff, 0xab];
        let encoded = hex_encode(&data);
        assert_eq!(encoded, "000f10ffab");
        assert_eq!(hex_decode(&encoded), Some(data.to_vec()));
    }

    #[test]
    fn test_hex_decode_rejects_invalid() {
        assert_eq!(hex_decode("abc"), None); // odd length
        assert_eq!(hex_decode("zz"), None); // non-hex chars
        assert_eq!(hex_decode(""), Some(vec![]));
    }

    #[test]
    fn test_encrypt_decrypt_round_trip() {
        let vault = vault_with_key();
        let encrypted = vault.encrypt("sk-ant-test-123").unwrap();

        assert!(encrypted.starts_with(ENC_PREFIX));
        assert!(!encrypted.contains("sk-ant-test-123"));

        let decrypted = vault.decrypt(&encrypted);
        assert_eq!(decrypted.as_deref(), Some("sk-ant-test-123"));
    }

    #[test]
    fn test_encrypt_produces_unique_values() {
        let vault = vault_with_key();
        let a = vault.encrypt("same-value").unwrap();
        let b = vault.encrypt("same-value").unwrap();
        assert_ne!(a, b, "random nonce must yield distinct ciphertexts");
    }

    #[test]
    fn test_decrypt_plaintext_returns_none() {
        let vault = vault_with_key();
        assert_eq!(vault.decrypt("sk-plaintext-123"), None);
    }

    #[test]
    fn test_decrypt_corrupted_value_returns_none() {
        let vault = vault_with_key();
        let encrypted = vault.encrypt("secret").unwrap();
        // Flip a char inside the hex blob — must fail cleanly, not panic.
        let corrupted = format!("{}{}", ENC_PREFIX, "0");
        let truncated = encrypted[..encrypted.len() - 2].to_string();
        assert_eq!(vault.decrypt(&corrupted), None);
        assert_eq!(vault.decrypt(&truncated), None);
    }

    #[test]
    fn test_decrypt_with_wrong_key_returns_none() {
        let encrypted = vault_with_key().encrypt("secret").unwrap();

        let mut other = Zeroizing::new([0u8; KEY_LEN]);
        other.copy_from_slice(b"fedcba9876543210fedcba9876543210");
        let wrong_vault = CredentialVault { key: Some(other) };

        assert_eq!(wrong_vault.decrypt(&encrypted), None);
    }

    #[test]
    fn test_empty_vault_cannot_encrypt_or_decrypt() {
        let vault = CredentialVault { key: None };
        assert!(!vault.available());
        assert!(vault.encrypt("secret").is_err());
        assert_eq!(vault.decrypt("enc:v1:abcd"), None);
    }

    #[test]
    fn test_parse_hex_key_validates_length() {
        let good = "a".repeat(64);
        assert!(parse_hex_key(&good).is_some());

        let too_short = "a".repeat(62);
        assert!(parse_hex_key(&too_short).is_none());

        let non_hex = "g".repeat(64);
        assert!(parse_hex_key(&non_hex).is_none());
    }

    #[test]
    fn test_master_key_from_env() {
        std::env::set_var("RUPOO_MASTER_KEY", "b".repeat(64));
        let key = master_key_from_env();
        assert!(key.is_some());
        std::env::remove_var("RUPOO_MASTER_KEY");

        std::env::set_var("RUPOO_MASTER_KEY", "bad-key");
        assert!(master_key_from_env().is_none());
        std::env::remove_var("RUPOO_MASTER_KEY");

        assert!(master_key_from_env().is_none());
    }
}
