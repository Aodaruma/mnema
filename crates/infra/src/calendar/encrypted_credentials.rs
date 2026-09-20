use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aes_gcm::aead::{Aead, AeadCore, OsRng, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use mnema_core::ids::CalendarAccountId;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::credentials::{CredentialStore, CredentialStoreError, OAuthCredential};

const ENVELOPE_VERSION: u8 = 1;
const MAX_CREDENTIAL_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedEnvelope {
    version: u8,
    nonce: String,
    ciphertext: String,
}

/// AES-256-GCM credential store for headless environments without an OS keyring.
///
/// Callers supply the 32-byte master key (for example from
/// `MNEMA_CREDENTIAL_KEY` or a mounted secret file). One authenticated encrypted
/// file is stored per calendar-account UUID; OAuth token text is never written
/// in plaintext. The account UUID is authenticated as AES-GCM additional data,
/// preventing ciphertext files from being swapped between accounts.
#[derive(Clone)]
pub struct EncryptedFileCredentialStore {
    directory: Arc<PathBuf>,
    cipher: Arc<Aes256Gcm>,
    access: Arc<tokio::sync::Mutex<()>>,
}

impl fmt::Debug for EncryptedFileCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncryptedFileCredentialStore")
            .field("directory", &self.directory)
            .field("cipher", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl EncryptedFileCredentialStore {
    #[must_use]
    pub fn new(directory: impl Into<PathBuf>, mut master_key: [u8; 32]) -> Self {
        let cipher = Aes256Gcm::new_from_slice(&master_key)
            .expect("a 32-byte AES-256 key is always a valid key");
        master_key.zeroize();
        Self {
            directory: Arc::new(directory.into()),
            cipher: Arc::new(cipher),
            access: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Decodes a URL-safe, unpadded base64 master key supplied by the caller.
    pub fn from_base64_key(
        directory: impl Into<PathBuf>,
        encoded_master_key: &str,
    ) -> Result<Self, CredentialStoreError> {
        let mut decoded = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(encoded_master_key)
                .map_err(|_| storage_error("credential master key is not valid base64url"))?,
        );
        if decoded.len() != 32 {
            return Err(storage_error(
                "credential master key must decode to exactly 32 bytes",
            ));
        }
        let mut key = [0_u8; 32];
        key.copy_from_slice(&decoded);
        decoded.zeroize();
        Ok(Self::new(directory, key))
    }

    fn path_for(directory: &Path, account_id: &CalendarAccountId) -> PathBuf {
        directory.join(format!("{}.oauth.json.enc", account_id.0))
    }
}

#[async_trait::async_trait]
impl CredentialStore for EncryptedFileCredentialStore {
    async fn load(
        &self,
        account_id: CalendarAccountId,
    ) -> Result<Option<OAuthCredential>, CredentialStoreError> {
        let _guard = self.access.lock().await;
        let path = Self::path_for(&self.directory, &account_id);
        let aad = account_id.0.to_string();
        let cipher = self.cipher.clone();
        tokio::task::spawn_blocking(move || read_credential(&path, &aad, &cipher))
            .await
            .map_err(|error| storage_error(error.to_string()))?
    }

    async fn save(
        &self,
        account_id: CalendarAccountId,
        credential: OAuthCredential,
    ) -> Result<(), CredentialStoreError> {
        let _guard = self.access.lock().await;
        let path = Self::path_for(&self.directory, &account_id);
        let aad = account_id.0.to_string();
        let cipher = self.cipher.clone();
        tokio::task::spawn_blocking(move || write_credential(&path, &aad, &cipher, &credential))
            .await
            .map_err(|error| storage_error(error.to_string()))?
    }

    async fn delete(&self, account_id: CalendarAccountId) -> Result<(), CredentialStoreError> {
        let _guard = self.access.lock().await;
        let path = Self::path_for(&self.directory, &account_id);
        tokio::task::spawn_blocking(move || {
            remove_if_exists(&path)?;
            remove_if_exists(&backup_path(&path))
        })
        .await
        .map_err(|error| storage_error(error.to_string()))?
    }
}

fn write_credential(
    path: &Path,
    aad: &str,
    cipher: &Aes256Gcm,
    credential: &OAuthCredential,
) -> Result<(), CredentialStoreError> {
    let plaintext = Zeroizing::new(
        serde_json::to_vec(credential).map_err(|error| storage_error(error.to_string()))?,
    );
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: &plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| storage_error("credential encryption failed"))?;
    let envelope = EncryptedEnvelope {
        version: ENVELOPE_VERSION,
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
    };
    let serialized =
        serde_json::to_vec(&envelope).map_err(|error| storage_error(error.to_string()))?;
    write_atomically(path, &serialized)
}

fn read_credential(
    path: &Path,
    aad: &str,
    cipher: &Aes256Gcm,
) -> Result<Option<OAuthCredential>, CredentialStoreError> {
    let path = match fs::metadata(path) {
        Ok(metadata) => {
            if metadata.len() > MAX_CREDENTIAL_FILE_BYTES {
                return Err(storage_error("encrypted credential file is too large"));
            }
            path.to_path_buf()
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let backup = backup_path(path);
            match fs::metadata(&backup) {
                Ok(metadata) => {
                    if metadata.len() > MAX_CREDENTIAL_FILE_BYTES {
                        return Err(storage_error("encrypted credential backup is too large"));
                    }
                    backup
                }
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(storage_error(error.to_string())),
            }
        }
        Err(error) => return Err(storage_error(error.to_string())),
    };
    let serialized = fs::read(path).map_err(|error| storage_error(error.to_string()))?;
    let envelope: EncryptedEnvelope = serde_json::from_slice(&serialized)
        .map_err(|_| storage_error("invalid credential envelope"))?;
    if envelope.version != ENVELOPE_VERSION {
        return Err(storage_error("unsupported credential envelope version"));
    }
    let nonce = URL_SAFE_NO_PAD
        .decode(envelope.nonce)
        .map_err(|_| storage_error("invalid credential nonce"))?;
    if nonce.len() != 12 {
        return Err(storage_error("invalid credential nonce length"));
    }
    let ciphertext = URL_SAFE_NO_PAD
        .decode(envelope.ciphertext)
        .map_err(|_| storage_error("invalid credential ciphertext"))?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| storage_error("credential decryption failed"))?,
    );
    serde_json::from_slice(&plaintext)
        .map(Some)
        .map_err(|_| storage_error("decrypted credential is invalid"))
}

fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), CredentialStoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| storage_error("credential path has no parent directory"))?;
    fs::create_dir_all(parent).map_err(|error| storage_error(error.to_string()))?;
    let temporary = parent.join(format!(".credential-{}.tmp", uuid::Uuid::new_v4()));
    let backup = backup_path(path);

    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| storage_error(error.to_string()))?;
        file.write_all(contents)
            .map_err(|error| storage_error(error.to_string()))?;
        file.sync_all()
            .map_err(|error| storage_error(error.to_string()))?;

        remove_if_exists(&backup)?;
        let had_original = path.exists();
        if had_original {
            fs::rename(path, &backup).map_err(|error| storage_error(error.to_string()))?;
        }
        if let Err(error) = fs::rename(&temporary, path) {
            if had_original {
                let _ = fs::rename(&backup, path);
            }
            return Err(storage_error(error.to_string()));
        }
        remove_if_exists(&backup)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("enc.bak")
}

fn remove_if_exists(path: &Path) -> Result<(), CredentialStoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error.to_string())),
    }
}

fn storage_error(message: impl Into<String>) -> CredentialStoreError {
    CredentialStoreError::Storage(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn credential() -> OAuthCredential {
        OAuthCredential {
            access_token: "access-secret".into(),
            refresh_token: Some("refresh-secret".into()),
            token_type: "Bearer".into(),
            scope: Some("calendar".into()),
            expires_at: Some(datetime!(2026-08-15 01:00 UTC)),
        }
    }

    #[tokio::test]
    async fn survives_recreation_without_plaintext_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let account_id = CalendarAccountId::new();
        let expected = credential();
        let store = EncryptedFileCredentialStore::new(directory.path(), [7_u8; 32]);
        store
            .save(account_id.clone(), expected.clone())
            .await
            .unwrap();

        let bytes = fs::read(EncryptedFileCredentialStore::path_for(
            directory.path(),
            &account_id,
        ))
        .unwrap();
        assert!(
            !bytes
                .windows(b"access-secret".len())
                .any(|value| value == b"access-secret")
        );
        assert!(
            !bytes
                .windows(b"refresh-secret".len())
                .any(|value| value == b"refresh-secret")
        );

        let recreated = EncryptedFileCredentialStore::new(directory.path(), [7_u8; 32]);
        assert_eq!(
            recreated.load(account_id.clone()).await.unwrap(),
            Some(expected)
        );
        assert!(!format!("{recreated:?}").contains("070707"));
        recreated.delete(account_id.clone()).await.unwrap();
        assert!(recreated.load(account_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn wrong_key_cannot_decrypt_or_swap_accounts() {
        let directory = tempfile::tempdir().unwrap();
        let account_id = CalendarAccountId::new();
        EncryptedFileCredentialStore::new(directory.path(), [1_u8; 32])
            .save(account_id.clone(), credential())
            .await
            .unwrap();
        assert!(
            EncryptedFileCredentialStore::new(directory.path(), [2_u8; 32])
                .load(account_id.clone())
                .await
                .is_err()
        );

        let other_id = CalendarAccountId::new();
        let original = EncryptedFileCredentialStore::path_for(directory.path(), &account_id);
        let swapped = EncryptedFileCredentialStore::path_for(directory.path(), &other_id);
        fs::copy(original, swapped).unwrap();
        assert!(
            EncryptedFileCredentialStore::new(directory.path(), [1_u8; 32])
                .load(other_id)
                .await
                .is_err()
        );
    }
}
