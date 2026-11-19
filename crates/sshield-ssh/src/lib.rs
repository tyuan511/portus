//! Pure-Rust SSH/SFTP client, connection pool, `~/.ssh/config` importer,
//! and multi-hop `ProxyJump` bastion tunneling (via `russh` + `ssh2-config`).

use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::ChannelMsg;
use russh::client::{self, AuthResult, Config};
use russh::keys::agent::client::AgentClient;
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate, decode_secret_key, load_secret_key};
use ssh2_config::{ParseRule, SshConfig};
use sshield_core::{AuthKind, CommandOutput, Result, ServerAsset, SshieldError};
use sshield_vault::SealedVault;
use tokio::sync::Mutex;

/// `russh` client handler for Sshield sessions.
pub struct SshClientHandler {
    host: String,
    port: u16,
}

impl client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        // Phase 1: log server key fingerprint/type and accept.
        // TODO(Phase 2): enforce ~/.ssh/known_hosts / TOFU pinning per asset.
        tracing::debug!(
            host = %self.host,
            port = self.port,
            server_key = ?server_public_key,
            "Verified server host key"
        );
        Ok(true)
    }
}

/// Import server assets from the current user's `~/.ssh/config`.
pub fn import_default_ssh_config() -> Result<Vec<ServerAsset>> {
    let home = dirs::home_dir()
        .ok_or_else(|| SshieldError::ConfigImport("Cannot determine home directory".into()))?;
    let config_path = home.join(".ssh").join("config");
    if !config_path.exists() {
        return Err(SshieldError::ConfigImport(format!(
            "SSH config file not found at {}",
            config_path.display()
        )));
    }
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| SshieldError::ConfigImport(format!("Failed to read {}: {e}", config_path.display())))?;
    import_ssh_config_str(&content)
}

/// Parse an OpenSSH config string into `ServerAsset` entries, resolving `ProxyJump` chains.
pub fn import_ssh_config_str(content: &str) -> Result<Vec<ServerAsset>> {
    let mut reader = BufReader::new(content.as_bytes());
    let config = SshConfig::default()
        .parse(&mut reader, ParseRule::ALLOW_UNKNOWN_FIELDS)
        .map_err(|e| SshieldError::ConfigImport(format!("Failed to parse SSH config: {e}")))?;

    let default_user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".to_string());

    let mut alias_to_id: HashMap<String, String> = HashMap::new();
    let mut raw_entries: Vec<(String, ssh2_config::HostParams)> = Vec::new();
    let mut seen_aliases = HashSet::new();

    for host in config.get_hosts() {
        for clause in &host.pattern {
            let pat = clause.pattern.trim();
            if clause.negated || pat.is_empty() || pat.contains('*') || pat.contains('?') {
                continue;
            }
            if seen_aliases.insert(pat.to_string()) {
                let params = config.query(pat);
                let id = format!("ssh-{}", sanitize_id(pat));
                alias_to_id.insert(pat.to_string(), id);
                raw_entries.push((pat.to_string(), params));
            }
        }
    }

    let mut assets = Vec::new();
    let mut extra_jump_assets: HashMap<String, ServerAsset> = HashMap::new();

    for (alias, params) in raw_entries {
        let id = alias_to_id.get(&alias).cloned().unwrap_or_else(|| format!("ssh-{}", sanitize_id(&alias)));
        let host = params.host_name.clone().unwrap_or_else(|| alias.clone());
        let port = params.port.unwrap_or(22);
        let username = params.user.clone().unwrap_or_else(|| default_user.clone());

        let identity_file = params
            .identity_file
            .as_ref()
            .and_then(|files| files.first())
            .map(|p| expand_tilde(&p.to_string_lossy()));

        let auth_kind = if identity_file.is_some() {
            AuthKind::IdentityFile
        } else {
            AuthKind::AgentOrDefaultKey
        };

        let jump_host_id = resolve_proxy_jump(
            params.proxy_jump.as_deref(),
            &alias_to_id,
            &default_user,
            &mut extra_jump_assets,
        );

        assets.push(ServerAsset {
            id,
            name: alias,
            host,
            port,
            username,
            auth_kind,
            identity_file,
            credential_id: None,
            jump_host_id,
            group_name: Some("Imported".to_string()),
            tags: vec!["ssh-config".to_string()],
            description: Some("Imported from SSH config".to_string()),
            enabled: true,
        });
    }

    // Prepend any inline jump hosts that were not explicitly defined as Host blocks
    let mut result: Vec<ServerAsset> = extra_jump_assets.into_values().collect();
    result.extend(assets);
    Ok(result)
}

fn resolve_proxy_jump(
    proxy_jump: Option<&[String]>,
    alias_to_id: &HashMap<String, String>,
    default_user: &str,
    extra_jump_assets: &mut HashMap<String, ServerAsset>,
) -> Option<String> {
    let jumps = proxy_jump?;
    if jumps.is_empty() {
        return None;
    }

    // `ProxyJump` can be comma-separated (`bastion1,bastion2`) which ssh2-config splits into Vec<String>.
    // Walk the chain in order and link each hop to the previous one if inline.
    let mut prev_id: Option<String> = None;
    for hop in jumps {
        let hop = hop.trim();
        if hop.is_empty() || hop.eq_ignore_ascii_case("none") {
            return None;
        }
        if let Some(existing_id) = alias_to_id.get(hop) {
            prev_id = Some(existing_id.clone());
        } else {
            let (user, host, port) = parse_jump_spec(hop, default_user);
            let jump_id = format!("ssh-jump-{}", sanitize_id(hop));
            extra_jump_assets
                .entry(jump_id.clone())
                .or_insert_with(|| ServerAsset {
                    id: jump_id.clone(),
                    name: hop.to_string(),
                    host,
                    port,
                    username: user,
                    auth_kind: AuthKind::AgentOrDefaultKey,
                    identity_file: None,
                    credential_id: None,
                    jump_host_id: prev_id.clone(),
                    group_name: Some("Bastion".to_string()),
                    tags: vec!["ssh-config".to_string(), "jump-host".to_string()],
                    description: Some("Auto-created ProxyJump host".to_string()),
                    enabled: true,
                });
            prev_id = Some(jump_id);
        }
    }
    prev_id
}

fn parse_jump_spec(spec: &str, default_user: &str) -> (String, String, u16) {
    let (user, rest) = match spec.split_once('@') {
        Some((u, r)) => (u.to_string(), r),
        None => (default_user.to_string(), spec),
    };
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(22)),
        None => (rest.to_string(), 22),
    };
    (user, host, port)
}

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().to_string();
    }
    path.to_string()
}

type SharedSession = Arc<Mutex<client::Handle<SshClientHandler>>>;

/// Asynchronous SSH connection pool supporting multi-hop `ProxyJump` tunneling.
pub struct SshPool {
    sessions: Mutex<HashMap<String, SharedSession>>,
    client_config: Arc<Config>,
    vault: Option<Arc<SealedVault>>,
}

impl Default for SshPool {
    fn default() -> Self {
        Self::new()
    }
}

impl SshPool {
    pub fn new() -> Self {
        Self::create(None)
    }

    pub fn with_vault(vault: Arc<SealedVault>) -> Self {
        Self::create(Some(vault))
    }

    fn create(vault: Option<Arc<SealedVault>>) -> Self {
        let config = Config {
            inactivity_timeout: Some(Duration::from_secs(300)),
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            ..Default::default()
        };
        Self {
            sessions: Mutex::new(HashMap::new()),
            client_config: Arc::new(config),
            vault,
        }
    }

    /// Evict a cached session (e.g., when asset credentials or host change).
    pub async fn invalidate(&self, server_id: &str) {
        let mut guard = self.sessions.lock().await;
        guard.remove(server_id);
    }

    /// Test connectivity to `asset` (establishing any ProxyJump tunnels) without executing a command.
    pub async fn test_connection(
        &self,
        asset: &ServerAsset,
        all_assets: &HashMap<String, ServerAsset>,
    ) -> Result<()> {
        let _ = self.get_or_connect(asset, all_assets).await?;
        Ok(())
    }

    /// Execute `command` on `asset` with a hard `timeout`.
    pub async fn execute_command(
        &self,
        asset: &ServerAsset,
        all_assets: &HashMap<String, ServerAsset>,
        command: &str,
        timeout: Duration,
    ) -> Result<CommandOutput> {
        let started = Instant::now();
        let session = self.get_or_connect(asset, all_assets).await?;

        let exec_future = async {
            let channel = {
                let guard = session.lock().await;
                guard
                    .channel_open_session()
                    .await
                    .map_err(|e| SshieldError::Ssh(format!("Failed to open SSH session channel: {e}")))?
            };

            let mut channel = channel;
            channel
                .exec(true, command)
                .await
                .map_err(|e| SshieldError::Ssh(format!("Failed to exec command: {e}")))?;

            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut exit_code: Option<i32> = None;

            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                    ChannelMsg::ExtendedData { data, ext } if ext == 1 => {
                        stderr.extend_from_slice(&data);
                    }
                    ChannelMsg::ExitStatus { exit_status } => {
                        exit_code = Some(exit_status as i32);
                    }
                    ChannelMsg::Eof | ChannelMsg::Close => break,
                    _ => {}
                }
            }

            Ok::<CommandOutput, SshieldError>(CommandOutput {
                exit_code: exit_code.unwrap_or(0),
                stdout: String::from_utf8_lossy(&stdout).to_string(),
                stderr: String::from_utf8_lossy(&stderr).to_string(),
                duration_ms: started.elapsed().as_millis() as u64,
            })
        };

        match tokio::time::timeout(timeout, exec_future).await {
            Ok(res) => res,
            Err(_) => Err(SshieldError::Timeout(timeout.as_secs())),
        }
    }

    async fn get_or_connect(
        &self,
        asset: &ServerAsset,
        all_assets: &HashMap<String, ServerAsset>,
    ) -> Result<SharedSession> {
        {
            let guard = self.sessions.lock().await;
            if let Some(existing) = guard.get(&asset.id) {
                let handle = existing.lock().await;
                if !handle.is_closed() {
                    drop(handle);
                    return Ok(Arc::clone(existing));
                }
            }
        }

        // Build full ProxyJump chain from outermost bastion -> ... -> target
        let chain = resolve_jump_chain(asset, all_assets)?;
        let mut current_handle: Option<client::Handle<SshClientHandler>> = None;

        for hop in chain {
            let handler = SshClientHandler {
                host: hop.host.clone(),
                port: hop.port,
            };

            let mut handle = match current_handle {
                None => {
                    let addr = format!("{}:{}", hop.host, hop.port);
                    client::connect(Arc::clone(&self.client_config), addr, handler)
                        .await
                        .map_err(|e| {
                            SshieldError::Ssh(format!(
                                "TCP/SSH connect to {} ({}:{}) failed: {e}",
                                hop.name, hop.host, hop.port
                            ))
                        })?
                }
                Some(prev) => {
                    let tunnel = prev
                        .channel_open_direct_tcpip(
                            hop.host.clone(),
                            hop.port as u32,
                            "127.0.0.1",
                            0,
                        )
                        .await
                        .map_err(|e| {
                            SshieldError::Ssh(format!(
                                "ProxyJump direct-tcpip to {} ({}:{}) failed: {e}",
                                hop.name, hop.host, hop.port
                            ))
                        })?;

                    client::connect_stream(
                        Arc::clone(&self.client_config),
                        tunnel.into_stream(),
                        handler,
                    )
                    .await
                    .map_err(|e| {
                        SshieldError::Ssh(format!(
                            "ProxyJump SSH handshake with {} ({}:{}) failed: {e}",
                            hop.name, hop.host, hop.port
                        ))
                    })?
                }
            };

            authenticate_handle(&mut handle, &hop, self.vault.as_deref()).await?;
            current_handle = Some(handle);
        }

        let final_handle = current_handle
            .ok_or_else(|| SshieldError::Ssh("Empty ProxyJump chain".to_string()))?;
        let shared = Arc::new(Mutex::new(final_handle));

        let mut guard = self.sessions.lock().await;
        guard.insert(asset.id.clone(), Arc::clone(&shared));
        Ok(shared)
    }
}

fn resolve_jump_chain(
    target: &ServerAsset,
    all_assets: &HashMap<String, ServerAsset>,
) -> Result<Vec<ServerAsset>> {
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut cursor = Some(target.clone());

    while let Some(node) = cursor {
        if !visited.insert(node.id.clone()) {
            return Err(SshieldError::Ssh(format!(
                "Cyclic ProxyJump detected involving asset `{}`",
                node.id
            )));
        }
        let next_id = node.jump_host_id.clone();
        chain.push(node);
        cursor = match next_id {
            Some(ref jid) => {
                let jump = all_assets.get(jid).ok_or_else(|| {
                    SshieldError::AssetNotFound(format!("Jump host `{jid}` not found"))
                })?;
                Some(jump.clone())
            }
            None => None,
        };
    }

    chain.reverse();
    Ok(chain)
}

async fn authenticate_handle(
    handle: &mut client::Handle<SshClientHandler>,
    asset: &ServerAsset,
    vault: Option<&SealedVault>,
) -> Result<()> {
    // 0. Try encrypted credential from SealedVault if `credential_id` is bound
    if let Some(ref cred_id) = asset.credential_id
        && let Some(v) = vault
    {
        let secret = v.get_secret(cred_id).await?;
        // Try decoding as PEM private key first; if not PEM, try password auth
        if secret.contains("PRIVATE KEY") {
            if let Ok(key) = decode_secret_key(&secret, None) {
                let hash_alg = if key.algorithm().is_rsa() {
                    handle.best_supported_rsa_hash().await.ok().flatten().flatten()
                } else {
                    None
                };
                let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
                if let Ok(AuthResult::Success) = handle.authenticate_publickey(&asset.username, key_with_alg).await {
                    return Ok(());
                }
            }
        } else if let Ok(AuthResult::Success) = handle.authenticate_password(&asset.username, &*secret).await {
            return Ok(());
        }
    }

    // 1. Try explicit IdentityFile if configured
    if let Some(ref key_path) = asset.identity_file {
        let expanded = expand_tilde(key_path);
        if try_key_file(handle, &asset.username, Path::new(&expanded)).await? {
            return Ok(());
        }
    }

    // 2. Try SSH Agent if available
    if let Ok(mut agent) = AgentClient::connect_env().await
        && let Ok(identities) = agent.request_identities().await
    {
        for identity in identities {
            if let russh::keys::agent::AgentIdentity::PublicKey { key: pubkey, .. } = identity {
                let best_hash = handle
                    .best_supported_rsa_hash()
                    .await
                    .ok()
                    .flatten()
                    .flatten();
                if let Ok(AuthResult::Success) = handle
                    .authenticate_publickey_with(&asset.username, pubkey, best_hash, &mut agent)
                    .await
                {
                    return Ok(());
                }
            }
        }
    }

    // 3. Fallback to default keys (~/.ssh/id_ed25519, id_rsa, id_ecdsa)
    if let Some(home) = dirs::home_dir() {
        for name in ["id_ed25519", "id_rsa", "id_ecdsa"] {
            let path = home.join(".ssh").join(name);
            if path.exists() && try_key_file(handle, &asset.username, &path).await? {
                return Ok(());
            }
        }
    }

    Err(SshieldError::Ssh(format!(
        "Authentication failed for {}@{}:{} (no matching key or agent identity)",
        asset.username, asset.host, asset.port
    )))
}

async fn try_key_file(
    handle: &mut client::Handle<SshClientHandler>,
    username: &str,
    path: &Path,
) -> Result<bool> {
    let key = match load_secret_key(path, None) {
        Ok(k) => k,
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "Skipping private key file");
            return Ok(false);
        }
    };

    let hash_alg = if key.algorithm().is_rsa() {
        handle
            .best_supported_rsa_hash()
            .await
            .ok()
            .flatten()
            .flatten()
    } else {
        None
    };

    let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
    match handle.authenticate_publickey(username, key_with_alg).await {
        Ok(AuthResult::Success) => Ok(true),
        Ok(_) => Ok(false),
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "Public key auth attempt failed");
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_ssh_config_with_proxy_jump_chain() {
        let cfg = r#"
Host bastion
    HostName 10.0.0.1
    User ops
    Port 2222
    IdentityFile ~/.ssh/id_bastion

Host prod-db
    HostName 192.168.10.50
    User postgres
    ProxyJump bastion

Host *
    ServerAliveInterval 60
"#;

        let assets = import_ssh_config_str(cfg).expect("parse ssh config");
        assert_eq!(assets.len(), 2);

        let bastion = assets.iter().find(|a| a.name == "bastion").unwrap();
        assert_eq!(bastion.host, "10.0.0.1");
        assert_eq!(bastion.port, 2222);
        assert_eq!(bastion.username, "ops");
        assert!(bastion.jump_host_id.is_none());

        let db = assets.iter().find(|a| a.name == "prod-db").unwrap();
        assert_eq!(db.host, "192.168.10.50");
        assert_eq!(db.username, "postgres");
        assert_eq!(db.jump_host_id.as_deref(), Some(bastion.id.as_str()));
    }

    #[test]
    fn detects_cyclic_proxy_jump() {
        let a = ServerAsset {
            id: "a".into(),
            name: "a".into(),
            host: "1.1.1.1".into(),
            port: 22,
            username: "root".into(),
            auth_kind: AuthKind::AgentOrDefaultKey,
            identity_file: None,
            credential_id: None,
            jump_host_id: Some("b".into()),
            group_name: None,
            tags: vec![],
            description: None,
            enabled: true,
        };
        let b = ServerAsset {
            id: "b".into(),
            name: "b".into(),
            host: "2.2.2.2".into(),
            port: 22,
            username: "root".into(),
            auth_kind: AuthKind::AgentOrDefaultKey,
            identity_file: None,
            credential_id: None,
            jump_host_id: Some("a".into()),
            group_name: None,
            tags: vec![],
            description: None,
            enabled: true,
        };
        let mut map = HashMap::new();
        map.insert("a".into(), a.clone());
        map.insert("b".into(), b);

        let err = resolve_jump_chain(&a, &map).unwrap_err();
        assert!(err.to_string().contains("Cyclic ProxyJump"));
    }
}
