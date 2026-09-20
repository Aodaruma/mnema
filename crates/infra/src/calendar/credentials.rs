use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use mnema_core::ids::CalendarAccountId;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthCredential {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub scope: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
}

impl fmt::Debug for OAuthCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OAuthCredential")
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("token_type", &self.token_type)
            .field("scope", &self.scope)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl OAuthCredential {
    #[must_use]
    pub fn expires_soon(&self, now: OffsetDateTime) -> bool {
        self.expires_at
            .is_some_and(|expires_at| expires_at <= now + Duration::minutes(1))
    }
}

#[derive(Debug, Error)]
pub enum CredentialStoreError {
    #[error("credential store error: {0}")]
    Storage(String),
}

#[async_trait::async_trait]
pub trait CredentialStore: Send + Sync {
    async fn load(
        &self,
        account_id: CalendarAccountId,
    ) -> Result<Option<OAuthCredential>, CredentialStoreError>;
    async fn save(
        &self,
        account_id: CalendarAccountId,
        credential: OAuthCredential,
    ) -> Result<(), CredentialStoreError>;
    async fn delete(&self, account_id: CalendarAccountId) -> Result<(), CredentialStoreError>;
}

/// OAuth credential persistence backed by the operating system's secure store.
///
/// Entries are keyed by calendar-account UUID under the configured service.
/// The stored secret is a JSON representation of [`OAuthCredential`]; neither
/// this type's `Debug` output nor Mnema's database contains token material.
#[derive(Clone)]
pub struct KeyringCredentialStore {
    service: Arc<str>,
    access: Arc<tokio::sync::Mutex<()>>,
}

impl fmt::Debug for KeyringCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KeyringCredentialStore")
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl KeyringCredentialStore {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: Arc::from(service.into()),
            access: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    #[must_use]
    pub fn mnema() -> Self {
        Self::new("mnema.calendar.oauth")
    }

    fn username(account_id: &CalendarAccountId) -> String {
        account_id.0.to_string()
    }
}

#[async_trait::async_trait]
impl CredentialStore for KeyringCredentialStore {
    async fn load(
        &self,
        account_id: CalendarAccountId,
    ) -> Result<Option<OAuthCredential>, CredentialStoreError> {
        let _guard = self.access.lock().await;
        let service = self.service.to_string();
        let username = Self::username(&account_id);
        let serialized = tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&service, &username)
                .map_err(|error| CredentialStoreError::Storage(error.to_string()))?;
            match entry.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(error) => Err(CredentialStoreError::Storage(error.to_string())),
            }
        })
        .await
        .map_err(|error| CredentialStoreError::Storage(error.to_string()))??;

        serialized
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| CredentialStoreError::Storage(error.to_string()))
            })
            .transpose()
    }

    async fn save(
        &self,
        account_id: CalendarAccountId,
        credential: OAuthCredential,
    ) -> Result<(), CredentialStoreError> {
        let serialized = serde_json::to_string(&credential)
            .map_err(|error| CredentialStoreError::Storage(error.to_string()))?;
        let _guard = self.access.lock().await;
        let service = self.service.to_string();
        let username = Self::username(&account_id);
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&service, &username)
                .map_err(|error| CredentialStoreError::Storage(error.to_string()))?;
            entry
                .set_password(&serialized)
                .map_err(|error| CredentialStoreError::Storage(error.to_string()))
        })
        .await
        .map_err(|error| CredentialStoreError::Storage(error.to_string()))?
    }

    async fn delete(&self, account_id: CalendarAccountId) -> Result<(), CredentialStoreError> {
        let _guard = self.access.lock().await;
        let service = self.service.to_string();
        let username = Self::username(&account_id);
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(&service, &username)
                .map_err(|error| CredentialStoreError::Storage(error.to_string()))?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(CredentialStoreError::Storage(error.to_string())),
            }
        })
        .await
        .map_err(|error| CredentialStoreError::Storage(error.to_string()))?
    }
}

/// Volatile implementation for tests and short-lived sessions.
///
/// Production callers should provide an OS-backed implementation (Windows
/// Credential Manager, macOS Keychain, or Secret Service) through
/// [`CredentialStore`]. OAuth credentials are deliberately absent from Mnema's
/// database repositories and migrations.
#[derive(Clone, Default)]
pub struct MemoryCredentialStore {
    credentials: Arc<RwLock<HashMap<CalendarAccountId, OAuthCredential>>>,
}

#[async_trait::async_trait]
impl CredentialStore for MemoryCredentialStore {
    async fn load(
        &self,
        account_id: CalendarAccountId,
    ) -> Result<Option<OAuthCredential>, CredentialStoreError> {
        Ok(self.credentials.read().await.get(&account_id).cloned())
    }

    async fn save(
        &self,
        account_id: CalendarAccountId,
        credential: OAuthCredential,
    ) -> Result<(), CredentialStoreError> {
        self.credentials
            .write()
            .await
            .insert(account_id, credential);
        Ok(())
    }

    async fn delete(&self, account_id: CalendarAccountId) -> Result<(), CredentialStoreError> {
        self.credentials.write().await.remove(&account_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[tokio::test]
    async fn memory_store_roundtrips_without_exposing_tokens_in_debug() {
        let store = MemoryCredentialStore::default();
        let account_id = CalendarAccountId::new();
        let credential = OAuthCredential {
            access_token: "access-secret".into(),
            refresh_token: Some("refresh-secret".into()),
            token_type: "Bearer".into(),
            scope: None,
            expires_at: Some(datetime!(2026-08-15 01:00 UTC)),
        };

        store
            .save(account_id.clone(), credential.clone())
            .await
            .unwrap();
        assert_eq!(
            store.load(account_id).await.unwrap(),
            Some(credential.clone())
        );
        let debug = format!("{credential:?}");
        assert!(!debug.contains("access-secret"));
        assert!(!debug.contains("refresh-secret"));
    }

    #[test]
    fn keyring_store_debug_contains_no_credential_material() {
        let store = KeyringCredentialStore::mnema();
        let debug = format!("{store:?}");
        assert!(debug.contains("mnema.calendar.oauth"));
        assert!(!debug.contains("access_token"));
        assert!(!debug.contains("refresh_token"));
    }
}
