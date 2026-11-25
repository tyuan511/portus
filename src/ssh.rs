use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_channel::Sender;
use gpui_terminal::{ExitReport, GridSize, TerminalSink, Transport};
use russh::keys::known_hosts::{check_known_hosts, learn_known_hosts};
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate, load_secret_key};
use russh::{ChannelMsg, Disconnect, client};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};

use crate::settings::{HostProfile, HostProtocol};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub(crate) fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("bulwark-ssh")
            .build()
            .expect("failed to start tokio runtime for SSH")
    })
}

#[derive(Clone, Debug)]
pub enum SshPromptKind {
    Password {
        endpoint: String,
    },
    Passphrase {
        key_path: String,
    },
    RelayPassword {
        command: String,
        endpoint: String,
    },
    HostKey {
        host: String,
        port: u16,
        fingerprint: String,
    },
}

#[derive(Debug)]
pub struct SshPrompt {
    pub kind: SshPromptKind,
    pub reply: oneshot::Sender<Option<String>>,
}

#[derive(Debug)]
pub enum SshUiEvent {
    Connecting(String),
    Connected,
    Prompt(SshPrompt),
    Error(String),
}

enum SshCommand {
    Data(Vec<u8>),
    Resize(GridSize),
    Terminate,
}

pub struct SshTransport {
    tx: mpsc::UnboundedSender<SshCommand>,
}

impl SshTransport {
    pub fn spawn(
        host: HostProfile,
        all_hosts: Vec<HostProfile>,
        sink: TerminalSink,
        events: Sender<SshUiEvent>,
    ) -> gpui_terminal::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        runtime().spawn(async move {
            run_ssh_session(host, all_hosts, sink, events, rx).await;
        });
        Ok(Self { tx })
    }
}

impl Transport for SshTransport {
    fn write(&mut self, bytes: &[u8]) -> gpui_terminal::Result<()> {
        self.tx
            .send(SshCommand::Data(bytes.to_vec()))
            .map_err(|_| gpui_terminal::Error::Closed)
    }

    fn resize(&mut self, size: GridSize) -> gpui_terminal::Result<()> {
        let _ = self.tx.send(SshCommand::Resize(size));
        Ok(())
    }

    fn terminate(&mut self) -> gpui_terminal::Result<()> {
        let _ = self.tx.send(SshCommand::Terminate);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JumpTarget {
    username: String,
    host: String,
    port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JumpHop {
    username: String,
    host: String,
    port: u16,
    identity_file: String,
    encrypted_password: String,
}

fn resolve_jump_chain(host: &HostProfile, all_hosts: &[HostProfile]) -> Vec<JumpHop> {
    let mut visited = HashSet::new();
    if host.id != 0 {
        visited.insert(host.id);
    }
    let mut hops = Vec::new();
    collect_jump_hops(host, all_hosts, &mut visited, &mut hops);
    hops
}

fn collect_jump_hops(
    host: &HostProfile,
    all_hosts: &[HostProfile],
    visited: &mut HashSet<u64>,
    hops: &mut Vec<JumpHop>,
) {
    if let Some(jid) = host.jump_host_id
        && visited.insert(jid)
        && let Some(jump_profile) = all_hosts
            .iter()
            .find(|h| h.id == jid && h.protocol == HostProtocol::Ssh)
    {
        collect_jump_hops(jump_profile, all_hosts, visited, hops);
        hops.push(JumpHop {
            username: if jump_profile.username.is_empty() {
                host.username.clone()
            } else {
                jump_profile.username.clone()
            },
            host: jump_profile.address.clone(),
            port: jump_profile.port,
            identity_file: jump_profile.identity_file.clone(),
            encrypted_password: jump_profile.password.clone(),
        });
        return;
    }

    for target in parse_jump_targets(&host.jump_host, &host.username) {
        let matched = all_hosts.iter().find(|h| {
            h.protocol == HostProtocol::Ssh
                && !visited.contains(&h.id)
                && h.address == target.host
                && h.port == target.port
                && (h.username.is_empty() || h.username == target.username)
        });
        if let Some(jump_profile) = matched {
            if jump_profile.id != 0 {
                visited.insert(jump_profile.id);
            }
            hops.push(JumpHop {
                username: target.username,
                host: target.host,
                port: target.port,
                identity_file: jump_profile.identity_file.clone(),
                encrypted_password: jump_profile.password.clone(),
            });
        } else {
            hops.push(JumpHop {
                username: target.username,
                host: target.host,
                port: target.port,
                identity_file: host.identity_file.clone(),
                encrypted_password: String::new(),
            });
        }
    }
}

fn parse_relay_commands(raw: &str) -> Vec<String> {
    raw.split(['\n', '\r'])
        .flat_map(|line| line.split(";;"))
        .map(str::trim)
        .filter(|cmd| !cmd.is_empty())
        .map(str::to_owned)
        .collect()
}

fn strip_ansi(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }
                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn trailing_line(text: &str) -> &str {
    let after_nl = text.rsplit('\n').next().unwrap_or(text);
    let after_cr = after_nl.rsplit('\r').next().unwrap_or(after_nl);
    after_cr.trim()
}

fn is_password_prompt(text: &str) -> bool {
    let line = trailing_line(text);
    if !(line.ends_with(':') || line.ends_with('：')) {
        return false;
    }
    let lower = line.to_lowercase();
    if ["verification", "otp", "2fa", "token", "passphrase", "pin"]
        .iter()
        .any(|blocked| lower.contains(blocked))
    {
        return false;
    }
    lower.contains("password")
        || lower.contains("passwd")
        || line.contains("密码")
        || line.contains("口令")
}

fn is_relay_auth_failure(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "authentication failure",
        "incorrect password",
        "sorry, try again",
        "su: sorry",
        "permission denied",
        "not in the sudoers",
        "认证失败",
        "鉴定故障",
        "密码错误",
        "对不起，请重试",
    ]
    .iter()
    .any(|needle| lower.contains(needle) || text.contains(needle))
}

fn is_shell_prompt(text: &str) -> bool {
    let line = trailing_line(text);
    if line.is_empty() || is_password_prompt(text) {
        return false;
    }
    line.ends_with('$')
        || line.ends_with('#')
        || line.ends_with('%')
        || line.ends_with('>')
        || line.ends_with('❯')
        || line.ends_with('➜')
        || line.starts_with('➜')
        || line.starts_with('❯')
}

fn strip_echoed_command<'a>(text: &'a str, command: &str) -> &'a str {
    let trimmed_cmd = command.trim();
    if trimmed_cmd.is_empty() {
        return text;
    }
    if let Some((first, rest)) = text.split_once('\n') {
        let first_clean = first.trim_matches(['\r', ' ', '\t']);
        if first_clean == trimmed_cmd || first_clean.ends_with(trimmed_cmd) {
            return rest;
        }
    }
    let clean = text.trim_matches(['\r', ' ', '\t']);
    if clean == trimmed_cmd {
        return "";
    }
    text
}

fn build_target_ssh_command(host: &HostProfile) -> String {
    let address = host
        .address
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&host.address);
    let destination = format!("{}@{address}", host.username);
    shell_words::join([
        "ssh".to_string(),
        "-p".to_string(),
        host.port.to_string(),
        destination,
    ])
}

fn append_tail(buf: &mut String, chunk: &str, max_bytes: usize) {
    buf.push_str(chunk);
    if buf.len() > max_bytes {
        let mut cut = buf.len() - max_bytes;
        while cut < buf.len() && !buf.is_char_boundary(cut) {
            cut += 1;
        }
        buf.drain(..cut);
    }
}

fn parse_jump_targets(jump_host: &str, default_user: &str) -> Vec<JumpTarget> {
    jump_host
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|item| {
            let item = item.strip_prefix("ssh://").unwrap_or(item);
            let (username, endpoint) = match item.split_once('@') {
                Some((user, rest)) if !user.is_empty() => (user.to_string(), rest),
                _ => (default_user.to_string(), item),
            };
            let (host, port) = if let Some(rest) = endpoint.strip_prefix('[') {
                if let Some((addr, after)) = rest.split_once(']') {
                    let port = after
                        .strip_prefix(':')
                        .and_then(|p| p.parse::<u16>().ok())
                        .unwrap_or(22);
                    (addr.to_string(), port)
                } else {
                    return None;
                }
            } else if endpoint.matches(':').count() == 1 {
                let (addr, port_str) = endpoint.rsplit_once(':')?;
                let port = port_str.parse::<u16>().ok().unwrap_or(22);
                (addr.to_string(), port)
            } else {
                (endpoint.to_string(), 22)
            };
            Some(JumpTarget {
                username,
                host,
                port,
            })
        })
        .collect()
}

fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(relative) = raw.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(relative);
    }
    PathBuf::from(raw)
}

async fn request_prompt(events: &Sender<SshUiEvent>, kind: SshPromptKind) -> Option<String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    if events
        .send(SshUiEvent::Prompt(SshPrompt {
            kind,
            reply: reply_tx,
        }))
        .await
        .is_err()
    {
        return None;
    }
    reply_rx.await.ok().flatten()
}

pub(crate) struct ClientHandler {
    host: String,
    port: u16,
    events: Sender<SshUiEvent>,
    key_error: Arc<Mutex<Option<String>>>,
    forwarded_tx: Option<mpsc::UnboundedSender<russh::Channel<russh::client::Msg>>>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _connected_address: &str,
        _connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        if let Some(tx) = &self.forwarded_tx {
            let _ = tx.send(channel);
        }
        Ok(())
    }

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let pubkey: Cow<'_, russh::keys::PublicKey> = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => Cow::Borrowed(key),
            PublicKeyOrCertificate::Certificate(cert) => {
                Cow::Owned(russh::keys::PublicKey::new(cert.public_key().clone(), ""))
            }
        };

        match check_known_hosts(&self.host, self.port, pubkey.as_ref()) {
            Ok(true) => Ok(true),
            Ok(false) | Err(russh::keys::Error::IO(_)) => {
                let fingerprint = pubkey.fingerprint(Default::default()).to_string();
                let answer = request_prompt(
                    &self.events,
                    SshPromptKind::HostKey {
                        host: self.host.clone(),
                        port: self.port,
                        fingerprint,
                    },
                )
                .await;
                if answer.as_deref() == Some("trust") {
                    if let Some(home) = dirs::home_dir() {
                        let _ = std::fs::create_dir_all(home.join(".ssh"));
                    }
                    let _ = learn_known_hosts(&self.host, self.port, pubkey.as_ref());
                    Ok(true)
                } else {
                    *self.key_error.lock().unwrap() = Some(format!(
                        "Host key verification rejected for {}:{}",
                        self.host, self.port
                    ));
                    Ok(false)
                }
            }
            Err(russh::keys::Error::KeyChanged { line }) => {
                *self.key_error.lock().unwrap() = Some(format!(
                    "WARNING: Host key for {}:{} has changed (known_hosts line {line}). Connection aborted.",
                    self.host, self.port
                ));
                Ok(false)
            }
            Err(error) => {
                *self.key_error.lock().unwrap() = Some(format!("Host key check failed: {error}"));
                Ok(false)
            }
        }
    }
}

fn client_config() -> Arc<client::Config> {
    Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(3600)),
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    })
}

async fn authenticate_session(
    session: &mut client::Handle<ClientHandler>,
    username: &str,
    identity_file: &str,
    password: Option<&str>,
    endpoint: &str,
    events: &Sender<SshUiEvent>,
) -> Result<(), String> {
    // 1. Explicit identity file
    if !identity_file.trim().is_empty() {
        let path = expand_tilde(identity_file.trim());
        let key_pair = load_key_with_prompt(&path, events).await?;
        let rsa_hash = session
            .best_supported_rsa_hash()
            .await
            .map_err(|e| e.to_string())?
            .flatten();
        let res = session
            .authenticate_publickey(
                username,
                PrivateKeyWithHashAlg::new(Arc::new(key_pair), rsa_hash),
            )
            .await
            .map_err(|e| e.to_string())?;
        if res.success() {
            return Ok(());
        }
    }

    // 2. Saved password (if configured)
    if let Some(pw) = password.filter(|pw| !pw.is_empty())
        && let Ok(res) = session.authenticate_password(username, pw).await
        && res.success()
    {
        return Ok(());
    }

    if identity_file.trim().is_empty() {
        // 3. Try SSH agent on Unix
        #[cfg(unix)]
        if let Ok(sock) = std::env::var("SSH_AUTH_SOCK")
            && !sock.is_empty()
            && let Ok(mut agent) =
                russh::keys::agent::client::AgentClient::connect_uds(sock).await
            && let Ok(identities) = agent.request_identities().await
        {
            let rsa_hash = session
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten();
            for identity in identities {
                if let Ok(res) = session
                    .authenticate_publickey_with(
                        username,
                        identity.public_key().into_owned(),
                        rsa_hash,
                        &mut agent,
                    )
                    .await
                    && res.success()
                {
                    return Ok(());
                }
            }
        }

        // 3. Try default keys in ~/.ssh/
        if let Some(home) = dirs::home_dir() {
            let ssh_dir = home.join(".ssh");
            for name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
                let candidate = ssh_dir.join(name);
                if candidate.is_file()
                    && let Ok(key_pair) = load_secret_key(&candidate, None)
                {
                    let rsa_hash = session
                        .best_supported_rsa_hash()
                        .await
                        .ok()
                        .flatten()
                        .flatten();
                    if let Ok(res) = session
                        .authenticate_publickey(
                            username,
                            PrivateKeyWithHashAlg::new(Arc::new(key_pair), rsa_hash),
                        )
                        .await
                        && res.success()
                    {
                        return Ok(());
                    }
                }
            }
        }
    }

    // 4. Fallback to interactive password authentication
    for _ in 0..3 {
        let Some(password) = request_prompt(
            events,
            SshPromptKind::Password {
                endpoint: endpoint.to_string(),
            },
        )
        .await
        else {
            return Err("Authentication cancelled.".into());
        };
        let res = session
            .authenticate_password(username, password)
            .await
            .map_err(|e| e.to_string())?;
        if res.success() {
            return Ok(());
        }
    }

    Err(format!("Authentication failed for {endpoint}"))
}

async fn load_key_with_prompt(
    path: &Path,
    events: &Sender<SshUiEvent>,
) -> Result<russh::keys::PrivateKey, String> {
    match load_secret_key(path, None) {
        Ok(key) => Ok(key),
        Err(first_err) => {
            let err_msg = first_err.to_string();
            let maybe_encrypted = err_msg.to_lowercase().contains("encrypt")
                || err_msg.to_lowercase().contains("passphrase")
                || err_msg.to_lowercase().contains("decrypt");
            if !maybe_encrypted {
                return Err(format!("Failed to load key {}: {first_err}", path.display()));
            }
            let key_path = path.display().to_string();
            let Some(passphrase) =
                request_prompt(events, SshPromptKind::Passphrase { key_path }).await
            else {
                return Err("Key passphrase prompt cancelled.".into());
            };
            load_secret_key(path, Some(&passphrase))
                .map_err(|e| format!("Failed to decrypt key {}: {e}", path.display()))
        }
    }
}

async fn connect_jump_chain(
    jumps: &[JumpHop],
    fallback_identity: &str,
    config: &Arc<client::Config>,
    events: &Sender<SshUiEvent>,
) -> Result<Option<(client::Handle<ClientHandler>, String)>, String> {
    let mut current_session: Option<client::Handle<ClientHandler>> = None;
    let mut last_endpoint = String::new();
    for jump in jumps {
        let endpoint = format!("{}@{}:{}", jump.username, jump.host, jump.port);
        let _ = events
            .send(SshUiEvent::Connecting(format!("Jump: {endpoint}")))
            .await;

        let key_error = Arc::new(Mutex::new(None));
        let handler = ClientHandler {
            host: jump.host.clone(),
            port: jump.port,
            events: events.clone(),
            key_error: Arc::clone(&key_error),
            forwarded_tx: None,
        };
        let mut session = match current_session.take() {
            None => client::connect(
                Arc::clone(config),
                (jump.host.as_str(), jump.port),
                handler,
            )
            .await
            .map_err(|e| {
                key_error
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap_or_else(|| e.to_string())
            })?,
            Some(prev) => {
                let channel = prev
                    .channel_open_direct_tcpip(jump.host.clone(), u32::from(jump.port), "127.0.0.1", 0)
                    .await
                    .map_err(|e| format!("Jump tunnel to {endpoint} failed: {e}"))?;
                client::connect_stream(Arc::clone(config), channel.into_stream(), handler)
                    .await
                    .map_err(|e| {
                        key_error
                            .lock()
                            .unwrap()
                            .take()
                            .unwrap_or_else(|| e.to_string())
                    })?
            }
        };
        let identity = if jump.identity_file.trim().is_empty() {
            fallback_identity
        } else {
            jump.identity_file.as_str()
        };
        let saved_password = crate::vault::decrypt_secret(&jump.encrypted_password);
        authenticate_session(
            &mut session,
            &jump.username,
            identity,
            saved_password.as_deref(),
            &endpoint,
            events,
        )
        .await?;
        current_session = Some(session);
        last_endpoint = endpoint;
    }
    Ok(current_session.map(|session| (session, last_endpoint)))
}

pub(crate) async fn connect_target(
    host: &HostProfile,
    all_hosts: &[HostProfile],
    events: &Sender<SshUiEvent>,
) -> Result<client::Handle<ClientHandler>, String> {
    connect_target_with_forwarding(host, all_hosts, events, None).await
}

pub(crate) async fn connect_target_with_forwarding(
    host: &HostProfile,
    all_hosts: &[HostProfile],
    events: &Sender<SshUiEvent>,
    forwarded_tx: Option<mpsc::UnboundedSender<russh::Channel<russh::client::Msg>>>,
) -> Result<client::Handle<ClientHandler>, String> {
    let config = client_config();
    let jumps = resolve_jump_chain(host, all_hosts);
    if !jumps.is_empty() && !parse_relay_commands(&host.jump_relay_command).is_empty() {
        return Err(
            "Jump relay commands are only supported in interactive SSH terminal sessions, not SFTP or port forwarding."
                .into(),
        );
    }

    let current_session = connect_jump_chain(&jumps, &host.identity_file, &config, events)
        .await?
        .map(|(session, _)| session);

    let endpoint = host.endpoint();
    let _ = events.send(SshUiEvent::Connecting(endpoint.clone())).await;

    let key_error = Arc::new(Mutex::new(None));
    let handler = ClientHandler {
        host: host.address.clone(),
        port: host.port,
        events: events.clone(),
        key_error: Arc::clone(&key_error),
        forwarded_tx,
    };

    let mut session = match current_session {
        None => client::connect(
            config,
            (host.address.as_str(), host.port),
            handler,
        )
        .await
        .map_err(|e| {
            key_error
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| e.to_string())
        })?,
        Some(prev) => {
            let channel = prev
                .channel_open_direct_tcpip(
                    host.address.clone(),
                    u32::from(host.port),
                    "127.0.0.1",
                    0,
                )
                .await
                .map_err(|e| format!("Jump tunnel to {endpoint} failed: {e}"))?;
            client::connect_stream(config, channel.into_stream(), handler)
                .await
                .map_err(|e| {
                    key_error
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap_or_else(|| e.to_string())
                })?
        }
    };

    let saved_password = crate::vault::decrypt_secret(&host.password);
    authenticate_session(
        &mut session,
        &host.username,
        &host.identity_file,
        saved_password.as_deref(),
        &endpoint,
        events,
    )
    .await?;

    Ok(session)
}

enum RelayOutcome {
    Completed,
    UserTakeover,
    Closed(Option<i32>),
    Terminated,
}

async fn wait_for_initial_shell(
    channel: &mut russh::Channel<russh::client::Msg>,
    sink: &TerminalSink,
    rx: &mut mpsc::UnboundedReceiver<SshCommand>,
) -> RelayOutcome {
    let mut buf = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    let mut saw_output = false;

    loop {
        let idle_duration = if is_shell_prompt(&buf) {
            Duration::from_millis(180)
        } else if saw_output {
            Duration::from_millis(900)
        } else {
            Duration::from_secs(4)
        };
        let sleep_until = (tokio::time::Instant::now() + idle_duration).min(deadline);

        tokio::select! {
            _ = tokio::time::sleep_until(sleep_until) => {
                return RelayOutcome::Completed;
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(SshCommand::Data(bytes)) => {
                        if channel.data(&bytes[..]).await.is_err() {
                            return RelayOutcome::Closed(None);
                        }
                        return RelayOutcome::UserTakeover;
                    }
                    Some(SshCommand::Resize(size)) => {
                        let _ = channel
                            .window_change(
                                u32::try_from(size.columns.max(1)).unwrap_or(u32::MAX),
                                u32::try_from(size.rows.max(1)).unwrap_or(u32::MAX),
                                0,
                                0,
                            )
                            .await;
                    }
                    Some(SshCommand::Terminate) | None => {
                        return RelayOutcome::Terminated;
                    }
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        saw_output = true;
                        sink.output(&data);
                        append_tail(&mut buf, &strip_ansi(&data), 4096);
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        return RelayOutcome::Closed(i32::try_from(exit_status).ok());
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        return RelayOutcome::Closed(None);
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn run_relay_step(
    command: &str,
    jump_endpoint: &str,
    saved_relay_password: Option<&str>,
    saved_pw_used: &mut bool,
    channel: &mut russh::Channel<russh::client::Msg>,
    sink: &TerminalSink,
    events: &Sender<SshUiEvent>,
    rx: &mut mpsc::UnboundedReceiver<SshCommand>,
) -> RelayOutcome {
    let _ = events
        .send(SshUiEvent::Connecting(format!("Relay: {command}")))
        .await;
    let line = format!("{command}\r");
    if channel.data(line.as_bytes()).await.is_err() {
        return RelayOutcome::Closed(None);
    }

    let mut step_buf = String::new();
    let mut awaiting_post_auth = false;
    let mut prompt_attempts = 0u8;
    let mut deadline = tokio::time::Instant::now() + Duration::from_secs(12);

    loop {
        let effective = strip_echoed_command(&step_buf, command);

        if is_password_prompt(effective) {
            if prompt_attempts >= 3 {
                return RelayOutcome::UserTakeover;
            }
            prompt_attempts += 1;

            let password = if !*saved_pw_used
                && let Some(saved) = saved_relay_password.filter(|pw| !pw.is_empty())
            {
                *saved_pw_used = true;
                saved.to_owned()
            } else {
                let Some(pw) = request_prompt(
                    events,
                    SshPromptKind::RelayPassword {
                        command: command.to_owned(),
                        endpoint: jump_endpoint.to_owned(),
                    },
                )
                .await
                else {
                    return RelayOutcome::UserTakeover;
                };
                pw
            };

            let payload = format!("{password}\r");
            if channel.data(payload.as_bytes()).await.is_err() {
                return RelayOutcome::Closed(None);
            }
            step_buf.clear();
            awaiting_post_auth = true;
            deadline = tokio::time::Instant::now() + Duration::from_secs(12);
            continue;
        }

        if awaiting_post_auth && is_relay_auth_failure(effective) {
            return RelayOutcome::UserTakeover;
        }

        let ready_to_settle = is_shell_prompt(effective);
        let sleep_until = if ready_to_settle {
            (tokio::time::Instant::now() + Duration::from_millis(200)).min(deadline)
        } else {
            deadline
        };

        tokio::select! {
            _ = tokio::time::sleep_until(sleep_until) => {
                if ready_to_settle {
                    return RelayOutcome::Completed;
                }
                return RelayOutcome::UserTakeover;
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(SshCommand::Data(bytes)) => {
                        if channel.data(&bytes[..]).await.is_err() {
                            return RelayOutcome::Closed(None);
                        }
                        return RelayOutcome::UserTakeover;
                    }
                    Some(SshCommand::Resize(size)) => {
                        let _ = channel
                            .window_change(
                                u32::try_from(size.columns.max(1)).unwrap_or(u32::MAX),
                                u32::try_from(size.rows.max(1)).unwrap_or(u32::MAX),
                                0,
                                0,
                            )
                            .await;
                    }
                    Some(SshCommand::Terminate) | None => {
                        return RelayOutcome::Terminated;
                    }
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        sink.output(&data);
                        append_tail(&mut step_buf, &strip_ansi(&data), 8192);
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        return RelayOutcome::Closed(i32::try_from(exit_status).ok());
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        return RelayOutcome::Closed(None);
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn run_ssh_session(
    host: HostProfile,
    all_hosts: Vec<HostProfile>,
    sink: TerminalSink,
    events: Sender<SshUiEvent>,
    mut rx: mpsc::UnboundedReceiver<SshCommand>,
) {
    let jumps = resolve_jump_chain(&host, &all_hosts);
    let relay_commands = parse_relay_commands(&host.jump_relay_command);
    let use_relay = !jumps.is_empty() && !relay_commands.is_empty();

    let (session, jump_endpoint) = if use_relay {
        let config = client_config();
        match connect_jump_chain(&jumps, &host.identity_file, &config, &events).await {
            Ok(Some((session, endpoint))) => (session, Some(endpoint)),
            Ok(None) => {
                let _ = events
                    .send(SshUiEvent::Error("No jump host configured for relay.".into()))
                    .await;
                sink.exited(ExitReport::new(None));
                return;
            }
            Err(error) => {
                let _ = events.send(SshUiEvent::Error(error)).await;
                sink.exited(ExitReport::new(None));
                return;
            }
        }
    } else {
        match connect_target(&host, &all_hosts, &events).await {
            Ok(session) => (session, None),
            Err(error) => {
                let _ = events.send(SshUiEvent::Error(error)).await;
                sink.exited(ExitReport::new(None));
                return;
            }
        }
    };

    let mut channel = match session.channel_open_session().await {
        Ok(channel) => channel,
        Err(error) => {
            let _ = events
                .send(SshUiEvent::Error(format!("Could not open SSH channel: {error}")))
                .await;
            sink.exited(ExitReport::new(None));
            return;
        }
    };

    let size = sink.size();
    if let Err(error) = channel
        .request_pty(
            true,
            "xterm-256color",
            u32::try_from(size.columns.max(1)).unwrap_or(u32::MAX),
            u32::try_from(size.rows.max(1)).unwrap_or(u32::MAX),
            0,
            0,
            &[],
        )
        .await
    {
        let _ = events
            .send(SshUiEvent::Error(format!("PTY request failed: {error}")))
            .await;
        sink.exited(ExitReport::new(None));
        return;
    }

    if let Err(error) = channel.request_shell(true).await {
        let _ = events
            .send(SshUiEvent::Error(format!("Shell request failed: {error}")))
            .await;
        sink.exited(ExitReport::new(None));
        return;
    }

    let mut pending_target_password: Option<(String, Instant)> = None;
    let mut target_tail = String::new();

    if use_relay {
        let jump_ep = jump_endpoint.unwrap_or_default();
        let saved_relay_pw = crate::vault::decrypt_secret(&host.jump_relay_password);
        let mut saved_pw_used = false;
        let mut relay_ok = true;

        match wait_for_initial_shell(&mut channel, &sink, &mut rx).await {
            RelayOutcome::Completed => {}
            RelayOutcome::UserTakeover => {
                relay_ok = false;
            }
            RelayOutcome::Closed(code) => {
                sink.exited(ExitReport::new(code));
                return;
            }
            RelayOutcome::Terminated => {
                let _ = channel.eof().await;
                let _ = channel.close().await;
                let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
                sink.exited(ExitReport::new(None));
                return;
            }
        }

        if relay_ok {
            for cmd in &relay_commands {
                match run_relay_step(
                    cmd,
                    &jump_ep,
                    saved_relay_pw.as_deref(),
                    &mut saved_pw_used,
                    &mut channel,
                    &sink,
                    &events,
                    &mut rx,
                )
                .await
                {
                    RelayOutcome::Completed => {}
                    RelayOutcome::UserTakeover => {
                        relay_ok = false;
                        break;
                    }
                    RelayOutcome::Closed(code) => {
                        sink.exited(ExitReport::new(code));
                        return;
                    }
                    RelayOutcome::Terminated => {
                        let _ = channel.eof().await;
                        let _ = channel.close().await;
                        let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
                        sink.exited(ExitReport::new(None));
                        return;
                    }
                }
            }
        }

        if relay_ok {
            let target_cmd = build_target_ssh_command(&host);
            let _ = events
                .send(SshUiEvent::Connecting(host.endpoint()))
                .await;
            let payload = format!("{target_cmd}\r");
            if channel.data(payload.as_bytes()).await.is_err() {
                sink.exited(ExitReport::new(None));
                return;
            }
            if let Some(target_pw) = crate::vault::decrypt_secret(&host.password)
                && !target_pw.is_empty()
            {
                pending_target_password =
                    Some((target_pw, Instant::now() + Duration::from_secs(15)));
            }
        }
    }

    let _ = events.send(SshUiEvent::Connected).await;

    let mut exit_code: Option<i32> = None;
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    Some(SshCommand::Data(bytes)) => {
                        pending_target_password = None;
                        if channel.data(&bytes[..]).await.is_err() {
                            break;
                        }
                    }
                    Some(SshCommand::Resize(size)) => {
                        let _ = channel
                            .window_change(
                                u32::try_from(size.columns.max(1)).unwrap_or(u32::MAX),
                                u32::try_from(size.rows.max(1)).unwrap_or(u32::MAX),
                                0,
                                0,
                            )
                            .await;
                    }
                    Some(SshCommand::Terminate) | None => {
                        let _ = channel.eof().await;
                        let _ = channel.close().await;
                        let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
                        break;
                    }
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        sink.output(&data);
                        if let Some((pw, deadline)) = pending_target_password.take() {
                            if Instant::now() <= deadline {
                                append_tail(&mut target_tail, &strip_ansi(&data), 2048);
                                if is_password_prompt(&target_tail) {
                                    let payload = format!("{pw}\r");
                                    if channel.data(payload.as_bytes()).await.is_err() {
                                        break;
                                    }
                                    target_tail.clear();
                                } else {
                                    pending_target_password = Some((pw, deadline));
                                }
                            }
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        sink.output(&data);
                        if let Some((pw, deadline)) = pending_target_password.take() {
                            if Instant::now() <= deadline {
                                append_tail(&mut target_tail, &strip_ansi(&data), 2048);
                                if is_password_prompt(&target_tail) {
                                    let payload = format!("{pw}\r");
                                    if channel.data(payload.as_bytes()).await.is_err() {
                                        break;
                                    }
                                    target_tail.clear();
                                } else {
                                    pending_target_password = Some((pw, deadline));
                                }
                            }
                        }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = i32::try_from(exit_status).ok();
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    sink.exited(ExitReport::new(exit_code));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::HostIcon;

    fn sample_host(id: u64, name: &str, address: &str, user: &str, port: u16) -> HostProfile {
        HostProfile {
            id,
            name: name.into(),
            protocol: HostProtocol::Ssh,
            icon: HostIcon::Server,
            address: address.into(),
            username: user.into(),
            port,
            baud_rate: 115200,
            identity_file: String::new(),
            password: String::new(),
            jump_host: String::new(),
            jump_host_id: None,
            jump_relay_command: String::new(),
            jump_relay_password: String::new(),
            group: String::new(),
            tags: Vec::new(),
            last_connected: None,
        }
    }

    #[test]
    fn parses_single_and_chained_jump_targets() {
        assert_eq!(
            parse_jump_targets("bastion.example.com", "deploy"),
            vec![JumpTarget {
                username: "deploy".into(),
                host: "bastion.example.com".into(),
                port: 22,
            }]
        );
        assert_eq!(
            parse_jump_targets("ops@jump1:2222, [2001:db8::1]:2200", "root"),
            vec![
                JumpTarget {
                    username: "ops".into(),
                    host: "jump1".into(),
                    port: 2222,
                },
                JumpTarget {
                    username: "root".into(),
                    host: "2001:db8::1".into(),
                    port: 2200,
                },
            ]
        );
    }

    #[test]
    fn resolves_jump_chain_with_credentials_and_cycle_guard() {
        let mut jump1 = sample_host(1, "Outer", "outer.example.com", "ops", 2222);
        jump1.password = "enc:v1:outer".into();
        jump1.identity_file = "~/.ssh/outer".into();

        let mut jump2 = sample_host(2, "Inner", "inner.example.com", "bastion", 22);
        jump2.jump_host_id = Some(1);
        jump2.password = "enc:v1:inner".into();

        let mut target = sample_host(3, "App", "10.0.0.10", "admin", 22);
        target.jump_host_id = Some(2);

        let all = vec![jump1.clone(), jump2.clone(), target.clone()];
        let chain = resolve_jump_chain(&target, &all);
        assert_eq!(
            chain,
            vec![
                JumpHop {
                    username: "ops".into(),
                    host: "outer.example.com".into(),
                    port: 2222,
                    identity_file: "~/.ssh/outer".into(),
                    encrypted_password: "enc:v1:outer".into(),
                },
                JumpHop {
                    username: "bastion".into(),
                    host: "inner.example.com".into(),
                    port: 22,
                    identity_file: String::new(),
                    encrypted_password: "enc:v1:inner".into(),
                },
            ]
        );

        // Cycle protection: jump1 -> jump2 -> jump1
        let mut cyc1 = jump1;
        cyc1.jump_host_id = Some(2);
        let cyc_all = vec![cyc1, jump2, target.clone()];
        assert_eq!(resolve_jump_chain(&target, &cyc_all).len(), 2);
    }

    #[test]
    fn detects_password_prompts_shell_prompts_and_auth_failures() {
        assert!(is_password_prompt("Password: "));
        assert!(is_password_prompt("[sudo] password for deploy: "));
        assert!(is_password_prompt("admin@10.0.0.10's password:"));
        assert!(is_password_prompt("密码："));
        assert!(is_password_prompt("[sudo] deploy 的密码："));
        assert!(!is_password_prompt("Verification code: "));
        assert!(!is_password_prompt("Enter passphrase for key '/home/a/.ssh/id_ed25519': "));
        assert!(!is_password_prompt("user@host:~$ "));

        assert!(is_shell_prompt("user@bastion:~$ "));
        assert!(is_shell_prompt("[admin@bastion ~]$ "));
        assert!(is_shell_prompt("root@bastion:/# "));
        assert!(is_shell_prompt("➜  ~ "));
        assert!(!is_shell_prompt("Password: "));

        assert!(is_relay_auth_failure("su: Authentication failure\r\n"));
        assert!(is_relay_auth_failure("Sorry, try again.\r\n"));
        assert!(is_relay_auth_failure("su: 认证失败\r\n"));
        assert!(!is_relay_auth_failure("Welcome to Ubuntu 24.04"));

        assert_eq!(
            strip_ansi(b"\x1b[32madmin@jump\x1b[0m:\x1b[34m~\x1b[0m$ "),
            "admin@jump:~$ "
        );
        assert_eq!(
            strip_echoed_command("su admin\r\nPassword: ", "su admin"),
            "Password: "
        );
        assert_eq!(
            parse_relay_commands("su - admin\n;; sudo -i"),
            vec!["su - admin", "sudo -i"]
        );
    }

    #[test]
    fn builds_target_ssh_command() {
        let host = sample_host(1, "Prod", "10.0.0.10", "deploy", 2222);
        assert_eq!(
            build_target_ssh_command(&host),
            "ssh -p 2222 deploy@10.0.0.10"
        );
        let ipv6 = sample_host(2, "V6", "[2001:db8::1]", "root", 22);
        assert_eq!(
            build_target_ssh_command(&ipv6),
            "ssh -p 22 root@2001:db8::1"
        );
    }
}
