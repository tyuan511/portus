use std::collections::HashSet;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use data_encoding::BASE64;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::settings::{HostProfile, PortForwardRule, Settings, Snippet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultPayload {
    pub hosts: Vec<HostProfile>,
    pub snippets: Vec<Snippet>,
    pub tunnels: Vec<PortForwardRule>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct EncryptedVaultFile {
    version: u8,
    salt: String,
    nonce: String,
    ciphertext: String,
}

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Key derivation failed: {e}"))?;
    Ok(key)
}

pub fn export_vault(settings: &Settings, passphrase: &str) -> Result<Vec<u8>, String> {
    if passphrase.is_empty() {
        return Err("Enter a master passphrase to encrypt the vault.".into());
    }
    let payload = VaultPayload {
        hosts: settings.hosts.clone(),
        snippets: settings.snippets.clone(),
        tunnels: settings.tunnels.clone(),
    };
    let plaintext = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;

    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce_bytes);

    let key = derive_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).map_err(|e| e.to_string())?;
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|e| format!("Encryption failed: {e}"))?;

    let envelope = EncryptedVaultFile {
        version: 1,
        salt: BASE64.encode(&salt),
        nonce: BASE64.encode(&nonce_bytes),
        ciphertext: BASE64.encode(&ciphertext),
    };
    serde_json::to_vec_pretty(&envelope).map_err(|e| e.to_string())
}

pub fn import_vault(bytes: &[u8], passphrase: &str) -> Result<VaultPayload, String> {
    if passphrase.is_empty() {
        return Err("Enter the vault master passphrase.".into());
    }
    let envelope: EncryptedVaultFile =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid vault file: {e}"))?;
    if envelope.version != 1 {
        return Err(format!("Unsupported vault version: {}", envelope.version));
    }
    let salt = BASE64
        .decode(envelope.salt.as_bytes())
        .map_err(|e| format!("Invalid vault salt: {e}"))?;
    let nonce_bytes = BASE64
        .decode(envelope.nonce.as_bytes())
        .map_err(|e| format!("Invalid vault nonce: {e}"))?;
    let ciphertext = BASE64
        .decode(envelope.ciphertext.as_bytes())
        .map_err(|e| format!("Invalid vault ciphertext: {e}"))?;

    if nonce_bytes.len() != 12 {
        return Err("Invalid vault nonce length.".into());
    }
    let key = derive_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).map_err(|e| e.to_string())?;
    let plaintext = cipher
        .decrypt(&nonce, ciphertext.as_ref())
        .map_err(|_| "Incorrect passphrase or corrupted vault file.".to_string())?;
    serde_json::from_slice(&plaintext).map_err(|e| format!("Invalid vault payload: {e}"))
}

pub fn merge_vault_into(settings: &mut Settings, payload: VaultPayload) -> (usize, usize, usize) {
    let mut host_names: HashSet<String> = settings
        .hosts
        .iter()
        .map(|h| h.name.to_lowercase())
        .collect();
    let mut added_hosts = 0;
    for mut host in payload.hosts {
        if host_names.insert(host.name.to_lowercase())
            && let Some(ids) = settings.host_ids(1)
        {
            host.id = ids.start;
            settings.next_host_id = ids.end;
            settings.hosts.push(host);
            added_hosts += 1;
        }
    }

    let mut snippet_names: HashSet<String> = settings
        .snippets
        .iter()
        .map(|s| s.name.to_lowercase())
        .collect();
    let mut added_snippets = 0;
    for mut snippet in payload.snippets {
        if snippet_names.insert(snippet.name.to_lowercase())
            && let Some(ids) = settings.snippet_ids(1)
        {
            snippet.id = ids.start;
            settings.next_snippet_id = ids.end;
            settings.snippets.push(snippet);
            added_snippets += 1;
        }
    }

    let mut tunnel_names: HashSet<String> = settings
        .tunnels
        .iter()
        .map(|t| t.name.to_lowercase())
        .collect();
    let mut added_tunnels = 0;
    for mut rule in payload.tunnels {
        if tunnel_names.insert(rule.name.to_lowercase())
            && let Some(ids) = settings.tunnel_ids(1)
        {
            rule.id = ids.start;
            settings.next_tunnel_id = ids.end;
            settings.tunnels.push(rule);
            added_tunnels += 1;
        }
    }

    (added_hosts, added_snippets, added_tunnels)
}

const SECRET_PREFIX: &str = "enc:v1:";

fn local_secret_key_path() -> Option<PathBuf> {
    dirs::config_dir().map(|parent| parent.join("Bulwark").join(".host_key"))
}

fn load_or_create_local_key() -> Result<[u8; 32], String> {
    let path = local_secret_key_path()
        .ok_or_else(|| "Could not locate application config directory.".to_string())?;
    if let Ok(bytes) = fs::read(&path)
        && bytes.len() == 32
    {
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create key directory: {e}"))?;
    }
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    let mut file = fs::File::create(&path).map_err(|e| format!("Failed to create key file: {e}"))?;
    file.write_all(&key)
        .map_err(|e| format!("Failed to write key file: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

pub fn encrypt_secret_with_key(plaintext: &str, key: &[u8; 32]) -> Result<String, String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).map_err(|e| e.to_string())?;
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| format!("Secret encryption failed: {e}"))?;
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(format!("{SECRET_PREFIX}{}", BASE64.encode(&combined)))
}

pub fn decrypt_secret_with_key(stored: &str, key: &[u8; 32]) -> Option<String> {
    if stored.is_empty() {
        return None;
    }
    let Some(encoded) = stored.strip_prefix(SECRET_PREFIX) else {
        return Some(stored.to_owned());
    };
    let raw = BASE64.decode(encoded.as_bytes()).ok()?;
    if raw.len() <= 12 {
        return None;
    }
    let (nonce_bytes, ciphertext) = raw.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let nonce = Nonce::try_from(nonce_bytes).ok()?;
    let plaintext = cipher.decrypt(&nonce, ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}

pub fn encrypt_secret(plaintext: &str) -> Result<String, String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }
    let key = load_or_create_local_key()?;
    encrypt_secret_with_key(plaintext, &key)
}

pub fn decrypt_secret(stored: &str) -> Option<String> {
    if stored.is_empty() {
        return None;
    }
    if !stored.starts_with(SECRET_PREFIX) {
        return Some(stored.to_owned());
    }
    let key = load_or_create_local_key().ok()?;
    decrypt_secret_with_key(stored, &key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{HostIcon, HostProtocol};

    #[test]
    fn encrypts_and_decrypts_host_secret_roundtrip() {
        let key = [0x42u8; 32];
        let encrypted = encrypt_secret_with_key("s3cr3t-pw!", &key).unwrap();
        assert!(encrypted.starts_with(SECRET_PREFIX));
        assert_ne!(encrypted, "s3cr3t-pw!");
        assert_eq!(
            decrypt_secret_with_key(&encrypted, &key).as_deref(),
            Some("s3cr3t-pw!")
        );
        assert_eq!(decrypt_secret_with_key(&encrypted, &[0x99u8; 32]), None);
        assert_eq!(encrypt_secret_with_key("", &key).unwrap(), "");
        assert_eq!(decrypt_secret_with_key("", &key), None);
    }

    #[test]
    fn encrypts_and_decrypts_vault_roundtrip() {
        let mut settings = Settings::default();
        settings.hosts.push(HostProfile {
            id: 1,
            name: "Edge".into(),
            protocol: HostProtocol::Ssh,
            icon: HostIcon::Server,
            address: "edge.example.com".into(),
            username: "ops".into(),
            port: 22,
            baud_rate: 115200,
            identity_file: String::new(),
            password: String::new(),
            jump_host: String::new(),
            jump_host_id: None,
            jump_relay_command: String::new(),
            jump_relay_password: String::new(),
            group: "Prod".into(),
            tags: vec!["edge".into()],
            last_connected: None,
        });

        let sealed = export_vault(&settings, "correct-horse-battery").unwrap();
        assert!(import_vault(&sealed, "wrong-pass").is_err());

        let opened = import_vault(&sealed, "correct-horse-battery").unwrap();
        assert_eq!(opened.hosts.len(), 1);
        assert_eq!(opened.hosts[0].name, "Edge");

        let mut target = Settings::default();
        let (h, s, t) = merge_vault_into(&mut target, opened);
        assert_eq!((h, s, t), (1, 0, 0));
        assert_eq!(target.hosts[0].name, "Edge");
    }
}
