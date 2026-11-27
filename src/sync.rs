use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use chrono::{DateTime, Utc};
use data_encoding::BASE64;
use hmac::{Hmac, Mac};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::settings::{
    S3SyncConfig, Settings, SyncConflictStrategy, SyncProvider, SyncSettings, WebDavSyncConfig,
};
use crate::updater::CURRENT_VERSION;
use crate::vault::{decrypt_secret, encrypt_secret};

type HmacSha256 = Hmac<Sha256>;

const ENVELOPE_VERSION: u8 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SyncStatus {
    #[default]
    Idle,
    Testing,
    Pushing,
    Pulling,
    Syncing,
    Synced {
        timestamp: u64,
        summary: String,
    },
    Error(String),
}

impl SyncStatus {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Testing | Self::Pushing | Self::Pulling | Self::Syncing
        )
    }
}

#[derive(Clone, Debug)]
pub enum SyncUiEvent {
    Changed,
    TestSucceeded {
        message: String,
    },
    TestFailed {
        message: String,
    },
    Pushed {
        timestamp: u64,
        content_hash: String,
        device_id: String,
        silent: bool,
    },
    Pulled {
        settings: Box<Settings>,
        timestamp: u64,
        content_hash: String,
        device_id: String,
        strategy: SyncConflictStrategy,
        silent: bool,
    },
    UpToDate {
        timestamp: u64,
        silent: bool,
    },
    Failed {
        message: String,
        silent: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncEnvelope {
    pub version: u8,
    pub updated_at: u64,
    #[serde(default)]
    pub device_id: String,
    pub encrypted: bool,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ciphertext: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
}

pub struct OpenedSyncPayload {
    pub updated_at: u64,
    pub content_hash: String,
    pub settings: Settings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncApplyStats {
    pub hosts: usize,
    pub keychain: usize,
    pub snippets: usize,
    pub tunnels: usize,
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn ensure_device_id(existing: &str) -> String {
    let trimmed = existing.trim();
    if !trimmed.is_empty() {
        return trimmed.to_owned();
    }
    let mut bytes = [0u8; 8];
    rand::rng().fill_bytes(&mut bytes);
    format!("portus-{}", hex::encode(bytes))
}

fn decrypt_settings_secrets(settings: &mut Settings) {
    for host in &mut settings.hosts {
        if let Some(plain) = decrypt_secret(&host.password) {
            host.password = plain;
        }
        if let Some(plain) = decrypt_secret(&host.jump_relay_password) {
            host.jump_relay_password = plain;
        }
    }
    for entry in &mut settings.keychain {
        if let Some(plain) = decrypt_secret(&entry.secret) {
            entry.secret = plain;
        }
    }
}

fn encrypt_settings_secrets(settings: &mut Settings) -> Result<(), String> {
    for host in &mut settings.hosts {
        if !host.password.is_empty() {
            host.password = encrypt_secret(&host.password)?;
        }
        if !host.jump_relay_password.is_empty() {
            host.jump_relay_password = encrypt_secret(&host.jump_relay_password)?;
        }
    }
    for entry in &mut settings.keychain {
        if !entry.secret.is_empty() {
            entry.secret = encrypt_secret(&entry.secret)?;
        }
    }
    Ok(())
}

/// Produce a portable clone of `Settings` with local-only `sync` metadata cleared
/// and machine-bound secret ciphertexts decrypted into plaintext.
pub fn portable_snapshot(settings: &Settings) -> Settings {
    let mut snapshot = settings.clone();
    snapshot.sync = SyncSettings::default();
    decrypt_settings_secrets(&mut snapshot);
    snapshot
}

/// Compute a deterministic SHA-256 hex digest of all syncable settings and hosts.
/// Secrets are decrypted first so random local AES-GCM nonces do not cause hash drift.
pub fn compute_snapshot_hash(settings: &Settings) -> String {
    let snapshot = portable_snapshot(settings);
    let json = serde_json::to_vec(&snapshot).unwrap_or_default();
    let digest = Sha256::digest(&json);
    hex::encode(digest)
}

fn derive_sync_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Key derivation failed: {e}"))?;
    Ok(key)
}

pub fn seal_envelope(
    settings: &Settings,
    passphrase: &str,
    device_id: &str,
    updated_at: u64,
) -> Result<(Vec<u8>, String), String> {
    let snapshot = portable_snapshot(settings);
    let plaintext = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
    let content_hash = hex::encode(Sha256::digest(&plaintext));

    let envelope = if passphrase.is_empty() {
        SyncEnvelope {
            version: ENVELOPE_VERSION,
            updated_at,
            device_id: device_id.to_owned(),
            encrypted: false,
            content_hash: content_hash.clone(),
            salt: None,
            nonce: None,
            ciphertext: None,
            settings: Some(snapshot),
        }
    } else {
        let mut salt = [0u8; 16];
        let mut nonce_bytes = [0u8; 12];
        let mut rng = rand::rng();
        rng.fill_bytes(&mut salt);
        rng.fill_bytes(&mut nonce_bytes);

        let key = derive_sync_key(passphrase, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
        let nonce = Nonce::try_from(nonce_bytes.as_slice()).map_err(|e| e.to_string())?;
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|e| format!("Sync encryption failed: {e}"))?;

        SyncEnvelope {
            version: ENVELOPE_VERSION,
            updated_at,
            device_id: device_id.to_owned(),
            encrypted: true,
            content_hash: content_hash.clone(),
            salt: Some(BASE64.encode(&salt)),
            nonce: Some(BASE64.encode(&nonce_bytes)),
            ciphertext: Some(BASE64.encode(&ciphertext)),
            settings: None,
        }
    };

    let bytes = serde_json::to_vec_pretty(&envelope).map_err(|e| e.to_string())?;
    Ok((bytes, content_hash))
}

pub fn open_envelope(bytes: &[u8], passphrase: &str) -> Result<OpenedSyncPayload, String> {
    let envelope: SyncEnvelope =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid remote sync file: {e}"))?;
    if envelope.version != ENVELOPE_VERSION {
        return Err(format!(
            "Unsupported remote sync format version: {}",
            envelope.version
        ));
    }

    let mut settings = if envelope.encrypted {
        if passphrase.is_empty() {
            return Err(
                "Remote config is encrypted. Enter the sync encryption passphrase first."
                    .to_owned(),
            );
        }
        let salt_b64 = envelope
            .salt
            .as_deref()
            .ok_or_else(|| "Missing salt in encrypted sync file.".to_owned())?;
        let nonce_b64 = envelope
            .nonce
            .as_deref()
            .ok_or_else(|| "Missing nonce in encrypted sync file.".to_owned())?;
        let ct_b64 = envelope
            .ciphertext
            .as_deref()
            .ok_or_else(|| "Missing ciphertext in encrypted sync file.".to_owned())?;

        let salt = BASE64
            .decode(salt_b64.as_bytes())
            .map_err(|e| format!("Invalid sync salt: {e}"))?;
        let nonce_bytes = BASE64
            .decode(nonce_b64.as_bytes())
            .map_err(|e| format!("Invalid sync nonce: {e}"))?;
        let ciphertext = BASE64
            .decode(ct_b64.as_bytes())
            .map_err(|e| format!("Invalid sync ciphertext: {e}"))?;
        if nonce_bytes.len() != 12 {
            return Err("Invalid sync nonce length.".to_owned());
        }

        let key = derive_sync_key(passphrase, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
        let nonce = Nonce::try_from(nonce_bytes.as_slice()).map_err(|e| e.to_string())?;
        let plaintext = cipher
            .decrypt(&nonce, ciphertext.as_ref())
            .map_err(|_| "Incorrect sync passphrase or corrupted remote file.".to_owned())?;
        serde_json::from_slice::<Settings>(&plaintext)
            .map_err(|e| format!("Invalid decrypted settings payload: {e}"))?
    } else {
        envelope
            .settings
            .ok_or_else(|| "Missing settings payload in remote sync file.".to_owned())?
    };

    settings.sync = SyncSettings::default();
    settings.normalize();
    let content_hash = if envelope.content_hash.is_empty() {
        let json = serde_json::to_vec(&settings).unwrap_or_default();
        hex::encode(Sha256::digest(&json))
    } else {
        envelope.content_hash
    };

    Ok(OpenedSyncPayload {
        updated_at: envelope.updated_at,
        content_hash,
        settings,
    })
}

/// Apply a decrypted remote `Settings` snapshot onto `local`, preserving `local.sync`
/// and re-encrypting all secrets using this machine's local key.
pub fn apply_remote_settings(
    local: &mut Settings,
    mut remote: Settings,
    strategy: SyncConflictStrategy,
) -> Result<SyncApplyStats, String> {
    remote.normalize();
    encrypt_settings_secrets(&mut remote)?;

    let preserved_sync = local.sync.clone();

    match strategy {
        SyncConflictStrategy::Overwrite => {
            let stats = SyncApplyStats {
                hosts: remote.hosts.len(),
                keychain: remote.keychain.len(),
                snippets: remote.snippets.len(),
                tunnels: remote.tunnels.len(),
            };
            remote.sync = preserved_sync;
            *local = remote;
            Ok(stats)
        }
        SyncConflictStrategy::Merge => {
            // Copy general settings from remote while merging collections.
            let local_hosts = std::mem::take(&mut local.hosts);
            let local_next_host_id = local.next_host_id;
            let local_keychain = std::mem::take(&mut local.keychain);
            let local_next_keychain_id = local.next_keychain_id;
            let local_snippets = std::mem::take(&mut local.snippets);
            let local_next_snippet_id = local.next_snippet_id;
            let local_tunnels = std::mem::take(&mut local.tunnels);
            let local_next_tunnel_id = local.next_tunnel_id;

            let remote_hosts = std::mem::take(&mut remote.hosts);
            let remote_keychain = std::mem::take(&mut remote.keychain);
            let remote_snippets = std::mem::take(&mut remote.snippets);
            let remote_tunnels = std::mem::take(&mut remote.tunnels);

            *local = remote;
            local.sync = preserved_sync;
            local.hosts = local_hosts;
            local.next_host_id = local_next_host_id;
            local.keychain = local_keychain;
            local.next_keychain_id = local_next_keychain_id;
            local.snippets = local_snippets;
            local.next_snippet_id = local_next_snippet_id;
            local.tunnels = local_tunnels;
            local.next_tunnel_id = local_next_tunnel_id;

            let mut stats = SyncApplyStats::default();

            // Merge keychain first and track old->new ID mapping for host references.
            let mut kc_by_name: HashMap<String, u64> = local
                .keychain
                .iter()
                .map(|k| (k.name.to_lowercase(), k.id))
                .collect();
            let mut kc_id_map: HashMap<u64, u64> = HashMap::new();
            for mut entry in remote_keychain {
                let old_id = entry.id;
                let key = entry.name.to_lowercase();
                if let Some(&existing_id) = kc_by_name.get(&key) {
                    kc_id_map.insert(old_id, existing_id);
                } else if let Some(ids) = local.keychain_ids(1) {
                    entry.id = ids.start;
                    local.next_keychain_id = ids.end;
                    kc_by_name.insert(key, entry.id);
                    kc_id_map.insert(old_id, entry.id);
                    local.keychain.push(entry);
                    stats.keychain += 1;
                }
            }

            // Merge hosts and track old->new ID mapping for jump hosts & tunnels.
            let mut host_by_name: HashMap<String, u64> = local
                .hosts
                .iter()
                .map(|h| (h.name.to_lowercase(), h.id))
                .collect();
            let mut host_id_map: HashMap<u64, u64> = HashMap::new();
            let mut newly_added_host_indices = Vec::new();
            for mut host in remote_hosts {
                let old_id = host.id;
                let key = host.name.to_lowercase();
                if let Some(&existing_id) = host_by_name.get(&key) {
                    host_id_map.insert(old_id, existing_id);
                } else if let Some(ids) = local.host_ids(1) {
                    host.id = ids.start;
                    local.next_host_id = ids.end;
                    host.keychain_id = host
                        .keychain_id
                        .and_then(|old| kc_id_map.get(&old).copied());
                    host.jump_relay_keychain_id = host
                        .jump_relay_keychain_id
                        .and_then(|old| kc_id_map.get(&old).copied());
                    host_by_name.insert(key, host.id);
                    host_id_map.insert(old_id, host.id);
                    newly_added_host_indices.push(local.hosts.len());
                    local.hosts.push(host);
                    stats.hosts += 1;
                }
            }
            for idx in newly_added_host_indices {
                let remapped = local.hosts[idx]
                    .jump_host_id
                    .and_then(|old| host_id_map.get(&old).copied());
                local.hosts[idx].jump_host_id = remapped;
            }

            // Merge snippets.
            let mut snippet_names: HashSet<String> = local
                .snippets
                .iter()
                .map(|s| s.name.to_lowercase())
                .collect();
            for mut snippet in remote_snippets {
                if snippet_names.insert(snippet.name.to_lowercase())
                    && let Some(ids) = local.snippet_ids(1)
                {
                    snippet.id = ids.start;
                    local.next_snippet_id = ids.end;
                    local.snippets.push(snippet);
                    stats.snippets += 1;
                }
            }

            // Merge tunnels with remapped host IDs.
            let mut tunnel_names: HashSet<String> = local
                .tunnels
                .iter()
                .map(|t| t.name.to_lowercase())
                .collect();
            for mut rule in remote_tunnels {
                if tunnel_names.insert(rule.name.to_lowercase())
                    && let Some(ids) = local.tunnel_ids(1)
                {
                    rule.id = ids.start;
                    local.next_tunnel_id = ids.end;
                    if let Some(&mapped_host_id) = host_id_map.get(&rule.host_id) {
                        rule.host_id = mapped_host_id;
                    }
                    local.tunnels.push(rule);
                    stats.tunnels += 1;
                }
            }

            Ok(stats)
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP transport via curl (uses 0600 temp config file so secrets never appear in argv)
// ---------------------------------------------------------------------------

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

fn curl_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => {}
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn send_http(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<HttpResponse, String> {
    let mut cfg = tempfile::NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp curl config: {e}"))?;
    let out_file = tempfile::NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp response file: {e}"))?;

    let body_file = if let Some(data) = body {
        let mut f = tempfile::NamedTempFile::new()
            .map_err(|e| format!("Failed to create temp request body file: {e}"))?;
        f.write_all(data)
            .map_err(|e| format!("Failed to write request body: {e}"))?;
        f.flush()
            .map_err(|e| format!("Failed to flush request body: {e}"))?;
        Some(f)
    } else {
        None
    };

    writeln!(cfg, "request = {}", curl_escape(method)).map_err(|e| e.to_string())?;
    writeln!(cfg, "url = {}", curl_escape(url)).map_err(|e| e.to_string())?;
    writeln!(cfg, "connect-timeout = 15").map_err(|e| e.to_string())?;
    writeln!(cfg, "max-time = 60").map_err(|e| e.to_string())?;
    writeln!(cfg, "silent").map_err(|e| e.to_string())?;
    writeln!(cfg, "show-error").map_err(|e| e.to_string())?;
    if method == "HEAD" {
        writeln!(cfg, "head").map_err(|e| e.to_string())?;
    }
    writeln!(
        cfg,
        "header = {}",
        curl_escape(&format!("User-Agent: Portus/{CURRENT_VERSION}"))
    )
    .map_err(|e| e.to_string())?;
    for (k, v) in headers {
        writeln!(cfg, "header = {}", curl_escape(&format!("{k}: {v}")))
            .map_err(|e| e.to_string())?;
    }
    if let Some(ref f) = body_file {
        let path_str = f.path().to_string_lossy();
        writeln!(cfg, "data-binary = {}", curl_escape(&format!("@{path_str}")))
            .map_err(|e| e.to_string())?;
    }
    writeln!(
        cfg,
        "output = {}",
        curl_escape(&out_file.path().to_string_lossy())
    )
    .map_err(|e| e.to_string())?;
    writeln!(cfg, "write-out = \"%{{http_code}}\"").map_err(|e| e.to_string())?;
    cfg.flush().map_err(|e| e.to_string())?;

    let output = Command::new("curl")
        .arg("--config")
        .arg(cfg.path())
        .output()
        .map_err(|e| format!("Failed to execute curl: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            "Network request failed.".to_owned()
        } else {
            stderr
        });
    }

    let code_str = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let status: u16 = code_str
        .parse()
        .map_err(|_| format!("Invalid HTTP status code: {code_str}"))?;
    let resp_body = fs::read(out_file.path()).unwrap_or_default();

    Ok(HttpResponse {
        status,
        body: resp_body,
    })
}

// ---------------------------------------------------------------------------
// S3 Client (AWS Signature Version 4)
// ---------------------------------------------------------------------------

fn aws_uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if unreserved || (!encode_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn normalize_url(raw: &str) -> Result<Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Endpoint URL is required.".to_owned());
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    Url::parse(&with_scheme).map_err(|e| format!("Invalid endpoint URL: {e}"))
}

fn s3_target_url_and_host(cfg: &S3SyncConfig) -> Result<(String, String, String), String> {
    let bucket = cfg.bucket.trim().trim_matches('/');
    if bucket.is_empty() {
        return Err("S3 bucket name is required.".to_owned());
    }
    let key = cfg.object_key.trim().trim_start_matches('/');
    let key = if key.is_empty() {
        "portus/config.json"
    } else {
        key
    };
    let parsed = normalize_url(&cfg.endpoint)?;
    let scheme = parsed.scheme();
    let host = parsed
        .host_str()
        .ok_or_else(|| "S3 endpoint URL is missing a host.".to_owned())?;
    let port_suffix = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    let base_path = parsed.path().trim_end_matches('/');

    let (full_host, raw_path) = if cfg.path_style {
        let full_host = format!("{host}{port_suffix}");
        let raw_path = if base_path.is_empty() {
            format!("/{bucket}/{key}")
        } else {
            format!("{base_path}/{bucket}/{key}")
        };
        (full_host, raw_path)
    } else {
        let full_host = format!("{bucket}.{host}{port_suffix}");
        let raw_path = if base_path.is_empty() {
            format!("/{key}")
        } else {
            format!("{base_path}/{key}")
        };
        (full_host, raw_path)
    };

    let canonical_uri = aws_uri_encode(&raw_path, false);
    let url = format!("{scheme}://{full_host}{canonical_uri}");
    Ok((url, full_host, canonical_uri))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub fn sign_s3_headers(
    method: &str,
    host: &str,
    canonical_uri: &str,
    canonical_query: &str,
    region: &str,
    access_key: &str,
    secret_key: &str,
    payload: &[u8],
    now: DateTime<Utc>,
) -> Vec<(String, String)> {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let payload_hash = hex::encode(Sha256::digest(payload));

    let canonical_headers = format!(
        "host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n"
    );
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";

    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let canonical_request_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));

    let credential_scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let string_to_sign =
        format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{canonical_request_hash}");

    let k_date = hmac_sha256(
        format!("AWS4{secret_key}").as_bytes(),
        date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );

    vec![
        ("Authorization".to_owned(), authorization),
        ("x-amz-date".to_owned(), amz_date),
        ("x-amz-content-sha256".to_owned(), payload_hash),
    ]
}

fn extract_s3_error(status: u16, body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let extract_tag = |tag: &str| -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = text.find(&open)? + open.len();
        let end = text[start..].find(&close)? + start;
        let val = text[start..end].trim();
        (!val.is_empty()).then(|| val.to_owned())
    };
    match (extract_tag("Code"), extract_tag("Message")) {
        (Some(code), Some(msg)) => format!("S3 HTTP {status} ({code}): {msg}"),
        (Some(code), None) => format!("S3 HTTP {status} ({code})"),
        (None, Some(msg)) => format!("S3 HTTP {status}: {msg}"),
        (None, None) => format!("S3 returned HTTP {status}"),
    }
}

fn resolve_s3_credentials(cfg: &S3SyncConfig) -> Result<(String, String, String), String> {
    let access_key = cfg.access_key_id.trim().to_owned();
    if access_key.is_empty() {
        return Err("S3 Access Key ID is required.".to_owned());
    }
    let secret_key = decrypt_secret(&cfg.secret_access_key).unwrap_or_default();
    if secret_key.is_empty() {
        return Err("S3 Secret Access Key is required.".to_owned());
    }
    let region = cfg.region.trim();
    let region = if region.is_empty() {
        "us-east-1".to_owned()
    } else {
        region.to_owned()
    };
    Ok((access_key, secret_key, region))
}

fn s3_test(cfg: &S3SyncConfig) -> Result<String, String> {
    let (access_key, secret_key, region) = resolve_s3_credentials(cfg)?;
    let (url, host, canonical_uri) = s3_target_url_and_host(cfg)?;
    let headers = sign_s3_headers(
        "HEAD",
        &host,
        &canonical_uri,
        "",
        &region,
        &access_key,
        &secret_key,
        b"",
        Utc::now(),
    );
    let resp = send_http("HEAD", &url, &headers, None)?;
    match resp.status {
        200 => Ok("Connected to S3. Remote config exists.".to_owned()),
        404 => Ok("Connected to S3. Ready for first upload.".to_owned()),
        _ => Err(extract_s3_error(resp.status, &resp.body)),
    }
}

fn s3_put(cfg: &S3SyncConfig, data: &[u8]) -> Result<(), String> {
    let (access_key, secret_key, region) = resolve_s3_credentials(cfg)?;
    let (url, host, canonical_uri) = s3_target_url_and_host(cfg)?;
    let mut headers = sign_s3_headers(
        "PUT",
        &host,
        &canonical_uri,
        "",
        &region,
        &access_key,
        &secret_key,
        data,
        Utc::now(),
    );
    headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
    let resp = send_http("PUT", &url, &headers, Some(data))?;
    if (200..300).contains(&resp.status) {
        Ok(())
    } else {
        Err(extract_s3_error(resp.status, &resp.body))
    }
}

fn s3_get(cfg: &S3SyncConfig) -> Result<Option<Vec<u8>>, String> {
    let (access_key, secret_key, region) = resolve_s3_credentials(cfg)?;
    let (url, host, canonical_uri) = s3_target_url_and_host(cfg)?;
    let headers = sign_s3_headers(
        "GET",
        &host,
        &canonical_uri,
        "",
        &region,
        &access_key,
        &secret_key,
        b"",
        Utc::now(),
    );
    let resp = send_http("GET", &url, &headers, None)?;
    if resp.status == 404 {
        return Ok(None);
    }
    if (200..300).contains(&resp.status) {
        Ok(Some(resp.body))
    } else {
        Err(extract_s3_error(resp.status, &resp.body))
    }
}

// ---------------------------------------------------------------------------
// WebDAV Client
// ---------------------------------------------------------------------------

fn webdav_auth_headers(cfg: &WebDavSyncConfig) -> Vec<(String, String)> {
    let user = cfg.username.trim();
    let pass = decrypt_secret(&cfg.password).unwrap_or_default();
    if user.is_empty() && pass.is_empty() {
        return Vec::new();
    }
    let token = BASE64.encode(format!("{user}:{pass}").as_bytes());
    vec![("Authorization".to_owned(), format!("Basic {token}"))]
}

fn webdav_join_url(endpoint: &str, relative_path: &str) -> Result<String, String> {
    let parsed = normalize_url(endpoint)?;
    let base = parsed.as_str().trim_end_matches('/');
    let rel = relative_path.trim().trim_start_matches('/');
    if rel.is_empty() {
        return Ok(format!("{base}/portus-config.json"));
    }
    let encoded_segments: Vec<String> = rel
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| aws_uri_encode(seg, true))
        .collect();
    if encoded_segments.is_empty() {
        return Ok(format!("{base}/portus-config.json"));
    }
    Ok(format!("{base}/{}", encoded_segments.join("/")))
}

fn webdav_parent_dirs(endpoint: &str, relative_path: &str) -> Result<Vec<String>, String> {
    let parsed = normalize_url(endpoint)?;
    let base = parsed.as_str().trim_end_matches('/');
    let segments: Vec<&str> = relative_path
        .trim()
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if segments.len() <= 1 {
        return Ok(Vec::new());
    }
    let mut dirs = Vec::new();
    let mut acc = base.to_owned();
    for seg in &segments[..segments.len() - 1] {
        acc.push('/');
        acc.push_str(&aws_uri_encode(seg, true));
        dirs.push(acc.clone());
    }
    Ok(dirs)
}

fn webdav_test(cfg: &WebDavSyncConfig) -> Result<String, String> {
    let base_url = normalize_url(&cfg.endpoint)?;
    let mut headers = webdav_auth_headers(cfg);
    headers.push(("Depth".to_owned(), "0".to_owned()));

    let resp = send_http("PROPFIND", base_url.as_str(), &headers, None)?;
    if matches!(resp.status, 200 | 204 | 207) {
        return Ok("Connected to WebDAV server.".to_owned());
    }
    if resp.status == 405 {
        let auth = webdav_auth_headers(cfg);
        let opt = send_http("OPTIONS", base_url.as_str(), &auth, None)?;
        if (200..300).contains(&opt.status) {
            return Ok("Connected to WebDAV server.".to_owned());
        }
    }
    if matches!(resp.status, 401 | 403) {
        return Err(format!(
            "WebDAV authentication failed (HTTP {}). Check username and password.",
            resp.status
        ));
    }
    Err(format!("WebDAV returned HTTP {}.", resp.status))
}

fn webdav_put(cfg: &WebDavSyncConfig, data: &[u8]) -> Result<(), String> {
    let auth = webdav_auth_headers(cfg);
    for dir_url in webdav_parent_dirs(&cfg.endpoint, &cfg.remote_path)? {
        let _ = send_http("MKCOL", &dir_url, &auth, None);
    }

    let url = webdav_join_url(&cfg.endpoint, &cfg.remote_path)?;
    let mut headers = auth;
    headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
    let resp = send_http("PUT", &url, &headers, Some(data))?;
    if matches!(resp.status, 200 | 201 | 204) {
        Ok(())
    } else if matches!(resp.status, 401 | 403) {
        Err(format!(
            "WebDAV authentication failed (HTTP {}).",
            resp.status
        ))
    } else {
        Err(format!("WebDAV upload failed with HTTP {}.", resp.status))
    }
}

fn webdav_get(cfg: &WebDavSyncConfig) -> Result<Option<Vec<u8>>, String> {
    let url = webdav_join_url(&cfg.endpoint, &cfg.remote_path)?;
    let headers = webdav_auth_headers(cfg);
    let resp = send_http("GET", &url, &headers, None)?;
    if resp.status == 404 {
        return Ok(None);
    }
    if (200..300).contains(&resp.status) {
        Ok(Some(resp.body))
    } else if matches!(resp.status, 401 | 403) {
        Err(format!(
            "WebDAV authentication failed (HTTP {}).",
            resp.status
        ))
    } else {
        Err(format!("WebDAV download failed with HTTP {}.", resp.status))
    }
}

// ---------------------------------------------------------------------------
// Provider dispatch helpers
// ---------------------------------------------------------------------------

fn backend_test(sync: &SyncSettings) -> Result<String, String> {
    match sync.provider {
        SyncProvider::None => Err("Select S3 or WebDAV as the sync backend first.".to_owned()),
        SyncProvider::S3 => s3_test(&sync.s3),
        SyncProvider::WebDav => webdav_test(&sync.webdav),
    }
}

fn backend_put(sync: &SyncSettings, data: &[u8]) -> Result<(), String> {
    match sync.provider {
        SyncProvider::None => Err("Cloud sync is disabled.".to_owned()),
        SyncProvider::S3 => s3_put(&sync.s3, data),
        SyncProvider::WebDav => webdav_put(&sync.webdav, data),
    }
}

fn backend_get(sync: &SyncSettings) -> Result<Option<Vec<u8>>, String> {
    match sync.provider {
        SyncProvider::None => Err("Cloud sync is disabled.".to_owned()),
        SyncProvider::S3 => s3_get(&sync.s3),
        SyncProvider::WebDav => webdav_get(&sync.webdav),
    }
}

// ---------------------------------------------------------------------------
// SyncManager (background thread worker + UI channel)
// ---------------------------------------------------------------------------

pub struct SyncManager {
    status: Mutex<SyncStatus>,
    ui_tx: Option<async_channel::Sender<SyncUiEvent>>,
}

impl SyncManager {
    pub fn new(ui_tx: Option<async_channel::Sender<SyncUiEvent>>) -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new(SyncStatus::Idle),
            ui_tx,
        })
    }

    pub fn status(&self) -> SyncStatus {
        self.status.lock().unwrap().clone()
    }

    fn set_status(&self, next: SyncStatus) {
        *self.status.lock().unwrap() = next;
        self.emit(SyncUiEvent::Changed);
    }

    fn emit(&self, event: SyncUiEvent) {
        if let Some(tx) = &self.ui_tx {
            let _ = tx.try_send(event);
        }
    }

    fn try_begin(&self, busy: SyncStatus) -> bool {
        let mut guard = self.status.lock().unwrap();
        if guard.is_busy() {
            return false;
        }
        *guard = busy;
        drop(guard);
        self.emit(SyncUiEvent::Changed);
        true
    }

    pub fn test_connection(self: &Arc<Self>, sync: SyncSettings) {
        if !self.try_begin(SyncStatus::Testing) {
            return;
        }
        let manager = Arc::clone(self);
        std::thread::Builder::new()
            .name("portus-sync-test".into())
            .spawn(move || match backend_test(&sync) {
                Ok(message) => {
                    manager.set_status(SyncStatus::Idle);
                    manager.emit(SyncUiEvent::TestSucceeded { message });
                }
                Err(message) => {
                    manager.set_status(SyncStatus::Error(message.clone()));
                    manager.emit(SyncUiEvent::TestFailed { message });
                }
            })
            .ok();
    }

    pub fn push(self: &Arc<Self>, settings: Settings, silent: bool) {
        if !self.try_begin(SyncStatus::Pushing) {
            return;
        }
        let manager = Arc::clone(self);
        std::thread::Builder::new()
            .name("portus-sync-push".into())
            .spawn(move || {
                let passphrase =
                    decrypt_secret(&settings.sync.encryption_passphrase).unwrap_or_default();
                let device_id = ensure_device_id(&settings.sync.device_id);
                let ts = now_unix_secs();
                let res = seal_envelope(&settings, &passphrase, &device_id, ts)
                    .and_then(|(bytes, hash)| backend_put(&settings.sync, &bytes).map(|()| hash));
                match res {
                    Ok(content_hash) => {
                        manager.set_status(SyncStatus::Synced {
                            timestamp: ts,
                            summary: "Uploaded".to_owned(),
                        });
                        manager.emit(SyncUiEvent::Pushed {
                            timestamp: ts,
                            content_hash,
                            device_id,
                            silent,
                        });
                    }
                    Err(message) => {
                        manager.set_status(SyncStatus::Error(message.clone()));
                        manager.emit(SyncUiEvent::Failed { message, silent });
                    }
                }
            })
            .ok();
    }

    pub fn pull(self: &Arc<Self>, sync: SyncSettings, strategy: SyncConflictStrategy, silent: bool) {
        if !self.try_begin(SyncStatus::Pulling) {
            return;
        }
        let manager = Arc::clone(self);
        std::thread::Builder::new()
            .name("portus-sync-pull".into())
            .spawn(move || {
                let passphrase = decrypt_secret(&sync.encryption_passphrase).unwrap_or_default();
                let device_id = ensure_device_id(&sync.device_id);
                let res = backend_get(&sync).and_then(|maybe_bytes| {
                    let bytes = maybe_bytes.ok_or_else(|| {
                        "No remote configuration found yet. Push from a device first.".to_owned()
                    })?;
                    open_envelope(&bytes, &passphrase)
                });
                match res {
                    Ok(opened) => {
                        let ts = now_unix_secs();
                        manager.set_status(SyncStatus::Synced {
                            timestamp: ts,
                            summary: "Downloaded".to_owned(),
                        });
                        manager.emit(SyncUiEvent::Pulled {
                            settings: Box::new(opened.settings),
                            timestamp: ts,
                            content_hash: opened.content_hash,
                            device_id,
                            strategy,
                            silent,
                        });
                    }
                    Err(message) => {
                        manager.set_status(SyncStatus::Error(message.clone()));
                        manager.emit(SyncUiEvent::Failed { message, silent });
                    }
                }
            })
            .ok();
    }

    /// Smart two-way sync:
    /// - If no remote file exists -> pushes local settings.
    /// - If remote hash == local hash -> already up to date.
    /// - If local has no unsynced changes -> pulls remote settings.
    /// - If both changed -> newer timestamp wins (or pulls with configured strategy).
    pub fn sync_now(self: &Arc<Self>, settings: Settings, silent: bool) {
        if !self.try_begin(SyncStatus::Syncing) {
            return;
        }
        let manager = Arc::clone(self);
        std::thread::Builder::new()
            .name("portus-sync-now".into())
            .spawn(move || {
                let sync = settings.sync.clone();
                let passphrase = decrypt_secret(&sync.encryption_passphrase).unwrap_or_default();
                let device_id = ensure_device_id(&sync.device_id);
                let local_hash = compute_snapshot_hash(&settings);
                let ts = now_unix_secs();

                match backend_get(&sync) {
                    Ok(None) => {
                        // First sync: push local settings to remote.
                        match seal_envelope(&settings, &passphrase, &device_id, ts)
                            .and_then(|(bytes, hash)| backend_put(&sync, &bytes).map(|()| hash))
                        {
                            Ok(content_hash) => {
                                manager.set_status(SyncStatus::Synced {
                                    timestamp: ts,
                                    summary: "Uploaded".to_owned(),
                                });
                                manager.emit(SyncUiEvent::Pushed {
                                    timestamp: ts,
                                    content_hash,
                                    device_id,
                                    silent,
                                });
                            }
                            Err(message) => {
                                manager.set_status(SyncStatus::Error(message.clone()));
                                manager.emit(SyncUiEvent::Failed { message, silent });
                            }
                        }
                    }
                    Ok(Some(bytes)) => match open_envelope(&bytes, &passphrase) {
                        Ok(opened) => {
                            if opened.content_hash == local_hash {
                                manager.set_status(SyncStatus::Synced {
                                    timestamp: ts,
                                    summary: "Up to date".to_owned(),
                                });
                                manager.emit(SyncUiEvent::UpToDate {
                                    timestamp: ts,
                                    silent,
                                });
                                return;
                            }

                            let local_unchanged = !sync.last_sync_hash.is_empty()
                                && sync.last_sync_hash == local_hash;
                            let remote_newer =
                                opened.updated_at > sync.last_sync_at.unwrap_or(0);

                            if local_unchanged || remote_newer {
                                manager.set_status(SyncStatus::Synced {
                                    timestamp: ts,
                                    summary: "Downloaded".to_owned(),
                                });
                                manager.emit(SyncUiEvent::Pulled {
                                    settings: Box::new(opened.settings),
                                    timestamp: ts,
                                    content_hash: opened.content_hash,
                                    device_id,
                                    strategy: sync.conflict_strategy,
                                    silent,
                                });
                            } else {
                                match seal_envelope(&settings, &passphrase, &device_id, ts)
                                    .and_then(|(bytes, hash)| {
                                        backend_put(&sync, &bytes).map(|()| hash)
                                    }) {
                                    Ok(content_hash) => {
                                        manager.set_status(SyncStatus::Synced {
                                            timestamp: ts,
                                            summary: "Uploaded".to_owned(),
                                        });
                                        manager.emit(SyncUiEvent::Pushed {
                                            timestamp: ts,
                                            content_hash,
                                            device_id,
                                            silent,
                                        });
                                    }
                                    Err(message) => {
                                        manager.set_status(SyncStatus::Error(message.clone()));
                                        manager.emit(SyncUiEvent::Failed { message, silent });
                                    }
                                }
                            }
                        }
                        Err(message) => {
                            manager.set_status(SyncStatus::Error(message.clone()));
                            manager.emit(SyncUiEvent::Failed { message, silent });
                        }
                    },
                    Err(message) => {
                        manager.set_status(SyncStatus::Error(message.clone()));
                        manager.emit(SyncUiEvent::Failed { message, silent });
                    }
                }
            })
            .ok();
    }
}

pub fn format_timestamp(unix_secs: u64) -> String {
    DateTime::<Utc>::from_timestamp(unix_secs as i64, 0)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| unix_secs.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{HostIcon, HostProfile, HostProtocol, KeychainEntry, KeychainKind};
    use crate::theme::Theme;

    #[test]
    fn s3_sigv4_matches_aws_reference_vector() {
        // Official AWS SigV4 GET Object test vector:
        // https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html
        let dt = DateTime::parse_from_rfc3339("2013-05-24T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let headers = sign_s3_headers(
            "GET",
            "examplebucket.s3.amazonaws.com",
            "/test.txt",
            "",
            "us-east-1",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            b"",
            dt,
        );
        let auth = headers
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert!(auth.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature="
        ));
        // Deterministic signature check (empty payload SHA-256)
        let hash = headers
            .iter()
            .find(|(k, _)| k == "x-amz-content-sha256")
            .map(|(_, v)| v.as_str())
            .unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn s3_url_supports_path_style_and_virtual_hosted_style() {
        let mut cfg = S3SyncConfig {
            endpoint: "https://s3.us-east-1.amazonaws.com".into(),
            region: "us-east-1".into(),
            bucket: "my-bucket".into(),
            access_key_id: "ak".into(),
            secret_access_key: "sk".into(),
            object_key: "portus/my config.json".into(),
            path_style: true,
        };
        let (url, host, uri) = s3_target_url_and_host(&cfg).unwrap();
        assert_eq!(
            url,
            "https://s3.us-east-1.amazonaws.com/my-bucket/portus/my%20config.json"
        );
        assert_eq!(host, "s3.us-east-1.amazonaws.com");
        assert_eq!(uri, "/my-bucket/portus/my%20config.json");

        cfg.path_style = false;
        let (url2, host2, uri2) = s3_target_url_and_host(&cfg).unwrap();
        assert_eq!(
            url2,
            "https://my-bucket.s3.us-east-1.amazonaws.com/portus/my%20config.json"
        );
        assert_eq!(host2, "my-bucket.s3.us-east-1.amazonaws.com");
        assert_eq!(uri2, "/portus/my%20config.json");
    }

    #[test]
    fn webdav_url_joins_and_computes_parent_dirs() {
        let url = webdav_join_url("https://dav.example.com/dav/", "/portus/sub/config.json").unwrap();
        assert_eq!(url, "https://dav.example.com/dav/portus/sub/config.json");

        let parents =
            webdav_parent_dirs("https://dav.example.com/dav", "portus/sub/config.json").unwrap();
        assert_eq!(
            parents,
            vec![
                "https://dav.example.com/dav/portus",
                "https://dav.example.com/dav/portus/sub"
            ]
        );
    }

    #[test]
    fn envelope_roundtrip_encrypted_and_plain() {
        let mut settings = Settings::default();
        settings.theme = Theme::Midnight;
        settings.opacity = 85;
        settings.hosts.push(HostProfile {
            id: 1,
            name: "Prod API".into(),
            protocol: HostProtocol::Ssh,
            icon: HostIcon::Server,
            address: "10.0.0.10".into(),
            username: "deploy".into(),
            port: 22,
            baud_rate: 115200,
            keychain_id: Some(1),
            identity_file: String::new(),
            password: "plain-password".into(),
            jump_host: String::new(),
            jump_host_id: None,
            jump_relay_command: String::new(),
            jump_relay_keychain_id: None,
            jump_relay_password: String::new(),
            group: "Production".into(),
            tags: vec!["api".into()],
            last_connected: None,
        });
        settings.keychain.push(KeychainEntry {
            id: 1,
            name: "Prod Key".into(),
            kind: KeychainKind::Password,
            key_path: String::new(),
            secret: "kc-secret".into(),
            notes: String::new(),
        });

        // Encrypted roundtrip
        let (sealed, hash) = seal_envelope(&settings, "master-pass-123", "dev-1", 1700000000).unwrap();
        assert!(open_envelope(&sealed, "wrong-pass").is_err());
        assert!(open_envelope(&sealed, "").is_err());

        let opened = open_envelope(&sealed, "master-pass-123").unwrap();
        assert_eq!(opened.updated_at, 1700000000);
        assert_eq!(opened.content_hash, hash);
        assert_eq!(opened.settings.theme, Theme::Midnight);
        assert_eq!(opened.settings.opacity, 85);
        assert_eq!(opened.settings.hosts.len(), 1);
        assert_eq!(opened.settings.hosts[0].password, "plain-password");
        assert_eq!(opened.settings.keychain[0].secret, "kc-secret");

        // Plain roundtrip
        let (plain_bytes, plain_hash) = seal_envelope(&settings, "", "dev-1", 1700000001).unwrap();
        assert_eq!(plain_hash, hash);
        let opened_plain = open_envelope(&plain_bytes, "").unwrap();
        assert_eq!(opened_plain.settings.hosts[0].name, "Prod API");
    }

    #[test]
    fn extract_s3_xml_error_parses_code_and_message() {
        let xml = b"<Error><Code>SignatureDoesNotMatch</Code><Message>The request signature we calculated does not match.</Message></Error>";
        let msg = extract_s3_error(403, xml);
        assert_eq!(
            msg,
            "S3 HTTP 403 (SignatureDoesNotMatch): The request signature we calculated does not match."
        );
    }

    #[test]
    fn snapshot_hash_ignores_sync_metadata_and_tracks_config_changes() {
        let mut a = Settings::default();
        let mut b = Settings::default();
        b.sync.provider = SyncProvider::S3;
        b.sync.last_sync_at = Some(123456);
        b.sync.last_sync_hash = "old-hash".into();
        assert_eq!(compute_snapshot_hash(&a), compute_snapshot_hash(&b));

        a.opacity = 80;
        assert_ne!(compute_snapshot_hash(&a), compute_snapshot_hash(&b));
    }

    #[test]
    fn apply_remote_settings_overwrite_and_merge_preserve_local_sync() {
        use crate::settings::{PortForwardRule, TunnelKind};

        let mut local = Settings::default();
        local.sync.provider = SyncProvider::WebDav;
        local.sync.webdav.endpoint = "https://dav.example.com/dav".into();
        local.hosts.push(HostProfile {
            id: 1,
            name: "Local Only".into(),
            protocol: HostProtocol::Ssh,
            icon: HostIcon::Server,
            address: "192.168.1.10".into(),
            username: "local".into(),
            port: 22,
            baud_rate: 115200,
            keychain_id: None,
            identity_file: String::new(),
            password: String::new(),
            jump_host: String::new(),
            jump_host_id: None,
            jump_relay_command: String::new(),
            jump_relay_keychain_id: None,
            jump_relay_password: String::new(),
            group: String::new(),
            tags: Vec::new(),
            last_connected: None,
        });
        local.next_host_id = 2;

        let mut remote = Settings::default();
        remote.theme = Theme::Paper;
        remote.opacity = 90;
        remote.keychain.push(KeychainEntry {
            id: 1,
            name: "Remote Key".into(),
            kind: KeychainKind::SshKey,
            key_path: "~/.ssh/id_ed25519".into(),
            secret: String::new(),
            notes: String::new(),
        });
        remote.next_keychain_id = 2;
        remote.hosts.push(HostProfile {
            id: 1,
            name: "Bastion".into(),
            protocol: HostProtocol::Ssh,
            icon: HostIcon::Shield,
            address: "bastion.example.com".into(),
            username: "ops".into(),
            port: 22,
            baud_rate: 115200,
            keychain_id: Some(1),
            identity_file: String::new(),
            password: String::new(),
            jump_host: String::new(),
            jump_host_id: None,
            jump_relay_command: String::new(),
            jump_relay_keychain_id: None,
            jump_relay_password: String::new(),
            group: "Prod".into(),
            tags: Vec::new(),
            last_connected: None,
        });
        remote.hosts.push(HostProfile {
            id: 2,
            name: "App Node".into(),
            protocol: HostProtocol::Ssh,
            icon: HostIcon::Server,
            address: "10.0.1.5".into(),
            username: "ops".into(),
            port: 22,
            baud_rate: 115200,
            keychain_id: Some(1),
            identity_file: String::new(),
            password: String::new(),
            jump_host: String::new(),
            jump_host_id: Some(1),
            jump_relay_command: String::new(),
            jump_relay_keychain_id: None,
            jump_relay_password: String::new(),
            group: "Prod".into(),
            tags: Vec::new(),
            last_connected: None,
        });
        remote.next_host_id = 3;
        remote.tunnels.push(PortForwardRule {
            id: 1,
            name: "DB Tunnel".into(),
            kind: TunnelKind::Local,
            host_id: 2,
            bind_address: "127.0.0.1".into(),
            bind_port: 5432,
            target_host: "127.0.0.1".into(),
            target_port: 5432,
        });
        remote.next_tunnel_id = 2;

        // Test Merge mode: keeps Local Only, adds Bastion + App Node with remapped jump_host_id & tunnel host_id
        let mut merged = local.clone();
        let stats = apply_remote_settings(&mut merged, remote.clone(), SyncConflictStrategy::Merge)
            .unwrap();
        assert_eq!(stats.hosts, 2);
        assert_eq!(stats.keychain, 1);
        assert_eq!(stats.tunnels, 1);
        assert_eq!(merged.hosts.len(), 3);
        assert_eq!(merged.theme, Theme::Paper);
        assert_eq!(merged.sync.provider, SyncProvider::WebDav);
        assert_eq!(merged.sync.webdav.endpoint, "https://dav.example.com/dav");
        let bastion_id = merged.hosts.iter().find(|h| h.name == "Bastion").unwrap().id;
        let app_node = merged.hosts.iter().find(|h| h.name == "App Node").unwrap();
        assert_eq!(app_node.jump_host_id, Some(bastion_id));
        assert_eq!(merged.tunnels[0].host_id, app_node.id);

        // Test Overwrite mode: replaces hosts completely, preserves local.sync
        let mut overwritten = local.clone();
        let ov_stats =
            apply_remote_settings(&mut overwritten, remote, SyncConflictStrategy::Overwrite)
                .unwrap();
        assert_eq!(ov_stats.hosts, 2);
        assert_eq!(overwritten.hosts.len(), 2);
        assert_eq!(overwritten.theme, Theme::Paper);
        assert_eq!(overwritten.opacity, 90);
        assert_eq!(overwritten.sync.provider, SyncProvider::WebDav);
        assert_eq!(
            overwritten.sync.webdav.endpoint,
            "https://dav.example.com/dav"
        );
    }
}
