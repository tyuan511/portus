//! Encrypted Credential Vault (`vault.enc`) using Argon2id + AES-256-GCM.
//!
//! Security properties:
//! - Master key is derived via Argon2id from an operator passphrase and held in
//!   `Zeroizing<[u8; 32]>` only while unlocked.
//! - Every credential is encrypted individually with AES-256-GCM using a fresh 96-bit
//!   nonce and `credential_id` bound as AEAD Associated Data (AAD).
//! - SQLite, MCP tools, and the React UI never see plaintext secrets — only `VaultCredentialMeta`.
//! - Automatic idle lock wipes the master key from memory after `auto_lock_secs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use chrono::Utc;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sshield_core::{
    Result, SshieldError, VaultCredentialKind, VaultCredentialMeta, VaultLockState, VaultStatus,
};
use tokio::sync::Mutex;
use uuid::Uuid;
use zeroize::Zeroizing;

const KEY_VERIFIER_AAD: &[u8] = b"sshield-vault-verifier-v1";
const KEY_VERIFIER_PLAINTEXT: &[u8] = b"SSHIELD_VAULT_OK_V1";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SealedRecord {
    meta: VaultCredentialMeta,
    nonce_hex: String,
    ciphertext_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultFile {
    version: u32,
    salt_hex: String,
    verifier_nonce_hex: String,
    verifier_ct_hex: String,
    records: HashMap<String, SealedRecord>,
}

struct UnlockedKey {
    key: Zeroizing<[u8; 32]>,
    last_used: Instant,
}

/// Thread-safe encrypted credential vault with auto-lock and memory zeroization.
pub struct SealedVault {
    path: PathBuf,
    auto_lock_secs: u64,
    state: Mutex<Option<UnlockedKey>>,
}

impl SealedVault {
    pub fn new(path: impl Into<PathBuf>, auto_lock_secs: u64) -> Self {
        Self {
            path: path.into(),
            auto_lock_secs,
            state: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return current lock state and credential count without exposing secrets.
    pub async fn status(&self) -> Result<VaultStatus> {
        if !self.path.exists() {
            return Ok(VaultStatus {
                state: VaultLockState::Uninitialized,
                credential_count: 0,
                auto_lock_secs: self.auto_lock_secs,
            });
        }

        let file = self.read_file()?;
        let mut guard = self.state.lock().await;
        if let Some(ref unlocked) = *guard
            && self.auto_lock_secs > 0
            && unlocked.last_used.elapsed() > Duration::from_secs(self.auto_lock_secs)
        {
            *guard = None;
        }

        let state = if guard.is_some() {
            VaultLockState::Unlocked
        } else {
            VaultLockState::Locked
        };

        Ok(VaultStatus {
            state,
            credential_count: file.records.len(),
            auto_lock_secs: self.auto_lock_secs,
        })
    }

    /// Initialize a brand-new encrypted vault with an operator master passphrase.
    pub async fn initialize(&self, passphrase: &str) -> Result<()> {
        if passphrase.len() < 8 {
            return Err(SshieldError::Vault(
                "Master passphrase must be at least 8 characters".into(),
            ));
        }
        if self.path.exists() {
            return Err(SshieldError::Vault(
                "Vault is already initialized; use unlock instead".into(),
            ));
        }
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| SshieldError::Vault(format!("Failed to create vault dir: {e}")))?;
        }

        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = derive_master_key(passphrase, &salt)?;

        let mut v_nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut v_nonce);
        let cipher = Aes256Gcm::new_from_slice(&*key)
            .map_err(|e| SshieldError::Vault(format!("Cipher init failed: {e}")))?;
        let verifier_ct = cipher
            .encrypt(
                Nonce::from_slice(&v_nonce),
                Payload {
                    msg: KEY_VERIFIER_PLAINTEXT,
                    aad: KEY_VERIFIER_AAD,
                },
            )
            .map_err(|e| SshieldError::Vault(format!("Verifier encryption failed: {e}")))?;

        let vault_file = VaultFile {
            version: 1,
            salt_hex: hex::encode(salt),
            verifier_nonce_hex: hex::encode(v_nonce),
            verifier_ct_hex: hex::encode(verifier_ct),
            records: HashMap::new(),
        };
        self.write_file(&vault_file)?;

        let mut guard = self.state.lock().await;
        *guard = Some(UnlockedKey {
            key,
            last_used: Instant::now(),
        });
        Ok(())
    }

    /// Unlock an existing vault by deriving the Argon2id key and verifying the AEAD tag.
    pub async fn unlock(&self, passphrase: &str) -> Result<()> {
        if !self.path.exists() {
            return Err(SshieldError::Vault("Vault is not initialized yet".into()));
        }
        let file = self.read_file()?;
        let salt = hex::decode(&file.salt_hex)
            .map_err(|e| SshieldError::Vault(format!("Invalid vault salt: {e}")))?;
        let key = derive_master_key(passphrase, &salt)?;

        let v_nonce = hex::decode(&file.verifier_nonce_hex)
            .map_err(|e| SshieldError::Vault(format!("Invalid verifier nonce: {e}")))?;
        let v_ct = hex::decode(&file.verifier_ct_hex)
            .map_err(|e| SshieldError::Vault(format!("Invalid verifier ciphertext: {e}")))?;

        let cipher = Aes256Gcm::new_from_slice(&*key)
            .map_err(|e| SshieldError::Vault(format!("Cipher init failed: {e}")))?;
        let pt = cipher
            .decrypt(
                Nonce::from_slice(&v_nonce),
                Payload {
                    msg: &v_ct,
                    aad: KEY_VERIFIER_AAD,
                },
            )
            .map_err(|_| SshieldError::Vault("Invalid master passphrase".into()))?;

        if pt.as_slice() != KEY_VERIFIER_PLAINTEXT {
            return Err(SshieldError::Vault("Vault verifier mismatch".into()));
        }

        let mut guard = self.state.lock().await;
        *guard = Some(UnlockedKey {
            key,
            last_used: Instant::now(),
        });
        Ok(())
    }

    /// Immediately zeroize the master key in memory and lock the vault.
    pub async fn lock(&self) {
        let mut guard = self.state.lock().await;
        *guard = None;
    }

    /// List non-secret metadata for all stored credentials (works even when locked!).
    pub fn list_credentials_meta(&self) -> Result<Vec<VaultCredentialMeta>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = self.read_file()?;
        let mut list: Vec<VaultCredentialMeta> =
            file.records.into_values().map(|r| r.meta).collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(list)
    }

    /// Encrypt and store a secret credential (password, private key PEM, or LLM API key).
    pub async fn store_secret(
        &self,
        id: Option<String>,
        label: String,
        kind: VaultCredentialKind,
        username: Option<String>,
        secret: &str,
    ) -> Result<VaultCredentialMeta> {
        let key = self.active_key().await?;
        let cred_id = id.unwrap_or_else(|| format!("cred-{}", &Uuid::new_v4().to_string()[..8]));

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let cipher = Aes256Gcm::new_from_slice(&*key)
            .map_err(|e| SshieldError::Vault(format!("Cipher init failed: {e}")))?;

        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: secret.as_bytes(),
                    aad: cred_id.as_bytes(),
                },
            )
            .map_err(|e| SshieldError::Vault(format!("Credential encryption failed: {e}")))?;

        let meta = VaultCredentialMeta {
            id: cred_id.clone(),
            label,
            kind,
            username,
            preview: mask_preview(secret, kind),
            created_at: Utc::now(),
        };

        let mut file = self.read_file()?;
        file.records.insert(
            cred_id,
            SealedRecord {
                meta: meta.clone(),
                nonce_hex: hex::encode(nonce_bytes),
                ciphertext_hex: hex::encode(ciphertext),
            },
        );
        self.write_file(&file)?;
        Ok(meta)
    }

    /// Decrypt a credential into a `Zeroizing<String>` buffer for immediate backend use only.
    pub async fn get_secret(&self, cred_id: &str) -> Result<Zeroizing<String>> {
        let key = self.active_key().await?;
        let file = self.read_file()?;
        let record = file
            .records
            .get(cred_id)
            .ok_or_else(|| SshieldError::Vault(format!("Credential `{cred_id}` not found")))?;

        let nonce = hex::decode(&record.nonce_hex)
            .map_err(|e| SshieldError::Vault(format!("Corrupt nonce: {e}")))?;
        let ct = hex::decode(&record.ciphertext_hex)
            .map_err(|e| SshieldError::Vault(format!("Corrupt ciphertext: {e}")))?;

        let cipher = Aes256Gcm::new_from_slice(&*key)
            .map_err(|e| SshieldError::Vault(format!("Cipher init failed: {e}")))?;
        let pt = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ct,
                    aad: cred_id.as_bytes(),
                },
            )
            .map_err(|_| SshieldError::Vault("AEAD decryption failed (tampered or wrong key)".into()))?;

        let s = String::from_utf8(pt)
            .map_err(|e| SshieldError::Vault(format!("Non-UTF8 secret: {e}")))?;
        Ok(Zeroizing::new(s))
    }

    pub async fn delete_secret(&self, cred_id: &str) -> Result<bool> {
        let _ = self.active_key().await?;
        let mut file = self.read_file()?;
        let removed = file.records.remove(cred_id).is_some();
        if removed {
            self.write_file(&file)?;
        }
        Ok(removed)
    }

    async fn active_key(&self) -> Result<Zeroizing<[u8; 32]>> {
        let mut guard = self.state.lock().await;
        if let Some(ref mut unlocked) = *guard {
            if self.auto_lock_secs > 0
                && unlocked.last_used.elapsed() > Duration::from_secs(self.auto_lock_secs)
            {
                *guard = None;
                return Err(SshieldError::VaultLocked);
            }
            unlocked.last_used = Instant::now();
            return Ok(unlocked.key.clone());
        }
        Err(SshieldError::VaultLocked)
    }

    fn read_file(&self) -> Result<VaultFile> {
        let raw = std::fs::read_to_string(&self.path)
            .map_err(|e| SshieldError::Vault(format!("Failed to read vault file: {e}")))?;
        serde_json::from_str(&raw)
            .map_err(|e| SshieldError::Vault(format!("Failed to parse vault file: {e}")))
    }

    fn write_file(&self, file: &VaultFile) -> Result<()> {
        let json = serde_json::to_string_pretty(file)
            .map_err(|e| SshieldError::Vault(format!("Failed to serialize vault: {e}")))?;
        std::fs::write(&self.path, json)
            .map_err(|e| SshieldError::Vault(format!("Failed to write vault file: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

fn derive_master_key(passphrase: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut out = Zeroizing::new([0u8; 32]);
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut *out)
        .map_err(|e| SshieldError::Vault(format!("Argon2id key derivation failed: {e}")))?;
    Ok(out)
}

fn mask_preview(secret: &str, kind: VaultCredentialKind) -> String {
    match kind {
        VaultCredentialKind::Password => "••••••••".to_string(),
        VaultCredentialKind::PrivateKey => {
            format!("PEM Key ({} bytes)", secret.len())
        }
        VaultCredentialKind::ApiKey => {
            let chars: Vec<char> = secret.chars().collect();
            if chars.len() <= 8 {
                "***".to_string()
            } else {
                let head: String = chars.iter().take(4).collect();
                let tail: String = chars.iter().skip(chars.len() - 4).collect();
                format!("{head}***{tail}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn encrypt_decrypt_lock_and_tamper_detection() {
        let dir = std::env::temp_dir().join(format!("sshield-vault-test-{}", Uuid::new_v4()));
        let vault_path = dir.join("vault.enc");
        let vault = SealedVault::new(&vault_path, 600);

        assert_eq!(vault.status().await.unwrap().state, VaultLockState::Uninitialized);
        vault.initialize("correct-horse-battery-staple").await.unwrap();
        assert_eq!(vault.status().await.unwrap().state, VaultLockState::Unlocked);

        let meta = vault
            .store_secret(
                None,
                "OpenAI Key".into(),
                VaultCredentialKind::ApiKey,
                None,
                "sk-proj-1234567890abcdef",
            )
            .await
            .unwrap();
        assert_eq!(meta.preview, "sk-p***cdef");

        let secret = vault.get_secret(&meta.id).await.unwrap();
        assert_eq!(&*secret, "sk-proj-1234567890abcdef");

        // Lock clears in-memory key
        vault.lock().await;
        assert_eq!(vault.status().await.unwrap().state, VaultLockState::Locked);
        assert!(matches!(
            vault.get_secret(&meta.id).await,
            Err(SshieldError::VaultLocked)
        ));

        // Wrong passphrase fails
        assert!(vault.unlock("wrong-passphrase").await.is_err());

        // Right passphrase succeeds
        vault.unlock("correct-horse-battery-staple").await.unwrap();
        assert_eq!(&*vault.get_secret(&meta.id).await.unwrap(), "sk-proj-1234567890abcdef");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
