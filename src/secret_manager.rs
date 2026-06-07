//! Secret Manager for secure storage of sensitive information.
//!
//! This module provides a secure way to store and retrieve sensitive data
//! such as API keys, credentials, and tokens using the system's keyring.
//!
//! # Security Features
//!
//! - Uses system keyring integration (macOS Keychain, Windows Credential Manager, Linux Secret Service)
//! - Encrypted storage at rest
//! - No plaintext storage of secrets
//! - Secure credential lookup with proper error handling
//!
//! # Usage
//!
//! ```rust,ignore
//! let secret_manager = SecretManager::new("rupoo", "api-keys");
//!
//! // Store a secret
//! secret_manager.set("openai_api_key", "sk-...").await?;
//!
//! // Retrieve a secret
//! let api_key = secret_manager.get("openai_api_key").await?;
//!
//! // Delete a secret
//! secret_manager.delete("openai_api_key").await?;
//! ```

use tracing::{debug, warn};

use crate::error::{AgentError, AgentResult};

/// Secure secret manager using system keyring.
///
/// This manager provides a secure way to store and retrieve sensitive information
/// using the operating system's native keyring/keychain functionality.
pub struct SecretManager {
    service: String,
    account: String,
}

impl SecretManager {
    /// Create a new SecretManager with the specified service and account names.
    ///
    /// - `service`: The service name for the keyring entry (e.g., "rupoo")
    /// - `account`: The account name for the keyring entry (e.g., "api-keys")
    pub fn new(service: &str, account: &str) -> Self {
        Self {
            service: service.to_string(),
            account: account.to_string(),
        }
    }

    /// Create a SecretManager with default settings for Rupoo.
    pub fn rupoo_default() -> Self {
        Self::new("rupoo", "credentials")
    }

    /// Store a secret value under the specified key.
    ///
    /// # Security
    ///
    /// The secret is stored securely using the system keyring.
    /// On macOS, this uses Keychain.
    /// On Windows, this uses Credential Manager.
    /// On Linux, this uses Secret Service API (GNOME Keyring or similar).
    pub async fn set(&self, key: &str, value: &str) -> AgentResult<()> {
        #[cfg(feature = "keyring")]
        {
            let entry = keyring::Entry::new(&self.service, key)?;
            entry.set_password(value)?;
            debug!(key, "secret stored in keyring");
            Ok(())
        }
        #[cfg(not(feature = "keyring"))]
        {
            warn!("keyring feature not enabled, cannot store secret");
            Err(AgentError::Other("keyring feature not enabled".to_string()))
        }
    }

    /// Retrieve a secret value by key.
    ///
    /// Returns `Ok(None)` if the key does not exist.
    pub async fn get(&self, key: &str) -> AgentResult<Option<String>> {
        #[cfg(feature = "keyring")]
        {
            let entry = keyring::Entry::new(&self.service, key)?;
            match entry.get_password() {
                Ok(password) => {
                    debug!(key, "secret retrieved from keyring");
                    Ok(Some(password))
                }
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(AgentError::Other(format!("failed to get secret: {}", e))),
            }
        }
        #[cfg(not(feature = "keyring"))]
        {
            warn!("keyring feature not enabled, cannot retrieve secret");
            Ok(None)
        }
    }

    /// Delete a secret by key.
    ///
    /// Returns `true` if the secret existed and was deleted, `false` otherwise.
    pub async fn delete(&self, key: &str) -> AgentResult<bool> {
        #[cfg(feature = "keyring")]
        {
            let entry = keyring::Entry::new(&self.service, key)?;
            match entry.delete_password() {
                Ok(_) => {
                    debug!(key, "secret deleted from keyring");
                    Ok(true)
                }
                Err(keyring::Error::NoEntry) => Ok(false),
                Err(e) => Err(AgentError::Other(format!("failed to delete secret: {}", e))),
            }
        }
        #[cfg(not(feature = "keyring"))]
        {
            warn!("keyring feature not enabled, cannot delete secret");
            Ok(false)
        }
    }

    /// Check if a secret exists for the given key.
    pub async fn exists(&self, key: &str) -> AgentResult<bool> {
        self.get(key).await.map(|opt| opt.is_some())
    }

    /// List all secret keys stored in the keyring for this service.
    ///
    /// Note: This operation may not be supported on all platforms.
    pub async fn list_keys(&self) -> AgentResult<Vec<String>> {
        #[cfg(feature = "keyring")]
        {
            // keyring crate doesn't provide a direct list method
            // This is a limitation of the underlying system APIs
            warn!("listing keys not directly supported by keyring API");
            Ok(Vec::new())
        }
        #[cfg(not(feature = "keyring"))]
        {
            warn!("keyring feature not enabled");
            Ok(Vec::new())
        }
    }

    /// Get the service name associated with this SecretManager.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// Get the account name associated with this SecretManager.
    pub fn account(&self) -> &str {
        &self.account
    }
}

impl Default for SecretManager {
    fn default() -> Self {
        Self::new("rupoo", "credentials")
    }
}

/// Helper trait for secure settings storage.
///
/// This trait provides a secure alternative to storing sensitive information
/// in plaintext configuration files.
#[async_trait::async_trait]
pub trait SecureSettingsStorage {
    /// Store a secure setting.
    async fn set_secure(&self, key: &str, value: &str) -> AgentResult<()>;

    /// Retrieve a secure setting.
    async fn get_secure(&self, key: &str) -> AgentResult<Option<String>>;

    /// Delete a secure setting.
    async fn delete_secure(&self, key: &str) -> AgentResult<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_secret_manager_basic() {
        let manager = SecretManager::new("rupoo-test", "test-account");
        let test_key = "test-api-key";
        let test_value = "sk-test-12345";

        let _ = manager.delete(test_key).await;

        let set_result = manager.set(test_key, test_value).await;
        if let Err(AgentError::Keyring(_)) = set_result {
            println!("Skipping test: keyring not available in this environment");
            return;
        }
        set_result.unwrap();

        let result = manager.get(test_key).await.unwrap();
        assert_eq!(result, Some(test_value.to_string()));

        let exists = manager.exists(test_key).await.unwrap();
        assert!(exists);

        let deleted = manager.delete(test_key).await.unwrap();
        assert!(deleted);

        let result = manager.get(test_key).await.unwrap();
        assert!(result.is_none());

        let exists = manager.exists(test_key).await.unwrap();
        assert!(!exists);
    }

    #[tokio::test]
    async fn test_secret_manager_not_found() {
        let manager = SecretManager::new("rupoo-test", "test-account");

        let result = manager.get("non-existent-key").await;
        match result {
            Ok(None) => (),
            Err(AgentError::Keyring(_)) => {
                println!("Skipping test: keyring not available in this environment");
            }
            other => panic!("Unexpected result: {:?}", other),
        }

        let deleted = manager.delete("non-existent-key").await;
        match deleted {
            Ok(false) => (),
            Err(AgentError::Keyring(_)) => {
                println!("Skipping test: keyring not available in this environment");
            }
            other => panic!("Unexpected result: {:?}", other),
        }
    }
}
