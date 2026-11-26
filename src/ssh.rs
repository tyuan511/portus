use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_channel::Sender;
use gpui_terminal::{ExitReport, GridSize, TerminalSink, Transport};
use russh::keys::known_hosts::{check_known_hosts, learn_known_hosts};
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate, load_secret_key};
use russh::{ChannelMsg, Disconnect, client};
use tokio::runtime::Runtime;
use tokio::sync::{Notify, mpsc, oneshot};

use crate::settings::{HostProfile, HostProtocol};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub(crate) fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("portus-ssh")
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DockerContainer {
    pub name: String,
    pub image: String,
    pub status: String,
    pub running: bool,
    pub ports: String,
    pub cpu_pct: f32,
    pub mem_used: String,
    pub mem_pct: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ServerTelemetry {
    pub cpu_pct: u8,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub disk_pct: u8,
    pub disk_used: String,
    pub load_avg: String,
    pub uptime: String,
    pub latency_ms: u32,
    pub containers: Vec<DockerContainer>,
}

#[derive(Debug)]
pub enum SshUiEvent {
    Connecting(String),
    Connected,
    Telemetry(ServerTelemetry),
    Prompt(SshPrompt),
    Error(String),
}

enum SshCommand {
    Data(Vec<u8>),
    Resize(GridSize),
    Terminate,
}

struct CommandState {
    data: Vec<u8>,
    resize: Option<GridSize>,
    terminate: bool,
}

struct CommandQueue {
    state: Mutex<CommandState>,
    notify: Notify,
    closed: AtomicBool,
}

impl CommandQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(CommandState {
                data: Vec::with_capacity(4096),
                resize: None,
                terminate: false,
            }),
            notify: Notify::new(),
            closed: AtomicBool::new(false),
        }
    }

    fn push_data(&self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return true;
        }
        if self.closed.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.terminate {
            return false;
        }
        state.data.extend_from_slice(bytes);
        drop(state);
        self.notify.notify_one();
        true
    }

    fn push_resize(&self, size: GridSize) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        if let Ok(mut state) = self.state.lock()
            && !state.terminate
        {
            state.resize = Some(size);
        }
        self.notify.notify_one();
    }

    fn terminate(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut state) = self.state.lock() {
            state.terminate = true;
            state.data.clear();
            state.resize = None;
        }
        self.notify.notify_one();
    }

    async fn recv(&self) -> Option<SshCommand> {
        loop {
            if let Ok(mut state) = self.state.lock() {
                if !state.data.is_empty() {
                    let data = std::mem::replace(&mut state.data, Vec::with_capacity(4096));
                    return Some(SshCommand::Data(data));
                }
                if let Some(size) = state.resize.take() {
                    return Some(SshCommand::Resize(size));
                }
                if state.terminate || self.closed.load(Ordering::Acquire) {
                    return Some(SshCommand::Terminate);
                }
            }
            self.notify.notified().await;
        }
    }

    fn close(&self) {
        self.terminate();
    }
}

pub struct SshTransport {
    commands: Arc<CommandQueue>,
}

pub struct DockerLogRequest {
    pub container: String,
    pub tail: usize,
    pub timestamps: bool,
    pub reply: oneshot::Sender<Result<String, String>>,
}

pub enum SshControlRequest {
    DockerLogs(DockerLogRequest),
}

#[derive(Clone)]
pub struct SshControl {
    tx: mpsc::UnboundedSender<SshControlRequest>,
}

impl SshControl {
    pub fn channel() -> (Self, mpsc::UnboundedReceiver<SshControlRequest>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    pub fn fetch_docker_logs(
        &self,
        container: String,
        tail: usize,
        timestamps: bool,
    ) -> oneshot::Receiver<Result<String, String>> {
        let (reply, rx) = oneshot::channel();
        let req = DockerLogRequest {
            container,
            tail,
            timestamps,
            reply,
        };
        if let Err(err) = self.tx.send(SshControlRequest::DockerLogs(req)) {
            let SshControlRequest::DockerLogs(req) = err.0;
            let _ = req.reply.send(Err("SSH session is closed".to_owned()));
        }
        rx
    }
}

impl SshTransport {
    pub fn spawn(
        host: HostProfile,
        all_hosts: Vec<HostProfile>,
        sink: TerminalSink,
        events: Sender<SshUiEvent>,
        control_rx: mpsc::UnboundedReceiver<SshControlRequest>,
    ) -> gpui_terminal::Result<Self> {
        let commands = Arc::new(CommandQueue::new());
        let receiver = Arc::clone(&commands);
        runtime().spawn(async move {
            run_ssh_session(host, all_hosts, sink, events, receiver, control_rx).await;
        });
        Ok(Self { commands })
    }
}

impl Drop for SshTransport {
    fn drop(&mut self) {
        self.commands.close();
    }
}

impl Transport for SshTransport {
    fn write(&mut self, bytes: &[u8]) -> gpui_terminal::Result<()> {
        if self.commands.push_data(bytes) {
            Ok(())
        } else {
            Err(gpui_terminal::Error::Closed)
        }
    }

    fn resize(&mut self, size: GridSize) -> gpui_terminal::Result<()> {
        self.commands.push_resize(size);
        Ok(())
    }

    fn terminate(&mut self) -> gpui_terminal::Result<()> {
        self.commands.terminate();
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

fn strip_ansi(bytes: &[u8]) -> Cow<'_, str> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        let text = String::from_utf8_lossy(bytes);
        return Cow::Owned(strip_ansi_text(&text));
    };
    if !text.as_bytes().contains(&0x1b) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(strip_ansi_text(text))
}

fn strip_ansi_text(text: &str) -> String {
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
    let lower = line.to_ascii_lowercase();
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
    let lower = text.to_ascii_lowercase();
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
    static CONFIG: OnceLock<Arc<client::Config>> = OnceLock::new();
    Arc::clone(CONFIG.get_or_init(|| {
        Arc::new(client::Config {
            // Keep interactive sessions responsive while allowing large pastes
            // and command output to stay in flight without waiting for a
            // window update after every packet.
            window_size: 8 * 1024 * 1024,
            maximum_packet_size: 64 * 1024,
            channel_buffer_size: 256,
            inactivity_timeout: Some(Duration::from_secs(3600)),
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            nodelay: true,
            ..Default::default()
        })
    }))
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
            .map_err(|error| error.to_string())?
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
            let rsa_hash = session
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten();
            for name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
                let candidate = ssh_dir.join(name);
                if candidate.is_file()
                    && let Ok(key_pair) = load_secret_key(&candidate, None)
                {
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
            let lower_err = err_msg.to_ascii_lowercase();
            let maybe_encrypted = ["encrypt", "passphrase", "decrypt"]
                .iter()
                .any(|needle| lower_err.contains(needle));
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

enum TargetHandoff {
    Ready { remaining_pw: Option<String> },
    Closed(Option<i32>),
    Terminated,
}

/// Suppresses the echoed `ssh ...` command line and any automated target password
/// prompt so the terminal starts cleanly with the final target host's output.
async fn wait_for_target_handoff(
    target_cmd: &str,
    saved_target_pw: Option<&str>,
    channel: &mut russh::Channel<russh::client::Msg>,
    sink: &TerminalSink,
    rx: &CommandQueue,
) -> TargetHandoff {
    let mut raw_buf = Vec::new();
    let mut text_buf = String::new();
    let mut echo_skipped = false;
    let mut pw_to_send = saved_target_pw.map(str::to_owned);
    let mut waiting_after_pw = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);

    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                if !raw_buf.is_empty() {
                    sink.output(&raw_buf);
                }
                return TargetHandoff::Ready { remaining_pw: pw_to_send };
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(SshCommand::Data(bytes)) => {
                        if !raw_buf.is_empty() {
                            sink.output(&raw_buf);
                        }
                        if channel.data_bytes(bytes).await.is_err() {
                            return TargetHandoff::Closed(None);
                        }
                        return TargetHandoff::Ready { remaining_pw: None };
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
                        return TargetHandoff::Terminated;
                    }
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        raw_buf.extend_from_slice(&data);
                        append_tail(&mut text_buf, &strip_ansi(&data), 4096);

                        if !echo_skipped {
                            // Wait until the echoed `ssh ...` command line ends with a newline.
                            if let Some(nl) = raw_buf.iter().position(|&b| b == b'\n') {
                                raw_buf.drain(..=nl);
                                let cleaned = strip_echoed_command(&text_buf, target_cmd).to_owned();
                                text_buf = cleaned;
                                echo_skipped = true;
                            } else {
                                continue;
                            }
                        }

                        if waiting_after_pw {
                            // Discard the newline echoed right after sending the password.
                            if let Some(nl) = raw_buf.iter().position(|&b| b == b'\n') {
                                raw_buf.drain(..=nl);
                                text_buf.clear();
                                waiting_after_pw = false;
                            } else {
                                continue;
                            }
                        }

                        if is_password_prompt(&text_buf) {
                            if let Some(pw) = pw_to_send.take() {
                                let payload = format!("{pw}\r");
                                if channel.data_bytes(payload.into_bytes()).await.is_err() {
                                    return TargetHandoff::Closed(None);
                                }
                                raw_buf.clear();
                                text_buf.clear();
                                waiting_after_pw = true;
                                continue;
                            } else {
                                // Manual password required: reveal prompt immediately.
                                if !raw_buf.is_empty() {
                                    sink.output(&raw_buf);
                                }
                                return TargetHandoff::Ready { remaining_pw: None };
                            }
                        }

                        // Once real target output (banner, prompt, or host-key question) arrives,
                        // flush it cleanly and hand off to the live interactive loop.
                        let has_visible = !text_buf.trim().is_empty();
                        if echo_skipped && has_visible {
                            // If we still have a saved password, wait briefly for a possible password prompt
                            // only if the visible text looks like a partial SSH preamble rather than a shell prompt.
                            if pw_to_send.is_some()
                                && !is_shell_prompt(&text_buf)
                                && !text_buf.to_lowercase().contains("yes/no")
                                && !text_buf.contains("Last login")
                            {
                                continue;
                            }
                            if !raw_buf.is_empty() {
                                sink.output(&raw_buf);
                            }
                            return TargetHandoff::Ready { remaining_pw: pw_to_send };
                        }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        if !raw_buf.is_empty() {
                            sink.output(&raw_buf);
                        }
                        return TargetHandoff::Closed(i32::try_from(exit_status).ok());
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        if !raw_buf.is_empty() {
                            sink.output(&raw_buf);
                        }
                        return TargetHandoff::Closed(None);
                    }
                    _ => {}
                }
            }
        }
    }
}

const MAX_MUTED_BUF: usize = 65_536;

fn push_muted(buf: &mut Vec<u8>, data: &[u8]) {
    if buf.len() + data.len() > MAX_MUTED_BUF {
        let excess = (buf.len() + data.len()).saturating_sub(MAX_MUTED_BUF);
        let drain = excess.min(buf.len());
        buf.drain(..drain);
    }
    buf.extend_from_slice(data);
}

async fn wait_for_initial_shell(
    channel: &mut russh::Channel<russh::client::Msg>,
    muted_buf: &mut Vec<u8>,
    rx: &CommandQueue,
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
                        if channel.data_bytes(bytes).await.is_err() {
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
                        push_muted(muted_buf, &data);
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
    resolved_relay_pw: &mut Option<String>,
    channel: &mut russh::Channel<russh::client::Msg>,
    muted_buf: &mut Vec<u8>,
    events: &Sender<SshUiEvent>,
    rx: &CommandQueue,
) -> RelayOutcome {
    let _ = events
        .send(SshUiEvent::Connecting(format!("Relay: {command}")))
        .await;
    let line = format!("{command}\r");
    if channel.data_bytes(line.into_bytes()).await.is_err() {
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
            *resolved_relay_pw = Some(password.clone());

            let payload = format!("{password}\r");
            if channel.data_bytes(payload.into_bytes()).await.is_err() {
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
                        if channel.data_bytes(bytes).await.is_err() {
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
                        push_muted(muted_buf, &data);
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
    rx: Arc<CommandQueue>,
    mut control_rx: mpsc::UnboundedReceiver<SshControlRequest>,
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
    let mut relay_ok = false;
    let mut resolved_relay_pw: Option<String> = None;
    let mut relay_target_cmd = String::new();
    let mut relay_target_pw: Option<String> = None;

    if use_relay {
        let jump_ep = jump_endpoint.unwrap_or_default();
        let saved_relay_pw = crate::vault::decrypt_secret(&host.jump_relay_password);
        resolved_relay_pw = saved_relay_pw.clone().filter(|pw| !pw.is_empty());
        let mut saved_pw_used = false;
        relay_ok = true;
        let mut muted_buf = Vec::new();

        match wait_for_initial_shell(&mut channel, &mut muted_buf, &rx).await {
            RelayOutcome::Completed => {}
            RelayOutcome::UserTakeover => {
                relay_ok = false;
            }
            RelayOutcome::Closed(code) => {
                if !muted_buf.is_empty() {
                    sink.output(&muted_buf);
                }
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
                    &mut resolved_relay_pw,
                    &mut channel,
                    &mut muted_buf,
                    &events,
                    &rx,
                )
                .await
                {
                    RelayOutcome::Completed => {}
                    RelayOutcome::UserTakeover => {
                        relay_ok = false;
                        break;
                    }
                    RelayOutcome::Closed(code) => {
                        if !muted_buf.is_empty() {
                            sink.output(&muted_buf);
                        }
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

        if !relay_ok && !muted_buf.is_empty() {
            // Relay paused or user took over: reveal buffered context so user can interact.
            sink.output(&muted_buf);
        }

        if relay_ok {
            let target_cmd = build_target_ssh_command(&host);
            relay_target_cmd = target_cmd.clone();
            let _ = events
                .send(SshUiEvent::Connecting(host.endpoint()))
                .await;
            let payload = format!("{target_cmd}\r");
            if channel.data_bytes(payload.into_bytes()).await.is_err() {
                sink.exited(ExitReport::new(None));
                return;
            }
            let saved_target_pw = crate::vault::decrypt_secret(&host.password)
                .filter(|pw| !pw.is_empty());
            relay_target_pw = saved_target_pw.clone();
            match wait_for_target_handoff(
                &target_cmd,
                saved_target_pw.as_deref(),
                &mut channel,
                &sink,
                &rx,
            )
            .await
            {
                TargetHandoff::Ready { remaining_pw } => {
                    if let Some(pw) = remaining_pw {
                        pending_target_password =
                            Some((pw, Instant::now() + Duration::from_secs(12)));
                    }
                }
                TargetHandoff::Closed(code) => {
                    sink.exited(ExitReport::new(code));
                    return;
                }
                TargetHandoff::Terminated => {
                    let _ = channel.eof().await;
                    let _ = channel.close().await;
                    let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
                    sink.exited(ExitReport::new(None));
                    return;
                }
            }
        }
    }

    let _ = events.send(SshUiEvent::Connected).await;

    // Keep channel writes independent from channel reads. A large PTY paste can
    // fill the remote window while the shell is consuming input, and awaiting
    // the whole payload in the read loop would prevent window updates and
    // remote output from being processed.
    let (mut channel_read, channel_write) = channel.split();
    let (write_tx, mut write_rx) = mpsc::channel::<SshCommand>(8);
    let mut writer_task = tokio::spawn(async move {
        while let Some(command) = write_rx.recv().await {
            let result = match command {
                SshCommand::Data(bytes) => channel_write.data_bytes(bytes).await,
                SshCommand::Resize(size) => channel_write
                    .window_change(
                        u32::try_from(size.columns.max(1)).unwrap_or(u32::MAX),
                        u32::try_from(size.rows.max(1)).unwrap_or(u32::MAX),
                        0,
                        0,
                    )
                    .await,
                SshCommand::Terminate => {
                    let _ = channel_write.eof().await;
                    let _ = channel_write.close().await;
                    break;
                }
            };
            if result.is_err() {
                break;
            }
        }
    });

    let session = Arc::new(session);
    let mut control_task = None;
    let telemetry_task = if !use_relay {
        let ctrl_session = Arc::clone(&session);
        control_task = Some(tokio::spawn(async move {
            while let Some(req) = control_rx.recv().await {
                match req {
                    SshControlRequest::DockerLogs(log_req) => {
                        let s = Arc::clone(&ctrl_session);
                        tokio::spawn(async move {
                            let res = exec_docker_logs(
                                &s,
                                &log_req.container,
                                log_req.tail,
                                log_req.timestamps,
                            )
                            .await;
                            let _ = log_req.reply.send(res);
                        });
                    }
                }
            }
        }));
        let tele_session = Arc::clone(&session);
        let tele_events = events.clone();
        Some(tokio::spawn(async move {
            poll_telemetry_loop(tele_session, tele_events).await;
        }))
    } else if relay_ok && !relay_target_cmd.is_empty() {
        let tele_session = Arc::clone(&session);
        let tele_events = events.clone();
        Some(tokio::spawn(async move {
            poll_relay_telemetry_loop(
                tele_session,
                relay_commands,
                resolved_relay_pw,
                relay_target_cmd,
                relay_target_pw,
                tele_events,
                control_rx,
            )
            .await;
        }))
    } else {
        control_task = Some(tokio::spawn(async move {
            while let Some(req) = control_rx.recv().await {
                match req {
                    SshControlRequest::DockerLogs(log_req) => {
                        let _ = log_req.reply.send(Err(
                            "Docker logs are unavailable on this relay session.".to_owned(),
                        ));
                    }
                }
            }
        }));
        None
    };

    let mut exit_code: Option<i32> = None;
    loop {
        tokio::select! {
            result = &mut writer_task => {
                let _ = result;
                break;
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(SshCommand::Data(bytes)) => {
                        pending_target_password = None;
                        if write_tx.send(SshCommand::Data(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Some(SshCommand::Resize(size)) => {
                        if write_tx.send(SshCommand::Resize(size)).await.is_err() {
                            break;
                        }
                    }
                    Some(SshCommand::Terminate) | None => {
                        if let Some(task) = telemetry_task.as_ref() {
                            task.abort();
                        }
                        if let Some(task) = control_task.as_ref() {
                            task.abort();
                        }
                        let _ = write_tx.try_send(SshCommand::Terminate);
                        let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
                        break;
                    }
                }
            }
            msg = channel_read.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) |
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        sink.output(&data);
                        if let Some((pw, deadline)) = pending_target_password.take() {
                            if Instant::now() <= deadline {
                                append_tail(&mut target_tail, &strip_ansi(&data), 2048);
                                if is_password_prompt(&target_tail) {
                                    let payload = format!("{pw}\r");
                                    if write_tx
                                        .send(SshCommand::Data(payload.into_bytes()))
                                        .await
                                        .is_err()
                                    {
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

    if let Some(task) = telemetry_task {
        task.abort();
    }
    if let Some(task) = control_task {
        task.abort();
    }
    writer_task.abort();
    sink.exited(ExitReport::new(exit_code));
}

const TELEMETRY_PROBE_CMD: &str = "L=$(cut -d' ' -f1-3 /proc/loadavg 2>/dev/null); \
    M=$(free -m 2>/dev/null | awk 'NR==2 {print $3, $2}'); \
    D=$(df -h / 2>/dev/null | awk 'NR==2 {print $5, $3}'); \
    U=$(uptime -p 2>/dev/null | sed 's/^up //'); \
    C=$(awk '/^cpu / {u=$2+$4; t=u+$5; if(t>0) printf \"%d\", (u*100)/t}' /proc/stat 2>/dev/null); \
    printf '%s|%s|%s|%s|%s\\n' \"$C\" \"$M\" \"$D\" \"$L\" \"$U\"; \
    DK=''; \
    if command -v docker >/dev/null 2>&1; then \
      if docker ps >/dev/null 2>&1; then DK='docker'; \
      elif sudo -n docker ps >/dev/null 2>&1; then DK='sudo -n docker'; fi; \
    fi; \
    if [ -n \"$DK\" ]; then \
      echo '===DOCKER_PS==='; \
      $DK ps -a --format '{{.Names}}\\t{{.Image}}\\t{{.Status}}\\t{{.State}}\\t{{.Ports}}' 2>/dev/null | head -n 30; \
      echo '===DOCKER_STATS==='; \
      $DK stats --no-stream --format '{{.Name}}\\t{{.CPUPerc}}\\t{{.MemUsage}}\\t{{.MemPerc}}' 2>/dev/null | head -n 30; \
    fi";

const TELEMETRY_FRAME_BEGIN: &str = "__BWK_TELE_B__";
const TELEMETRY_FRAME_END: &str = "__BWK_TELE_E__";
const DOCKER_LOG_FRAME_BEGIN: &str = "__BWK_LOG_B__";
const DOCKER_LOG_FRAME_END: &str = "__BWK_LOG_E__";
const MAX_DOCKER_LOG_BYTES: usize = 512 * 1024;

fn build_docker_logs_cmd(container: &str, tail: usize, timestamps: bool) -> String {
    let quoted = shell_words::quote(container);
    let ts_flag = if timestamps { " --timestamps" } else { "" };
    format!(
        "DK=''; \
        if command -v docker >/dev/null 2>&1; then \
          if docker ps >/dev/null 2>&1; then DK='docker'; \
          elif sudo -n docker ps >/dev/null 2>&1; then DK='sudo -n docker'; fi; \
        fi; \
        if [ -z \"$DK\" ]; then \
          echo 'Docker is not available or permission denied.'; \
        else \
          $DK logs --tail {tail}{ts_flag} -- {quoted} 2>&1; \
        fi"
    )
}

async fn exec_docker_logs(
    session: &client::Handle<ClientHandler>,
    container: &str,
    tail: usize,
    timestamps: bool,
) -> Result<String, String> {
    let mut ch = session
        .channel_open_session()
        .await
        .map_err(|e| format!("Failed to open SSH channel: {e}"))?;
    let cmd = build_docker_logs_cmd(container, tail, timestamps);
    ch.exec(true, cmd)
        .await
        .map_err(|e| format!("Failed to execute docker logs: {e}"))?;

    let mut out = Vec::with_capacity(16384);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                return Err("Timed out fetching Docker logs.".to_owned());
            }
            msg = ch.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        if out.len() < MAX_DOCKER_LOG_BYTES {
                            let remaining = MAX_DOCKER_LOG_BYTES - out.len();
                            out.extend_from_slice(&data[..data.len().min(remaining)]);
                        }
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
        }
    }
    Ok(strip_ansi(&out).into_owned())
}

async fn poll_telemetry_loop(
    session: Arc<client::Handle<ClientHandler>>,
    events: Sender<SshUiEvent>,
) {
    const MAX_PROBE_BYTES: usize = 16384;
    let cadence = Duration::from_secs(6);
    let mut interval = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(1),
        cadence,
    );
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut out = Vec::with_capacity(MAX_PROBE_BYTES);
    loop {
        interval.tick().await;
        let started = Instant::now();
        let Ok(mut ch) = session.channel_open_session().await else {
            break;
        };
        if ch.exec(true, TELEMETRY_PROBE_CMD).await.is_err() {
            break;
        }
        out.clear();
        while let Some(msg) = ch.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    if out.len() < MAX_PROBE_BYTES {
                        let remaining = MAX_PROBE_BYTES - out.len();
                        out.extend_from_slice(&data[..data.len().min(remaining)]);
                    }
                }
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            }
        }
        let latency_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
        if let Ok(text) = std::str::from_utf8(&out)
            && let Some(tele) = parse_telemetry(text.trim(), latency_ms)
        {
            if events.send(SshUiEvent::Telemetry(tele)).await.is_err() {
                break;
            }
        }
    }
}

async fn run_framed_relay_cmd(
    ch: &mut russh::Channel<russh::client::Msg>,
    cmd: &str,
    begin_marker: &str,
    end_marker: &str,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Option<String>, ()> {
    let framed = format!("printf '{begin_marker}\\n'; {cmd}; printf '{end_marker}\\n'\r");
    if ch.data_bytes(framed.into_bytes()).await.is_err() {
        return Err(());
    }
    let mut text_buf = String::with_capacity(max_bytes.min(16384));
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                return Ok(None);
            }
            msg = ch.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        append_tail(&mut text_buf, &strip_ansi(&data), max_bytes);
                        if let Some(payload) = extract_framed_block(&text_buf, begin_marker, end_marker) {
                            return Ok(Some(payload.to_owned()));
                        }
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        return Err(());
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn open_relay_target_shell(
    session: &client::Handle<ClientHandler>,
    relay_commands: &[String],
    relay_password: Option<&str>,
    target_cmd: &str,
    target_password: Option<&str>,
) -> Result<russh::Channel<russh::client::Msg>, String> {
    let mut ch = session
        .channel_open_session()
        .await
        .map_err(|error| format!("Failed to open relay SSH channel: {error}"))?;
    ch.request_pty(true, "dumb", 200, 40, 0, 0, &[])
        .await
        .map_err(|error| format!("Failed to request relay PTY: {error}"))?;
    ch.request_shell(true)
        .await
        .map_err(|error| format!("Failed to open relay shell: {error}"))?;

    if !wait_headless_prompt(&mut ch, None, None, Duration::from_secs(5)).await {
        return Err("Relay shell did not become ready.".to_owned());
    }
    for command in relay_commands {
        let line = format!("{command}\r");
        ch.data_bytes(line.into_bytes())
            .await
            .map_err(|error| format!("Failed to run relay command: {error}"))?;
        if !wait_headless_prompt(
            &mut ch,
            Some(command),
            relay_password,
            Duration::from_secs(10),
        )
        .await
        {
            return Err(format!("Relay command did not complete: {command}"));
        }
    }

    let line = format!("{target_cmd}\r");
    ch.data_bytes(line.into_bytes())
        .await
        .map_err(|error| format!("Failed to connect to target host: {error}"))?;
    if !wait_headless_prompt(
        &mut ch,
        Some(target_cmd),
        target_password,
        Duration::from_secs(12),
    )
    .await
    {
        return Err("Target shell did not become ready through the relay.".to_owned());
    }

    let _ = ch.data_bytes(b"stty -echo 2>/dev/null\r".to_vec()).await;
    drain_headless_briefly(&mut ch, Duration::from_millis(250)).await;
    Ok(ch)
}

async fn exec_relay_docker_logs(
    session: Arc<client::Handle<ClientHandler>>,
    relay_commands: Vec<String>,
    relay_password: Option<String>,
    target_cmd: String,
    target_password: Option<String>,
    container: String,
    tail: usize,
    timestamps: bool,
) -> Result<String, String> {
    let mut ch = open_relay_target_shell(
        &session,
        &relay_commands,
        relay_password.as_deref(),
        &target_cmd,
        target_password.as_deref(),
    )
    .await?;
    let command = build_docker_logs_cmd(&container, tail, timestamps);
    match run_framed_relay_cmd(
        &mut ch,
        &command,
        DOCKER_LOG_FRAME_BEGIN,
        DOCKER_LOG_FRAME_END,
        MAX_DOCKER_LOG_BYTES,
        Duration::from_secs(15),
    )
    .await
    {
        Ok(Some(payload)) => {
            let _ = ch.close().await;
            Ok(payload)
        }
        Ok(None) => {
            let _ = ch.close().await;
            Err("Timed out fetching Docker logs.".to_owned())
        }
        Err(()) => Err("Relay SSH channel closed while fetching Docker logs.".to_owned()),
    }
}

async fn poll_relay_telemetry_loop(
    session: Arc<client::Handle<ClientHandler>>,
    relay_commands: Vec<String>,
    relay_password: Option<String>,
    target_cmd: String,
    target_password: Option<String>,
    events: Sender<SshUiEvent>,
    mut control_rx: mpsc::UnboundedReceiver<SshControlRequest>,
) {
    let Ok(mut ch) = open_relay_target_shell(
        &session,
        &relay_commands,
        relay_password.as_deref(),
        &target_cmd,
        target_password.as_deref(),
    )
    .await
    else {
        return;
    };

    let mut interval = tokio::time::interval(Duration::from_secs(6));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let started = Instant::now();
                match run_framed_relay_cmd(
                    &mut ch,
                    TELEMETRY_PROBE_CMD,
                    TELEMETRY_FRAME_BEGIN,
                    TELEMETRY_FRAME_END,
                    24576,
                    Duration::from_secs(10),
                )
                .await
                {
                    Ok(Some(payload)) => {
                        let latency_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
                        if let Some(tele) = parse_telemetry(&payload, latency_ms)
                            && events.send(SshUiEvent::Telemetry(tele)).await.is_err()
                        {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(()) => break,
                }
            }
            req = control_rx.recv() => {
                let Some(req) = req else {
                    break;
                };
                match req {
                    SshControlRequest::DockerLogs(log_req) => {
                        let log_session = Arc::clone(&session);
                        let log_relay_commands = relay_commands.clone();
                        let log_relay_password = relay_password.clone();
                        let log_target_cmd = target_cmd.clone();
                        let log_target_password = target_password.clone();
                        tokio::spawn(async move {
                            let result = exec_relay_docker_logs(
                                log_session,
                                log_relay_commands,
                                log_relay_password,
                                log_target_cmd,
                                log_target_password,
                                log_req.container,
                                log_req.tail,
                                log_req.timestamps,
                            )
                            .await;
                            let _ = log_req.reply.send(result);
                        });
                    }
                }
            }
        }
    }
    let _ = ch.close().await;
}

async fn wait_headless_prompt(
    ch: &mut russh::Channel<russh::client::Msg>,
    sent_cmd: Option<&str>,
    password: Option<&str>,
    timeout: Duration,
) -> bool {
    let mut buf = String::new();
    let mut pw_sent = false;
    let mut saw_output = false;
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let effective = match sent_cmd {
            Some(cmd) => strip_echoed_command(&buf, cmd),
            None => buf.as_str(),
        };

        if is_password_prompt(effective) {
            if pw_sent {
                return false;
            }
            let Some(pw) = password.filter(|p| !p.is_empty()) else {
                return false;
            };
            pw_sent = true;
            let payload = format!("{pw}\r");
            if ch.data_bytes(payload.into_bytes()).await.is_err() {
                return false;
            }
            buf.clear();
            continue;
        }

        if pw_sent && is_relay_auth_failure(effective) {
            return false;
        }

        let prompt_ready = is_shell_prompt(effective);
        let idle = if prompt_ready {
            Duration::from_millis(200)
        } else if saw_output && sent_cmd.is_none() {
            Duration::from_millis(900)
        } else {
            timeout
        };
        let sleep_until = (tokio::time::Instant::now() + idle).min(deadline);

        tokio::select! {
            _ = tokio::time::sleep_until(sleep_until) => {
                return prompt_ready || (sent_cmd.is_none() && saw_output);
            }
            msg = ch.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        saw_output = true;
                        append_tail(&mut buf, &strip_ansi(&data), 4096);
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        return false;
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn drain_headless_briefly(
    ch: &mut russh::Channel<russh::client::Msg>,
    duration: Duration,
) {
    let deadline = tokio::time::Instant::now() + duration;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            msg = ch.wait() => {
                if matches!(msg, Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None) {
                    break;
                }
            }
        }
    }
}

fn extract_framed_block<'a>(raw: &'a str, begin_marker: &str, end_marker: &str) -> Option<&'a str> {
    let mut start_offset = None;
    let mut offset = 0;
    for line in raw.split_inclusive('\n') {
        let trimmed = line.trim();
        if start_offset.is_none() {
            if trimmed == begin_marker {
                start_offset = Some(offset + line.len());
            }
        } else if trimmed == end_marker {
            let start = start_offset?;
            if offset >= start {
                return Some(raw[start..offset].trim());
            }
        }
        offset += line.len();
    }
    None
}

fn extract_framed_telemetry(raw: &str) -> Option<&str> {
    extract_framed_block(raw, TELEMETRY_FRAME_BEGIN, TELEMETRY_FRAME_END)
}

fn parse_telemetry(text: &str, latency_ms: u32) -> Option<ServerTelemetry> {
    let mut lines = text.lines();
    let line = lines.next()?;
    let (cpu, rest) = line.split_once('|')?;
    let (memory, rest) = rest.split_once('|')?;
    let (disk, rest) = rest.split_once('|')?;
    let (load, uptime) = rest.split_once('|')?;

    let cpu_pct = cpu.trim().parse::<u8>().unwrap_or(0).min(100);
    let mut mem_parts = memory.split_whitespace();
    let mem_used_mb = mem_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mem_total_mb = mem_parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut disk_parts = disk.split_whitespace();
    let disk_pct = disk_parts
        .next()
        .and_then(|s| s.trim_end_matches('%').parse::<u8>().ok())
        .unwrap_or(0)
        .min(100);
    let disk_used = disk_parts.next().unwrap_or("—").to_owned();
    let load_avg = load.trim().to_owned();
    let uptime = uptime.trim().to_owned();
    if mem_total_mb == 0 && load_avg.is_empty() && disk_pct == 0 {
        return None;
    }

    let mut containers = Vec::new();
    let mut stats_by_name = std::collections::HashMap::<String, (f32, String, f32)>::new();
    let mut section = 0u8;
    for raw in lines {
        let trimmed = raw.trim();
        if trimmed == "===DOCKER_PS===" {
            section = 1;
            continue;
        }
        if trimmed == "===DOCKER_STATS===" {
            section = 2;
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if section == 1 {
            let mut cols = raw.split('\t');
            let name = cols.next().unwrap_or("").trim().to_owned();
            if name.is_empty() {
                continue;
            }
            let image = cols.next().unwrap_or("").trim().to_owned();
            let status = cols.next().unwrap_or("").trim().to_owned();
            let state = cols.next().unwrap_or("").trim();
            let ports = cols.next().unwrap_or("").trim().to_owned();
            containers.push(DockerContainer {
                name,
                image,
                status,
                running: state.eq_ignore_ascii_case("running"),
                ports,
                cpu_pct: 0.0,
                mem_used: "—".into(),
                mem_pct: 0.0,
            });
        } else if section == 2 {
            let mut cols = raw.split('\t');
            let name = cols.next().unwrap_or("").trim().to_owned();
            let cpu_str = cols.next().unwrap_or("0%").trim().trim_end_matches('%');
            let mem_usage = cols.next().unwrap_or("—").trim();
            let mem_pct_str = cols.next().unwrap_or("0%").trim().trim_end_matches('%');
            let cpu_val = cpu_str.parse::<f32>().unwrap_or(0.0);
            let mem_used = mem_usage
                .split('/')
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("—")
                .to_owned();
            let mem_pct = mem_pct_str.parse::<f32>().unwrap_or(0.0).clamp(0.0, 100.0);
            if !name.is_empty() {
                stats_by_name.insert(name, (cpu_val, mem_used, mem_pct));
            }
        }
    }

    for c in &mut containers {
        if let Some((cpu_pct, mem_used, mem_pct)) = stats_by_name.get(&c.name) {
            c.cpu_pct = *cpu_pct;
            c.mem_used = mem_used.clone();
            c.mem_pct = *mem_pct;
        }
    }

    Some(ServerTelemetry {
        cpu_pct,
        mem_used_mb,
        mem_total_mb,
        disk_pct,
        disk_used,
        load_avg,
        uptime,
        latency_ms,
        containers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::HostIcon;
    use gpui_terminal::{Terminal, TerminalBuilder};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;

    #[test]
    fn command_queue_coalesces_input_and_keeps_latest_resize() {
        let queue = CommandQueue::new();
        assert!(queue.push_data(b"printf "));
        assert!(queue.push_data(b"ready\n"));
        queue.push_resize(GridSize {
            columns: 80,
            rows: 24,
        });
        queue.push_resize(GridSize {
            columns: 120,
            rows: 40,
        });

        assert!(matches!(
            runtime().block_on(queue.recv()),
            Some(SshCommand::Data(bytes)) if bytes == b"printf ready\n"
        ));
        assert!(matches!(
            runtime().block_on(queue.recv()),
            Some(SshCommand::Resize(GridSize { columns: 120, rows: 40 }))
        ));
    }

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
        }
    }

    struct PtyBenchCapture {
        count_output: AtomicBool,
        output_x: AtomicUsize,
        total: AtomicUsize,
        tail: Mutex<Vec<u8>>,
        armed: AtomicBool,
        echoed: Sender<Instant>,
    }

    struct PtyBenchTerminal {
        terminal: Terminal<SshTransport>,
        capture: Arc<PtyBenchCapture>,
        prompt_task: tokio::task::JoinHandle<()>,
        echoed: Receiver<Instant>,
    }

    fn wait_for_output(capture: &PtyBenchCapture, marker: &[u8], timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if capture
                .tail
                .lock()
                .map(|tail| tail.windows(marker.len()).any(|window| window == marker))
                .unwrap_or(false)
            {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn require_output(capture: &PtyBenchCapture, marker: &[u8], timeout: Duration, label: &str) {
        if wait_for_output(capture, marker, timeout) {
            return;
        }
        let tail = capture
            .tail
            .lock()
            .map(|tail| String::from_utf8_lossy(&tail).into_owned())
            .unwrap_or_default();
        panic!(
            "PTY benchmark {label} marker timeout, bytes={}, tail={tail:?}",
            capture.total.load(Ordering::Relaxed)
        );
    }

    fn start_pty_bench_terminal(host: HostProfile, all_hosts: Vec<HostProfile>) -> PtyBenchTerminal {
        let (echoed_tx, echoed_rx) = mpsc::channel();
        let capture = Arc::new(PtyBenchCapture {
            count_output: AtomicBool::new(false),
            output_x: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            tail: Mutex::new(Vec::with_capacity(16 * 1024)),
            armed: AtomicBool::new(false),
            echoed: echoed_tx,
        });
        let hook_capture = Arc::clone(&capture);
        let (events, event_rx) = async_channel::bounded(64);
        let prompt_task = runtime().spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                if let SshUiEvent::Prompt(prompt) = event {
                    let answer = match prompt.kind {
                        SshPromptKind::HostKey { .. } => Some("trust".to_owned()),
                        _ => None,
                    };
                    let _ = prompt.reply.send(answer);
                } else if let SshUiEvent::Error(error) = event {
                    eprintln!("pty benchmark SSH error: {error}");
                }
            }
        });
        let (_, control_rx) = SshControl::channel();
        let terminal = TerminalBuilder::new()
            .size(GridSize {
                columns: 120,
                rows: 40,
            })
            .on_output(move |bytes| {
                hook_capture.total.fetch_add(bytes.len(), Ordering::Relaxed);
                if hook_capture.count_output.load(Ordering::Acquire) {
                    let count = bytes.iter().filter(|&&b| b == b'X').count();
                    hook_capture.output_x.fetch_add(count, Ordering::Relaxed);
                }
                if hook_capture.armed.load(Ordering::Acquire)
                    && bytes.contains(&b'Q')
                {
                    let _ = hook_capture.echoed.send(Instant::now());
                }
                if let Ok(mut tail) = hook_capture.tail.lock() {
                    tail.extend_from_slice(bytes);
                    if tail.len() > 16 * 1024 {
                        let drain = tail.len() - 16 * 1024;
                        tail.drain(..drain);
                    }
                }
            })
            .connect(|sink| SshTransport::spawn(host, all_hosts, sink, events, control_rx))
            .expect("PTY benchmark transport");
        PtyBenchTerminal {
            terminal,
            capture,
            prompt_task,
            echoed: echoed_rx,
        }
    }

    fn write_shell_command(terminal: &Terminal<SshTransport>, command: &str) {
        terminal
            .write(format!("{command}\r").as_bytes())
            .expect("PTY benchmark command write");
    }

    fn prepare_pty_shell(bench: &PtyBenchTerminal) {
        write_shell_command(
            &bench.terminal,
            "printf '\\n\\137\\137PORTUS_PTY_READY\\137\\137\\n'",
        );
        let ready = wait_for_output(
            &bench.capture,
            b"__PORTUS_PTY_READY__",
            Duration::from_secs(10)
        );
        if !ready {
            let tail = bench
                .capture
                .tail
                .lock()
                .map(|tail| String::from_utf8_lossy(&tail).into_owned())
                .unwrap_or_default();
            panic!(
                "PTY benchmark shell did not become ready, bytes={}, tail={tail:?}",
                bench.capture.total.load(Ordering::Relaxed)
            );
        }
    }

    fn finish_pty_bench(bench: PtyBenchTerminal) {
        let _ = bench.terminal.terminate();
        bench.prompt_task.abort();
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

    #[test]
    #[ignore = "opt-in local sshd benchmark"]
    fn benchmark_russh_connect_path_when_enabled() {
        let Ok(port) = std::env::var("PORTUS_BENCH_RUSSH_PORT") else {
            return;
        };
        let Ok(port) = port.parse::<u16>() else {
            panic!("invalid PORTUS_BENCH_RUSSH_PORT");
        };
        let identity = std::env::var("PORTUS_BENCH_RUSSH_IDENTITY")
            .expect("PORTUS_BENCH_RUSSH_IDENTITY");
        let samples = std::env::var("PORTUS_BENCH_RUSSH_SAMPLES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(5);
        let (events, event_rx) = async_channel::unbounded();
        let prompt_task = runtime().spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                if let SshUiEvent::Prompt(prompt) = event {
                    let answer = match prompt.kind {
                        SshPromptKind::HostKey { .. } => Some("trust".to_owned()),
                        _ => None,
                    };
                    let _ = prompt.reply.send(answer);
                }
            }
        });
        let mut host = sample_host(1, "bench", "127.0.0.1", "yuantang", port);
        host.identity_file = identity;
        if let Ok(jump_port) = std::env::var("PORTUS_BENCH_RUSSH_JUMP_PORT") {
            host.jump_host = format!("yuantang@127.0.0.1:{jump_port}");
        }
        let label = std::env::var("PORTUS_BENCH_RUSSH_LABEL")
            .unwrap_or_else(|_| "direct".to_owned());
        let mut timings = Vec::with_capacity(samples);
        for _ in 0..samples {
            let started = Instant::now();
            let session = runtime()
                .block_on(connect_target(&host, &[], &events))
                .expect("russh benchmark connection");
            timings.push(started.elapsed());
            let _ = runtime().block_on(session.disconnect(Disconnect::ByApplication, "", "en"));
        }
        prompt_task.abort();
        timings.sort_unstable();
        let median = timings[timings.len() / 2];
        let p95 = timings[(timings.len() * 95).div_ceil(100).saturating_sub(1)];
        eprintln!(
            "russh_{label}_connect_ms n={} median={:.3} p95={:.3} samples={:?}",
            timings.len(),
            median.as_secs_f64() * 1000.0,
            p95.as_secs_f64() * 1000.0,
            timings
                .iter()
                .map(|sample| sample.as_secs_f64() * 1000.0)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore = "opt-in local sshd stream benchmark"]
    fn benchmark_russh_raw_stream_path_when_enabled() {
        let Ok(port) = std::env::var("PORTUS_BENCH_RUSSH_PORT") else {
            return;
        };
        let port = port.parse::<u16>().expect("invalid benchmark port");
        let identity = std::env::var("PORTUS_BENCH_RUSSH_IDENTITY")
            .expect("PORTUS_BENCH_RUSSH_IDENTITY");
        let mode = std::env::var("PORTUS_BENCH_RUSSH_RAW")
            .unwrap_or_else(|_| "paste".to_owned());
        let samples = std::env::var("PORTUS_BENCH_RUSSH_SAMPLES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(3);
        let (events, event_rx) = async_channel::unbounded();
        let prompt_task = runtime().spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                if let SshUiEvent::Prompt(prompt) = event {
                    let answer = match prompt.kind {
                        SshPromptKind::HostKey { .. } => Some("trust".to_owned()),
                        _ => None,
                    };
                    let _ = prompt.reply.send(answer);
                }
            }
        });
        let mut host = sample_host(1, "bench", "127.0.0.1", "yuantang", port);
        host.identity_file = identity;
        let mut timings = Vec::with_capacity(samples);
        for _ in 0..samples {
            let elapsed = runtime().block_on(async {
                let session = connect_target(&host, &[], &events).await?;
                let mut channel = session.channel_open_session().await.map_err(|error| error.to_string())?;
                let command = if mode == "output" {
                    "yes X | head -c 16777216"
                } else {
                    "dd of=/dev/null bs=64k count=128 status=none"
                };
                channel.exec(true, command).await.map_err(|error| error.to_string())?;
                let started = Instant::now();
                if mode == "output" {
                    while let Some(message) = channel.wait().await {
                        if matches!(message, ChannelMsg::Eof | ChannelMsg::Close) {
                            break;
                        }
                    }
                } else {
                    channel
                        .data_bytes(vec![b'X'; 8 * 1024 * 1024])
                        .await
                        .map_err(|error| error.to_string())?;
                    channel.eof().await.map_err(|error| error.to_string())?;
                    while let Some(message) = channel.wait().await {
                        if matches!(message, ChannelMsg::Eof | ChannelMsg::Close) {
                            break;
                        }
                    }
                }
                let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
                Ok::<_, String>(started.elapsed())
            })
            .unwrap_or_else(|error| panic!("russh raw stream benchmark failed: {error}"));
            timings.push(elapsed);
        }
        prompt_task.abort();
        timings.sort_unstable();
        let median = timings[timings.len() / 2];
        let p95 = timings[(timings.len() * 95).div_ceil(100).saturating_sub(1)];
        eprintln!(
            "russh_raw_{}_ms n={} median={:.3} p95={:.3} samples={:?}",
            mode,
            timings.len(),
            median.as_secs_f64() * 1000.0,
            p95.as_secs_f64() * 1000.0,
            timings
                .iter()
                .map(|sample| sample.as_secs_f64() * 1000.0)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore = "opt-in local sshd end-to-end PTY benchmark"]
    fn benchmark_russh_end_to_end_pty_when_enabled() {
        let Ok(port) = std::env::var("PORTUS_BENCH_RUSSH_PORT") else {
            return;
        };
        let port = port.parse::<u16>().expect("invalid benchmark port");
        let identity = std::env::var("PORTUS_BENCH_RUSSH_IDENTITY")
            .expect("PORTUS_BENCH_RUSSH_IDENTITY");
        let samples = std::env::var("PORTUS_BENCH_RUSSH_SAMPLES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(3);
        let jump_port = std::env::var("PORTUS_BENCH_RUSSH_JUMP_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok());

        let username = std::env::var("PORTUS_BENCH_RUSSH_USER")
            .or_else(|_| std::env::var("USER"))
            .expect("benchmark sshd username");
        let scenarios = ["paste", "output", "output_block", "interactive"];
        for route in ["direct", "jump"] {
            if route == "jump" && jump_port.is_none() {
                continue;
            }
            let mut startup_timings = Vec::with_capacity(samples);
            let mut scenario_timings = scenarios
                .iter()
                .map(|scenario| (*scenario, Vec::with_capacity(samples)))
                .collect::<std::collections::HashMap<_, _>>();

            for _ in 0..samples {
                let make_host = || {
                    let mut host = sample_host(1, "pty-bench", "127.0.0.1", &username, port);
                    host.identity_file.clone_from(&identity);
                    if route == "jump" {
                        host.jump_host = format!("{username}@127.0.0.1:{}", jump_port.unwrap());
                    }
                    host
                };

                let started = Instant::now();
                let bench = start_pty_bench_terminal(make_host(), Vec::new());
                prepare_pty_shell(&bench);
                startup_timings.push(started.elapsed());
                finish_pty_bench(bench);

                let payload = vec![b'X'; 8 * 1024 * 1024];
                let paste = {
                    let bench = start_pty_bench_terminal(make_host(), Vec::new());
                    prepare_pty_shell(&bench);
                    write_shell_command(
                        &bench.terminal,
                        "stty -echo -icanon min 1 time 0; printf '\\n\\137\\137BWK_PASTE_ARMED\\137\\137\\n'; head -c 8388608 >/dev/null; printf '\\n\\137\\137BWK_PASTE_DONE\\137\\137\\n'",
                    );
                    require_output(
                        &bench.capture,
                        b"__BWK_PASTE_ARMED__",
                        Duration::from_secs(10),
                        "paste armed",
                    );
                    let started = Instant::now();
                    bench
                        .terminal
                        .write(&payload)
                        .expect("PTY paste write");
                    require_output(
                        &bench.capture,
                        b"__BWK_PASTE_DONE__",
                        Duration::from_secs(20),
                        "paste done",
                    );
                    let elapsed = started.elapsed();
                    finish_pty_bench(bench);
                    elapsed
                };
                scenario_timings
                    .get_mut("paste")
                    .expect("paste scenario")
                    .push(paste);

                for (scenario, producer) in [
                    ("output", "yes X | head -c 16777216"),
                    ("output_block", r#"python3 -c 'import sys; block = b"X\n" * 32768; out = sys.stdout.buffer; [out.write(block) for _ in range(256)]; out.flush()'"#),
                ] {
                    let bench = start_pty_bench_terminal(make_host(), Vec::new());
                    prepare_pty_shell(&bench);
                    write_shell_command(
                        &bench.terminal,
                        &format!("stty -echo opost onlcr; printf '\\n\\137\\137BWK_OUTPUT_ARMED\\137\\137\\n'; read -r go; {producer} && printf '\\033[0m\\n\\137\\137BWK_OUTPUT_DONE\\137\\137\\n'; read -r ack"),
                    );
                    require_output(
                        &bench.capture,
                        b"__BWK_OUTPUT_ARMED__",
                        Duration::from_secs(10),
                        "output armed",
                    );
                    bench.capture.count_output.store(true, Ordering::Release);
                    let before = bench.capture.total.load(Ordering::Relaxed);
                    let started = Instant::now();
                    bench.terminal.write(b"\n").expect("PTY output release");
                    require_output(
                        &bench.capture,
                        b"__BWK_OUTPUT_DONE__",
                        Duration::from_secs(60),
                        "output done",
                    );
                    // The raw hook runs before feed(). Observe the marker in the grid too.
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while !bench.terminal.with_grid(|grid| {
                        (0..grid.size().rows).any(|row| grid.row_text(row).contains("__BWK_OUTPUT_DONE__"))
                    }) {
                        assert!(Instant::now() < deadline, "output marker missing from grid");
                        thread::yield_now();
                    }
                    let elapsed = started.elapsed();
                    assert_eq!(bench.capture.output_x.load(Ordering::Relaxed), 8 * 1024 * 1024);
                    let received = bench.capture.total.load(Ordering::Relaxed) - before;
                    assert!(received >= 24 * 1024 * 1024, "short PTY stream: {received}");
                    scenario_timings.get_mut(scenario).unwrap().push(elapsed);
                    finish_pty_bench(bench);
                }

                let interactive = {
                    let bench = start_pty_bench_terminal(make_host(), Vec::new());
                    prepare_pty_shell(&bench);
                    write_shell_command(
                        &bench.terminal,
                        "stty -icanon min 1 time 0; printf '\\n\\137\\137BWK_INTERACTIVE_ARMED\\137\\137\\n'; dd bs=1 count=1 of=/dev/null status=none; printf '\\n\\137\\137BWK_INTERACTIVE_DONE\\137\\137\\n'",
                    );
                    require_output(
                        &bench.capture,
                        b"__BWK_INTERACTIVE_ARMED__",
                        Duration::from_secs(10),
                        "interactive armed",
                    );
                    bench.capture.armed.store(true, Ordering::Release);
                    let started = Instant::now();
                    bench.terminal.write(b"Q").expect("PTY input write");
                    let echoed = bench
                        .echoed
                        .recv_timeout(Duration::from_secs(10))
                        .expect("PTY input echo");
                    let elapsed = echoed.duration_since(started);
                    require_output(
                        &bench.capture,
                        b"__BWK_INTERACTIVE_DONE__",
                        Duration::from_secs(10),
                        "interactive done",
                    );
                    finish_pty_bench(bench);
                    elapsed
                };
                scenario_timings
                    .get_mut("interactive")
                    .expect("interactive scenario")
                    .push(interactive);
            }

            startup_timings.sort_unstable();
            let startup_median = startup_timings[startup_timings.len() / 2];
            let startup_p95 = startup_timings
                [(startup_timings.len() * 95).div_ceil(100).saturating_sub(1)];
            eprintln!(
                "russh_pty_{route}_startup_ms n={} median={:.3} p95={:.3} samples={:?}",
                startup_timings.len(),
                startup_median.as_secs_f64() * 1000.0,
                startup_p95.as_secs_f64() * 1000.0,
                startup_timings
                    .iter()
                    .map(|sample| sample.as_secs_f64() * 1000.0)
                    .collect::<Vec<_>>()
            );
            for scenario in scenarios {
                let timings = scenario_timings
                    .get_mut(scenario)
                    .expect("PTY scenario timings");
                timings.sort_unstable();
                let median = timings[timings.len() / 2];
                let p95 = timings[(timings.len() * 95).div_ceil(100).saturating_sub(1)];
                eprintln!(
                    "russh_pty_{route}_{scenario}_ms n={} median={:.3} p95={:.3} samples={:?}",
                    timings.len(),
                    median.as_secs_f64() * 1000.0,
                    p95.as_secs_f64() * 1000.0,
                    timings
                        .iter()
                        .map(|sample| sample.as_secs_f64() * 1000.0)
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn parses_telemetry_with_and_without_docker_containers() {
        let plain = "14|4200 16384|61% 122G|0.42 0.38 0.31|14 days, 6 hours";
        let t1 = parse_telemetry(plain, 18).expect("plain telemetry");
        assert_eq!(t1.cpu_pct, 14);
        assert_eq!(t1.mem_used_mb, 4200);
        assert_eq!(t1.mem_total_mb, 16384);
        assert_eq!(t1.disk_pct, 61);
        assert_eq!(t1.disk_used, "122G");
        assert!(t1.containers.is_empty());

        let with_docker = concat!(
            "22|5120 16384|62% 124G|0.55 0.41 0.33|14 days\n",
            "===DOCKER_PS===\n",
            "api-gateway\tghcr.io/portus/api:v2\tUp 2 days\trunning\t0.0.0.0:443->8443/tcp\n",
            "certbot\tcertbot/certbot\tExited (0) 6h ago\texited\t\n",
            "===DOCKER_STATS===\n",
            "api-gateway\t6.40%\t284MiB / 1GiB\t27.73%\n"
        );
        let t2 = parse_telemetry(with_docker, 15).expect("docker telemetry");
        assert_eq!(t2.containers.len(), 2);
        assert_eq!(t2.containers[0].name, "api-gateway");
        assert!(t2.containers[0].running);
        assert!((t2.containers[0].cpu_pct - 6.4).abs() < 0.01);
        assert_eq!(t2.containers[0].mem_used, "284MiB");
        assert!((t2.containers[0].mem_pct - 27.73).abs() < 0.01);
        assert!(!t2.containers[1].running);
        assert_eq!(t2.containers[1].mem_used, "—");
    }

    #[test]
    fn extracts_framed_telemetry_ignoring_echoed_command() {
        let raw = concat!(
            "printf '__BWK_TELE_B__\\n'; probe; printf '__BWK_TELE_E__\\n'\r\n",
            "__BWK_TELE_B__\r\n",
            "18|4096 16384|55% 110G|0.31 0.28 0.25|3 days\r\n",
            "===DOCKER_PS===\r\n",
            "web\tnginx:alpine\tUp 3 days\trunning\t80/tcp\r\n",
            "__BWK_TELE_E__\r\n",
            "root@prod:~# "
        );
        let payload = extract_framed_telemetry(raw).expect("framed payload");
        let tele = parse_telemetry(payload, 22).expect("parsed relay telemetry");
        assert_eq!(tele.cpu_pct, 18);
        assert_eq!(tele.containers.len(), 1);
        assert_eq!(tele.containers[0].name, "web");
    }

    #[test]
    fn builds_safe_docker_logs_command() {
        let cmd = build_docker_logs_cmd("api-gateway", 500, false);
        assert!(cmd.contains("$DK logs --tail 500 -- api-gateway 2>&1"));
        assert!(!cmd.contains("--timestamps"));

        let quoted = build_docker_logs_cmd("my;rm -rf /", 100, true);
        assert!(quoted.contains("$DK logs --tail 100 --timestamps -- 'my;rm -rf /' 2>&1"));
    }

    #[test]
    fn extracts_framed_docker_logs() {
        let raw = concat!(
            "__BWK_LOG_B__\r\n",
            "2026-11-26T05:00:00Z INFO starting\r\n",
            "2026-11-26T05:00:01Z ERROR failed\r\n",
            "__BWK_LOG_E__\r\n",
        );
        let logs = extract_framed_block(raw, DOCKER_LOG_FRAME_BEGIN, DOCKER_LOG_FRAME_END)
            .expect("framed logs");
        assert!(logs.contains("INFO starting"));
        assert!(logs.contains("ERROR failed"));
    }
}
