use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::Sender;
use rand::Rng as _;
use russh::{ChannelMsg, Disconnect, client};
use russh_sftp::client::SftpSession;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, oneshot};

use crate::settings::{
    HostProfile, HostProtocol, Language, McpGuardAction, McpGuardRule, McpHostGrant, McpSettings,
    McpTransport, Settings, SettingsStore,
};
use crate::ssh::{ClientHandler, SshUiEvent, connect_target, runtime as ssh_runtime};

const MAX_READ_BYTES: usize = 256 * 1024;
const APPROVAL_TIMEOUT_SECS: u64 = 180;
const MAX_AUDIT_SESSIONS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpToolRisk {
    Low,
    Medium,
    High,
}

impl McpToolRisk {
    pub fn label(self, language: Language) -> &'static str {
        match self {
            Self::Low => language.text("Low risk", "低风险"),
            Self::Medium => language.text("Medium risk", "中风险"),
            Self::High => language.text("High risk", "高风险"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct McpToolSpec {
    pub id: &'static str,
    pub risk: McpToolRisk,
    pub desc_en: &'static str,
    pub desc_zh: &'static str,
}

impl McpToolSpec {
    pub fn desc(self, language: Language) -> &'static str {
        language.text(self.desc_en, self.desc_zh)
    }
}

pub const MCP_TOOLS: [McpToolSpec; 9] = [
    McpToolSpec {
        id: "list_hosts",
        risk: McpToolRisk::Low,
        desc_en: "List authorized hosts and group metadata (no plaintext credentials)",
        desc_zh: "列出已授权的主机与分组元数据（不含明文凭证）",
    },
    McpToolSpec {
        id: "open_host_session",
        risk: McpToolRisk::Low,
        desc_en: "Issue a TTL-scoped controlled SSH session according to host grant",
        desc_zh: "按主机授权签发带 TTL 的受控 SSH 会话",
    },
    McpToolSpec {
        id: "execute_command",
        risk: McpToolRisk::High,
        desc_en: "Execute a shell command in an active session (guarded by rules & approval)",
        desc_zh: "在活跃会话中执行 Shell 命令（受高危规则与审批约束）",
    },
    McpToolSpec {
        id: "close_host_session",
        risk: McpToolRisk::Low,
        desc_en: "Close and reclaim an issued MCP host session",
        desc_zh: "主动结束并回收已签发的 MCP 主机会话",
    },
    McpToolSpec {
        id: "sftp_list",
        risk: McpToolRisk::Low,
        desc_en: "Browse remote directory entries and file permissions",
        desc_zh: "浏览远程目录结构与文件权限信息",
    },
    McpToolSpec {
        id: "sftp_read",
        risk: McpToolRisk::Medium,
        desc_en: "Read remote config files and logs (secrets auto-redacted)",
        desc_zh: "读取远程配置文件与日志（自动脱敏密钥）",
    },
    McpToolSpec {
        id: "sftp_write",
        risk: McpToolRisk::High,
        desc_en: "Write or modify files on a remote host (approval required by default)",
        desc_zh: "向远程主机写入或修改文件（默认需审批）",
    },
    McpToolSpec {
        id: "run_snippet",
        risk: McpToolRisk::Medium,
        desc_en: "Run a predefined snippet from the Portus Snippets library",
        desc_zh: "执行 Portus 命令片段库中预定义的安全命令",
    },
    McpToolSpec {
        id: "list_snippets",
        risk: McpToolRisk::Low,
        desc_en: "Read the available snippets, including their commands and groups",
        desc_zh: "读取可用命令片段及其命令内容与分组",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditVerdict {
    Allow,
    Approved,
    Denied,
    Pending,
}

impl AuditVerdict {
    pub fn label(self, language: Language) -> &'static str {
        match self {
            Self::Allow => language.text("Allowed", "已放行"),
            Self::Approved => language.text("Approved", "人工批准"),
            Self::Denied => language.text("Denied", "含拦截"),
            Self::Pending => language.text("Pending", "待审批"),
        }
    }

    pub fn step_label(self, language: Language) -> &'static str {
        match self {
            Self::Allow => language.text("Allowed", "已放行"),
            Self::Approved => language.text("Approved", "人工批准"),
            Self::Denied => language.text("Blocked", "已拦截"),
            Self::Pending => language.text("Pending", "待审批"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct McpAuditEvent {
    pub ts: String,
    pub tool: String,
    pub summary: String,
    pub verdict: AuditVerdict,
    pub output: String,
}

#[derive(Clone, Debug)]
pub struct McpAuditSession {
    pub id: String,
    pub client: String,
    pub host: String,
    pub started: String,
    pub active: bool,
    pub verdict: AuditVerdict,
    pub events: Vec<McpAuditEvent>,
    pub audit_hash: String,
}

#[derive(Clone, Debug)]
pub struct McpPendingApprovalView {
    pub id: u64,
    pub client: String,
    pub host: String,
    pub session_id: String,
    #[allow(dead_code)]
    pub tool: String,
    pub cmd: String,
    pub reason: String,
    pub time: String,
}

#[derive(Clone, Debug)]
pub struct McpActiveSessionView {
    pub id: String,
    pub client: String,
    pub host: String,
    pub grant: McpHostGrant,
    pub ttl_left_secs: u64,
    pub cmds: u32,
    #[allow(dead_code)]
    pub started: String,
}

#[derive(Clone, Debug)]
pub struct McpSnapshot {
    pub running: bool,
    pub bind_error: Option<String>,
    #[allow(dead_code)]
    pub listening_port: u16,
    pub pending_approvals: Vec<McpPendingApprovalView>,
    pub active_sessions: Vec<McpActiveSessionView>,
    pub audit_sessions: Vec<McpAuditSession>,
}

#[derive(Clone, Debug)]
pub enum McpUiEvent {
    Changed,
    ApprovalRequested {
        client: String,
        host: String,
        cmd: String,
    },
    ServerError(String),
}

struct PendingApprovalEntry {
    view: McpPendingApprovalView,
    responder: Option<oneshot::Sender<bool>>,
}

struct ActiveSessionEntry {
    id: String,
    client: String,
    host_id: u64,
    host_name: String,
    grant: McpHostGrant,
    expires_at: Instant,
    cmds: u32,
    started: String,
    ssh: Arc<client::Handle<ClientHandler>>,
}

struct McpInner {
    settings: Settings,
    running: bool,
    bind_error: Option<String>,
    listening_port: u16,
    server_stop: Option<Arc<Notify>>,
    next_approval_id: u64,
    pending_approvals: Vec<PendingApprovalEntry>,
    active_sessions: HashMap<String, ActiveSessionEntry>,
    audit_sessions: Vec<McpAuditSession>,
    client_names: HashMap<String, String>,
}

pub struct McpRuntime {
    inner: Mutex<McpInner>,
    events: Option<Sender<McpUiEvent>>,
}

impl McpRuntime {
    pub fn new(settings: Settings, events: Option<Sender<McpUiEvent>>) -> Arc<Self> {
        let port = settings.mcp.port;
        let should_start = settings.mcp.enabled && settings.mcp.transport == McpTransport::Http;
        let runtime = Arc::new(Self {
            inner: Mutex::new(McpInner {
                settings,
                running: false,
                bind_error: None,
                listening_port: port,
                server_stop: None,
                next_approval_id: 1,
                pending_approvals: Vec::new(),
                active_sessions: HashMap::new(),
                audit_sessions: Vec::new(),
                client_names: HashMap::new(),
            }),
            events,
        });
        if should_start {
            runtime.start_http_server();
        }
        runtime
    }

    pub fn sync_settings(self: &Arc<Self>, settings: Settings) {
        let (want_http, want_port, was_running, old_port) = {
            let mut inner = self.inner.lock().unwrap();
            let was_running = inner.running;
            let old_port = inner.listening_port;
            let want_http = settings.mcp.enabled && settings.mcp.transport == McpTransport::Http;
            let want_port = settings.mcp.port;
            inner.settings = settings;
            (want_http, want_port, was_running, old_port)
        };

        if !want_http {
            if was_running {
                self.stop_http_server();
            } else {
                self.emit(McpUiEvent::Changed);
            }
        } else if !was_running || old_port != want_port {
            self.stop_http_server();
            self.start_http_server();
        } else {
            self.emit(McpUiEvent::Changed);
        }
    }

    pub fn start_http_server(self: &Arc<Self>) {
        let (port, stop) = {
            let mut inner = self.inner.lock().unwrap();
            if let Some(old) = inner.server_stop.take() {
                old.notify_waiters();
            }
            let stop = Arc::new(Notify::new());
            let port = inner.settings.mcp.port;
            inner.listening_port = port;
            inner.bind_error = None;
            inner.running = true;
            inner.server_stop = Some(Arc::clone(&stop));
            (port, stop)
        };
        self.emit(McpUiEvent::Changed);

        let rt = Arc::clone(self);
        ssh_runtime().spawn(async move {
            let addr = format!("127.0.0.1:{port}");
            let listener = match TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(err) => {
                    let msg = format!("Failed to bind {addr}: {err}");
                    {
                        let mut inner = rt.inner.lock().unwrap();
                        inner.running = false;
                        inner.bind_error = Some(msg.clone());
                        inner.server_stop = None;
                    }
                    rt.emit(McpUiEvent::ServerError(msg));
                    rt.emit(McpUiEvent::Changed);
                    return;
                }
            };

            let mut sweep = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = stop.notified() => {
                        break;
                    }
                    _ = sweep.tick() => {
                        rt.reap_expired_sessions();
                    }
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _)) => {
                                let rt_conn = Arc::clone(&rt);
                                tokio::spawn(async move {
                                    handle_http_connection(rt_conn, stream).await;
                                });
                            }
                            Err(_) => {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                        }
                    }
                }
            }
        });
    }

    pub fn stop_http_server(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(stop) = inner.server_stop.take() {
            stop.notify_waiters();
        }
        inner.running = false;
        inner.bind_error = None;
        drop(inner);
        self.emit(McpUiEvent::Changed);
    }

    pub fn snapshot(&self) -> McpSnapshot {
        self.reap_expired_sessions();
        let inner = self.inner.lock().unwrap();
        let now = Instant::now();
        let mut active_sessions: Vec<McpActiveSessionView> = inner
            .active_sessions
            .values()
            .map(|s| McpActiveSessionView {
                id: s.id.clone(),
                client: s.client.clone(),
                host: s.host_name.clone(),
                grant: s.grant,
                ttl_left_secs: s.expires_at.saturating_duration_since(now).as_secs(),
                cmds: s.cmds,
                started: s.started.clone(),
            })
            .collect();
        active_sessions.sort_by(|a, b| a.id.cmp(&b.id));

        let pending_approvals = inner
            .pending_approvals
            .iter()
            .map(|p| p.view.clone())
            .collect();

        McpSnapshot {
            running: inner.running
                || (inner.settings.mcp.enabled
                    && inner.settings.mcp.transport == McpTransport::Stdio),
            bind_error: inner.bind_error.clone(),
            listening_port: inner.listening_port,
            pending_approvals,
            active_sessions,
            audit_sessions: inner.audit_sessions.clone(),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.inner.lock().unwrap().pending_approvals.len()
    }

    pub fn simulate_approval(&self, language: Language) {
        let (client, host, cmd, reason, session_id) = {
            let mut inner = self.inner.lock().unwrap();
            let host_name = inner
                .settings
                .hosts
                .first()
                .map(|h| h.name.clone())
                .unwrap_or_else(|| "prod-api-01".to_owned());
            let samples = [
                (
                    "Claude Code",
                    "sudo nginx -t && sudo systemctl reload nginx",
                    language.text(
                        "Matched rule: Service stop/restart/reload",
                        "命中规则：服务启停与重载",
                    ),
                ),
                (
                    "Antigravity",
                    "docker compose -f /var/www/api/docker-compose.yml restart api-gateway",
                    language.text(
                        "Matched rule: Service stop/restart/reload",
                        "命中规则：服务启停与重载",
                    ),
                ),
                (
                    "Cursor",
                    "chmod 777 /var/log/portus",
                    language.text(
                        "Matched rule: Broad permission change",
                        "命中规则：大范围权限变更",
                    ),
                ),
            ];
            let idx = (inner.next_approval_id as usize) % samples.len();
            let (client, cmd, reason) = samples[idx];
            let id = inner.next_approval_id;
            inner.next_approval_id += 1;
            let session_id = gen_session_id();
            let now_ts = format_hms_now();

            inner.pending_approvals.insert(
                0,
                PendingApprovalEntry {
                    view: McpPendingApprovalView {
                        id,
                        client: client.to_owned(),
                        host: host_name.clone(),
                        session_id: session_id.clone(),
                        tool: "execute_command".to_owned(),
                        cmd: cmd.to_owned(),
                        reason: reason.to_owned(),
                        time: now_ts.clone(),
                    },
                    responder: None,
                },
            );

            push_audit_event_locked(
                &mut inner.audit_sessions,
                &session_id,
                client,
                &host_name,
                true,
                McpAuditEvent {
                    ts: now_ts,
                    tool: "execute_command".to_owned(),
                    summary: cmd.to_owned(),
                    verdict: AuditVerdict::Pending,
                    output: format!("等待人工审批：{reason}"),
                },
            );

            (
                client.to_owned(),
                host_name,
                cmd.to_owned(),
                reason.to_owned(),
                session_id,
            )
        };

        let _ = (reason, session_id);
        self.emit(McpUiEvent::ApprovalRequested { client, host, cmd });
        self.emit(McpUiEvent::Changed);
    }

    pub fn resolve_approval(&self, id: u64, approved: bool) -> Option<McpPendingApprovalView> {
        let entry = {
            let mut inner = self.inner.lock().unwrap();
            let pos = inner.pending_approvals.iter().position(|p| p.view.id == id)?;
            let entry = inner.pending_approvals.remove(pos);

            // If this was a simulated request (no live responder), update audit directly.
            if entry.responder.is_none() {
                let verdict = if approved {
                    AuditVerdict::Approved
                } else {
                    AuditVerdict::Denied
                };
                let output = if approved {
                    "✓ 已由用户人工批准并执行完成 (exit 0)".to_owned()
                } else {
                    "✗ 已被用户拒绝执行".to_owned()
                };
                resolve_pending_audit_locked(
                    &mut inner.audit_sessions,
                    &entry.view.session_id,
                    verdict,
                    output,
                );
            }
            entry
        };

        if let Some(tx) = entry.responder {
            let _ = tx.send(approved);
        }
        self.emit(McpUiEvent::Changed);
        Some(entry.view)
    }

    pub fn kill_session(&self, session_id: &str) -> bool {
        let removed = {
            let mut inner = self.inner.lock().unwrap();
            let removed = inner.active_sessions.remove(session_id);
            if let Some(sess) = &removed {
                push_audit_event_locked(
                    &mut inner.audit_sessions,
                    session_id,
                    &sess.client,
                    &sess.host_name,
                    false,
                    McpAuditEvent {
                        ts: format_hms_now(),
                        tool: "close_host_session".to_owned(),
                        summary: "管理员在控制台强制断开会话".to_owned(),
                        verdict: AuditVerdict::Allow,
                        output: "session terminated by user · audit hash verified".to_owned(),
                    },
                );
            }
            removed
        };

        if let Some(sess) = removed {
            ssh_runtime().spawn(async move {
                let _ = sess
                    .ssh
                    .disconnect(Disconnect::ByApplication, "", "en")
                    .await;
            });
            self.emit(McpUiEvent::Changed);
            true
        } else {
            false
        }
    }

    fn reap_expired_sessions(&self) {
        let now = Instant::now();
        let expired: Vec<ActiveSessionEntry> = {
            let mut inner = self.inner.lock().unwrap();
            let expired_ids: Vec<String> = inner
                .active_sessions
                .iter()
                .filter(|(_, s)| s.expires_at <= now)
                .map(|(k, _)| k.clone())
                .collect();
            if expired_ids.is_empty() {
                return;
            }
            let mut removed = Vec::with_capacity(expired_ids.len());
            for id in expired_ids {
                if let Some(sess) = inner.active_sessions.remove(&id) {
                    push_audit_event_locked(
                        &mut inner.audit_sessions,
                        &sess.id,
                        &sess.client,
                        &sess.host_name,
                        false,
                        McpAuditEvent {
                            ts: format_hms_now(),
                            tool: "close_host_session".to_owned(),
                            summary: "会话 TTL 到期自动回收".to_owned(),
                            verdict: AuditVerdict::Allow,
                            output: "session expired and reclaimed".to_owned(),
                        },
                    );
                    removed.push(sess);
                }
            }
            removed
        };

        for sess in expired {
            ssh_runtime().spawn(async move {
                let _ = sess
                    .ssh
                    .disconnect(Disconnect::ByApplication, "", "en")
                    .await;
            });
        }
        self.emit(McpUiEvent::Changed);
    }

    fn emit(&self, event: McpUiEvent) {
        if let Some(tx) = &self.events {
            let _ = tx.try_send(event);
        }
    }

    pub async fn handle_jsonrpc(
        self: &Arc<Self>,
        payload: Value,
        http_session_id: Option<&str>,
        header_client: Option<&str>,
    ) -> Option<Value> {
        let method = payload.get("method").and_then(Value::as_str).unwrap_or("");
        let id = payload.get("id").cloned();
        let params = payload.get("params").cloned().unwrap_or(json!({}));

        // Notifications have no `id` and expect no JSON-RPC response body.
        if id.is_none() {
            return None;
        }
        let id = id.unwrap();

        match method {
            "initialize" => {
                let client_name = params
                    .get("clientInfo")
                    .and_then(|c| c.get("name"))
                    .and_then(Value::as_str)
                    .or(header_client)
                    .unwrap_or("MCP Client")
                    .to_owned();
                if let Some(sid) = http_session_id {
                    self.inner
                        .lock()
                        .unwrap()
                        .client_names
                        .insert(sid.to_owned(), client_name);
                }
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": {
                            "tools": { "listChanged": false }
                        },
                        "serverInfo": {
                            "name": "portus-mcp",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }
                }))
            }
            "ping" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            })),
            "tools/list" => {
                let mcp_cfg = self.inner.lock().unwrap().settings.mcp.clone();
                let tools: Vec<Value> = MCP_TOOLS
                    .iter()
                    .filter(|t| mcp_cfg.is_tool_enabled(t.id))
                    .map(|t| tool_json_schema(t.id))
                    .collect();
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "tools": tools }
                }))
            }
            "tools/call" => {
                let tool_name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));

                let client_name = {
                    let inner = self.inner.lock().unwrap();
                    http_session_id
                        .and_then(|sid| inner.client_names.get(sid).cloned())
                        .or_else(|| header_client.map(str::to_owned))
                        .unwrap_or_else(|| "MCP Client".to_owned())
                };

                let result = self
                    .call_tool(tool_name, arguments, &client_name)
                    .await;
                match result {
                    Ok(text) => Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": text }],
                            "isError": false
                        }
                    })),
                    Err(err) => Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": err }],
                            "isError": true
                        }
                    })),
                }
            }
            _ => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {method}")
                }
            })),
        }
    }

    async fn call_tool(
        self: &Arc<Self>,
        tool_name: &str,
        args: Value,
        client_name: &str,
    ) -> Result<String, String> {
        let settings = self.inner.lock().unwrap().settings.clone();
        if !settings.mcp.is_tool_enabled(tool_name) {
            return Err(format!("Tool '{tool_name}' is disabled in Portus MCP settings."));
        }

        match tool_name {
            "list_hosts" => self.tool_list_hosts(&settings),
            "list_snippets" => self.tool_list_snippets(&settings, args),
            "open_host_session" => self.tool_open_host_session(&settings, args, client_name).await,
            "close_host_session" => self.tool_close_host_session(args).await,
            "execute_command" => self.tool_execute_command(&settings, args, client_name).await,
            "run_snippet" => self.tool_run_snippet(&settings, args, client_name).await,
            "sftp_list" => self.tool_sftp_list(&settings, args).await,
            "sftp_read" => self.tool_sftp_read(&settings, args).await,
            "sftp_write" => self.tool_sftp_write(&settings, args, client_name).await,
            _ => Err(format!("Unknown tool: {tool_name}")),
        }
    }

    fn tool_list_hosts(&self, settings: &Settings) -> Result<String, String> {
        let hosts: Vec<Value> = settings
            .hosts
            .iter()
            .filter(|h| h.protocol == HostProtocol::Ssh)
            .filter_map(|h| {
                let grant = settings.mcp.host_grant(h.id);
                if grant == McpHostGrant::Deny {
                    return None;
                }
                Some(json!({
                    "id": h.id,
                    "name": h.name,
                    "group": h.group,
                    "endpoint": h.endpoint(),
                    "tags": h.tags,
                    "grant": grant.code(),
                }))
            })
            .collect();
        serde_json::to_string_pretty(&json!({ "hosts": hosts })).map_err(|e| e.to_string())
    }

    fn tool_list_snippets(&self, settings: &Settings, args: Value) -> Result<String, String> {
        let group = args
            .get("group")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|group| !group.is_empty());
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_lowercase);

        let snippets: Vec<Value> = settings
            .snippets
            .iter()
            .filter(|snippet| group.is_none_or(|wanted| snippet.group.eq_ignore_ascii_case(wanted)))
            .filter(|snippet| {
                query.as_ref().is_none_or(|query| {
                    snippet.name.to_lowercase().contains(query)
                        || snippet.command.to_lowercase().contains(query)
                })
            })
            .map(|snippet| {
                let command = if settings.mcp.mask_secrets {
                    mask_secrets_text(&snippet.command)
                } else {
                    snippet.command.clone()
                };
                json!({
                    "id": snippet.id,
                    "name": snippet.name,
                    "group": snippet.group,
                    "command": command,
                    "add_newline": snippet.add_newline,
                })
            })
            .collect();

        serde_json::to_string_pretty(&json!({ "snippets": snippets }))
            .map_err(|error| error.to_string())
    }

    async fn tool_open_host_session(
        self: &Arc<Self>,
        settings: &Settings,
        args: Value,
        client_name: &str,
    ) -> Result<String, String> {
        let host = resolve_host_arg(settings, &args)?;
        if host.protocol != HostProtocol::Ssh {
            return Err(format!(
                "Host '{}' is not an SSH host and cannot be opened via MCP.",
                host.name
            ));
        }
        let grant = settings.mcp.host_grant(host.id);
        if grant == McpHostGrant::Deny {
            return Err(format!(
                "Access denied: host '{}' is set to 'deny' in Portus MCP settings.",
                host.name
            ));
        }

        let resolved = settings.resolve_host_auth(&host);
        let (ssh_tx, ssh_rx) = async_channel::unbounded::<SshUiEvent>();
        // Auto-decline interactive prompts in headless MCP connections unless host key is already known.
        tokio::spawn(async move {
            while let Ok(ev) = ssh_rx.recv().await {
                if let SshUiEvent::Prompt(p) = ev {
                    let _ = p.reply.send(None);
                }
            }
        });

        let handle = connect_target(&resolved, &settings.hosts, &ssh_tx)
            .await
            .map_err(|e| format!("Failed to connect to '{}': {e}", host.name))?;

        let session_id = gen_session_id();
        let ttl_mins = settings.mcp.session_ttl_mins.max(5);
        let started = format_hm_now();
        let ts = format_hms_now();

        {
            let mut inner = self.inner.lock().unwrap();
            inner.active_sessions.insert(
                session_id.clone(),
                ActiveSessionEntry {
                    id: session_id.clone(),
                    client: client_name.to_owned(),
                    host_id: host.id,
                    host_name: host.name.clone(),
                    grant,
                    expires_at: Instant::now() + Duration::from_secs(u64::from(ttl_mins) * 60),
                    cmds: 0,
                    started: started.clone(),
                    ssh: Arc::new(handle),
                },
            );
            push_audit_event_locked(
                &mut inner.audit_sessions,
                &session_id,
                client_name,
                &host.name,
                true,
                McpAuditEvent {
                    ts,
                    tool: "open_host_session".to_owned(),
                    summary: format!("签发会话 (TTL {ttl_mins}m)"),
                    verdict: AuditVerdict::Allow,
                    output: format!(
                        "session_id={session_id} host={} grant={}",
                        host.name,
                        grant.code()
                    ),
                },
            );
        }
        self.emit(McpUiEvent::Changed);

        serde_json::to_string_pretty(&json!({
            "session_id": session_id,
            "host": host.name,
            "grant": grant.code(),
            "ttl_minutes": ttl_mins,
        }))
        .map_err(|e| e.to_string())
    }

    async fn tool_close_host_session(self: &Arc<Self>, args: Value) -> Result<String, String> {
        let session_id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument: session_id".to_owned())?;

        let removed = {
            let mut inner = self.inner.lock().unwrap();
            let removed = inner.active_sessions.remove(session_id);
            if let Some(sess) = &removed {
                push_audit_event_locked(
                    &mut inner.audit_sessions,
                    session_id,
                    &sess.client,
                    &sess.host_name,
                    false,
                    McpAuditEvent {
                        ts: format_hms_now(),
                        tool: "close_host_session".to_owned(),
                        summary: "关闭并回收会话".to_owned(),
                        verdict: AuditVerdict::Allow,
                        output: "session closed · audit hash verified".to_owned(),
                    },
                );
            }
            removed
        };

        let Some(sess) = removed else {
            return Err(format!("Session not found: {session_id}"));
        };

        let _ = sess
            .ssh
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;
        self.emit(McpUiEvent::Changed);
        Ok(format!("Session {session_id} closed."))
    }

    async fn tool_execute_command(
        self: &Arc<Self>,
        settings: &Settings,
        args: Value,
        client_name: &str,
    ) -> Result<String, String> {
        let session_id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument: session_id".to_owned())?;
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Missing required argument: command".to_owned())?;

        self.exec_guarded_command(
            settings,
            session_id,
            "execute_command",
            command,
            client_name,
        )
        .await
    }

    async fn tool_run_snippet(
        self: &Arc<Self>,
        settings: &Settings,
        args: Value,
        client_name: &str,
    ) -> Result<String, String> {
        let session_id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument: session_id".to_owned())?;
        let snippet_id = args.get("snippet_id").and_then(Value::as_u64);
        let snippet_name = args.get("name").and_then(Value::as_str);

        let snippet = settings
            .snippets
            .iter()
            .find(|s| {
                snippet_id == Some(s.id)
                    || snippet_name.is_some_and(|n| s.name.eq_ignore_ascii_case(n))
            })
            .ok_or_else(|| "Snippet not found in Portus Snippets library.".to_owned())?;

        self.exec_guarded_command(
            settings,
            session_id,
            "run_snippet",
            snippet.command.trim(),
            client_name,
        )
        .await
    }

    async fn exec_guarded_command(
        self: &Arc<Self>,
        settings: &Settings,
        session_id: &str,
        tool_name: &str,
        command: &str,
        client_name: &str,
    ) -> Result<String, String> {
        let (ssh, host_name, grant) = self.lookup_active_session(session_id)?;

        // 1. Check guard rules first
        if let Some(rule) = find_matching_guard_rule(&settings.mcp.guard_rules, command) {
            match rule.action {
                McpGuardAction::Deny => {
                    let msg = format!("BLOCKED: 命中高危规则「{}」({})", rule.label, rule.pattern);
                    self.record_audit(
                        session_id,
                        client_name,
                        &host_name,
                        tool_name,
                        command,
                        AuditVerdict::Denied,
                        &msg,
                    );
                    return Err(msg);
                }
                McpGuardAction::Approval => {
                    let reason = format!("命中规则：{}", rule.label);
                    let approved = self
                        .request_approval(client_name, &host_name, session_id, tool_name, command, &reason)
                        .await;
                    if !approved {
                        return Err(format!("Execution denied by user ({reason})."));
                    }
                    return self
                        .run_ssh_exec_and_audit(
                            settings,
                            session_id,
                            &ssh,
                            client_name,
                            &host_name,
                            tool_name,
                            command,
                            AuditVerdict::Approved,
                        )
                        .await;
                }
            }
        }

        // 2. Check host grant level
        match grant {
            McpHostGrant::Deny => {
                let msg = format!("BLOCKED: 主机 '{host_name}' 已设为禁止访问");
                self.record_audit(
                    session_id,
                    client_name,
                    &host_name,
                    tool_name,
                    command,
                    AuditVerdict::Denied,
                    &msg,
                );
                return Err(msg);
            }
            McpHostGrant::Readonly => {
                if !is_readonly_command(command) {
                    let msg = format!(
                        "BLOCKED: 主机 '{host_name}' 仅授予只读权限，拒绝执行非只读命令"
                    );
                    self.record_audit(
                        session_id,
                        client_name,
                        &host_name,
                        tool_name,
                        command,
                        AuditVerdict::Denied,
                        &msg,
                    );
                    return Err(msg);
                }
            }
            McpHostGrant::Approval => {
                let reason = format!("{host_name} 设为「需审批」");
                let approved = self
                    .request_approval(client_name, &host_name, session_id, tool_name, command, &reason)
                    .await;
                if !approved {
                    return Err(format!("Execution denied by user ({reason})."));
                }
                return self
                    .run_ssh_exec_and_audit(
                        settings,
                        session_id,
                        &ssh,
                        client_name,
                        &host_name,
                        tool_name,
                        command,
                        AuditVerdict::Approved,
                    )
                    .await;
            }
            McpHostGrant::Allow => {}
        }

        self.run_ssh_exec_and_audit(
            settings,
            session_id,
            &ssh,
            client_name,
            &host_name,
            tool_name,
            command,
            AuditVerdict::Allow,
        )
        .await
    }

    async fn run_ssh_exec_and_audit(
        &self,
        settings: &Settings,
        session_id: &str,
        ssh: &Arc<client::Handle<ClientHandler>>,
        client_name: &str,
        host_name: &str,
        tool_name: &str,
        command: &str,
        verdict: AuditVerdict,
    ) -> Result<String, String> {
        let mut ch = ssh
            .channel_open_session()
            .await
            .map_err(|e| format!("Failed to open SSH exec channel: {e}"))?;
        ch.exec(true, command)
            .await
            .map_err(|e| format!("Failed to exec command: {e}"))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code: Option<u32> = None;

        while let Some(msg) = ch.wait().await {
            match msg {
                ChannelMsg::Data { data } => {
                    if stdout.len() < MAX_READ_BYTES {
                        stdout.extend_from_slice(&data);
                    }
                }
                ChannelMsg::ExtendedData { data, .. } => {
                    if stderr.len() < MAX_READ_BYTES {
                        stderr.extend_from_slice(&data);
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => {
                    exit_code = Some(exit_status);
                }
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            }
        }

        let mut combined = String::from_utf8_lossy(&stdout).into_owned();
        if !stderr.is_empty() {
            if !combined.is_empty() && !combined.ends_with('\n') {
                combined.push('\n');
            }
            combined.push_str(&String::from_utf8_lossy(&stderr));
        }
        if settings.mcp.mask_secrets {
            combined = mask_secrets_text(&combined);
        }

        let exit_str = exit_code.map_or_else(|| "unknown".into(), |c| c.to_string());
        let audit_output = if combined.trim().is_empty() {
            format!("(no output, exit {exit_str})")
        } else {
            truncate_chars(&combined, 2000)
        };

        {
            let mut inner = self.inner.lock().unwrap();
            if let Some(sess) = inner.active_sessions.get_mut(session_id) {
                sess.cmds += 1;
            }
            if verdict == AuditVerdict::Approved {
                resolve_pending_audit_locked(
                    &mut inner.audit_sessions,
                    session_id,
                    verdict,
                    audit_output,
                );
            } else {
                push_audit_event_locked(
                    &mut inner.audit_sessions,
                    session_id,
                    client_name,
                    host_name,
                    true,
                    McpAuditEvent {
                        ts: format_hms_now(),
                        tool: tool_name.to_owned(),
                        summary: command.to_owned(),
                        verdict,
                        output: audit_output,
                    },
                );
            }
        }
        self.emit(McpUiEvent::Changed);

        serde_json::to_string_pretty(&json!({
            "exit_code": exit_code,
            "output": combined,
        }))
        .map_err(|e| e.to_string())
    }

    async fn tool_sftp_list(
        &self,
        settings: &Settings,
        args: Value,
    ) -> Result<String, String> {
        let session_id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument: session_id".to_owned())?;
        let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let (ssh, host_name, _) = self.lookup_active_session(session_id)?;
        let client_name = self.session_client(session_id);

        let sftp = open_sftp_session(&ssh).await?;
        let dir = sftp
            .read_dir(path)
            .await
            .map_err(|e| format!("sftp_list failed: {e}"))?;
        let entries: Vec<Value> = dir
            .filter_map(|e| {
                let name = e.file_name();
                if name == "." || name == ".." {
                    return None;
                }
                let meta = e.metadata();
                Some(json!({
                    "name": name,
                    "is_dir": meta.is_dir(),
                    "size": meta.size.unwrap_or(0),
                    "permissions": meta.permissions,
                }))
            })
            .collect();

        let _ = settings;
        let out = serde_json::to_string_pretty(&json!({ "path": path, "entries": entries }))
            .map_err(|e| e.to_string())?;
        self.record_audit(
            session_id,
            &client_name,
            &host_name,
            "sftp_list",
            path,
            AuditVerdict::Allow,
            &format!("listed {} entries", entries.len()),
        );
        Ok(out)
    }

    async fn tool_sftp_read(
        &self,
        settings: &Settings,
        args: Value,
    ) -> Result<String, String> {
        let session_id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument: session_id".to_owned())?;
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument: path".to_owned())?;
        let (ssh, host_name, _) = self.lookup_active_session(session_id)?;
        let client_name = self.session_client(session_id);

        let sftp = open_sftp_session(&ssh).await?;
        let mut file = sftp
            .open(path)
            .await
            .map_err(|e| format!("sftp_read failed: {e}"))?;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        while buf.len() < MAX_READ_BYTES {
            let n = file
                .read(&mut chunk)
                .await
                .map_err(|e| format!("sftp_read failed: {e}"))?;
            if n == 0 {
                break;
            }
            let take = (MAX_READ_BYTES - buf.len()).min(n);
            buf.extend_from_slice(&chunk[..take]);
        }

        let mut text = String::from_utf8_lossy(&buf).into_owned();
        if settings.mcp.mask_secrets {
            text = mask_secrets_text(&text);
        }
        self.record_audit(
            session_id,
            &client_name,
            &host_name,
            "sftp_read",
            path,
            AuditVerdict::Allow,
            &truncate_chars(&text, 1000),
        );
        Ok(text)
    }

    async fn tool_sftp_write(
        self: &Arc<Self>,
        _settings: &Settings,
        args: Value,
        client_name: &str,
    ) -> Result<String, String> {
        let session_id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument: session_id".to_owned())?;
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument: path".to_owned())?;
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument: content".to_owned())?;
        let (ssh, host_name, grant) = self.lookup_active_session(session_id)?;

        if matches!(grant, McpHostGrant::Deny | McpHostGrant::Readonly) {
            let msg = format!("BLOCKED: 主机 '{host_name}' 不允许 SFTP 写入操作");
            self.record_audit(
                session_id,
                client_name,
                &host_name,
                "sftp_write",
                path,
                AuditVerdict::Denied,
                &msg,
            );
            return Err(msg);
        }

        let verdict = if grant == McpHostGrant::Approval {
            let summary = format!("write {} ({} bytes)", path, content.len());
            let reason = format!("sftp_write 写入远程文件（{host_name}）");
            let approved = self
                .request_approval(
                    client_name,
                    &host_name,
                    session_id,
                    "sftp_write",
                    &summary,
                    &reason,
                )
                .await;
            if !approved {
                return Err("sftp_write denied by user.".to_owned());
            }
            AuditVerdict::Approved
        } else {
            AuditVerdict::Allow
        };

        let sftp = open_sftp_session(&ssh).await?;
        let mut file = sftp
            .create(path)
            .await
            .map_err(|e| format!("sftp_write failed: {e}"))?;
        file.write_all(content.as_bytes())
            .await
            .map_err(|e| format!("sftp_write failed: {e}"))?;
        file.flush()
            .await
            .map_err(|e| format!("sftp_write failed: {e}"))?;

        let msg = format!("Wrote {} bytes to {path}", content.len());
        if verdict == AuditVerdict::Approved {
            let mut inner = self.inner.lock().unwrap();
            resolve_pending_audit_locked(&mut inner.audit_sessions, session_id, verdict, msg.clone());
            drop(inner);
            self.emit(McpUiEvent::Changed);
        } else {
            self.record_audit(
                session_id,
                client_name,
                &host_name,
                "sftp_write",
                path,
                verdict,
                &msg,
            );
        }
        Ok(msg)
    }

    fn lookup_active_session(
        &self,
        session_id: &str,
    ) -> Result<(Arc<client::Handle<ClientHandler>>, String, McpHostGrant), String> {
        self.reap_expired_sessions();
        let inner = self.inner.lock().unwrap();
        let sess = inner
            .active_sessions
            .get(session_id)
            .ok_or_else(|| format!("Invalid or expired session_id: {session_id}"))?;
        // Re-check latest host grant in case user lowered permissions dynamically.
        let latest_grant = inner.settings.mcp.host_grant(sess.host_id);
        Ok((Arc::clone(&sess.ssh), sess.host_name.clone(), latest_grant))
    }

    fn session_client(&self, session_id: &str) -> String {
        self.inner
            .lock()
            .unwrap()
            .active_sessions
            .get(session_id)
            .map(|s| s.client.clone())
            .unwrap_or_else(|| "MCP Client".to_owned())
    }

    async fn request_approval(
        &self,
        client_name: &str,
        host_name: &str,
        session_id: &str,
        tool_name: &str,
        cmd: &str,
        reason: &str,
    ) -> bool {
        // Headless stdio mode has no GUI approver -> deny safely.
        if self.events.is_none() {
            self.record_audit(
                session_id,
                client_name,
                host_name,
                tool_name,
                cmd,
                AuditVerdict::Denied,
                "BLOCKED: Stdio headless 模式无法执行人工审批，已自动拒绝",
            );
            return false;
        }

        let (tx, rx) = oneshot::channel::<bool>();
        let id = {
            let mut inner = self.inner.lock().unwrap();
            let id = inner.next_approval_id;
            inner.next_approval_id += 1;
            let now_ts = format_hms_now();
            inner.pending_approvals.insert(
                0,
                PendingApprovalEntry {
                    view: McpPendingApprovalView {
                        id,
                        client: client_name.to_owned(),
                        host: host_name.to_owned(),
                        session_id: session_id.to_owned(),
                        tool: tool_name.to_owned(),
                        cmd: cmd.to_owned(),
                        reason: reason.to_owned(),
                        time: now_ts.clone(),
                    },
                    responder: Some(tx),
                },
            );
            push_audit_event_locked(
                &mut inner.audit_sessions,
                session_id,
                client_name,
                host_name,
                true,
                McpAuditEvent {
                    ts: now_ts,
                    tool: tool_name.to_owned(),
                    summary: cmd.to_owned(),
                    verdict: AuditVerdict::Pending,
                    output: format!("等待人工审批：{reason}"),
                },
            );
            id
        };

        self.emit(McpUiEvent::ApprovalRequested {
            client: client_name.to_owned(),
            host: host_name.to_owned(),
            cmd: cmd.to_owned(),
        });
        self.emit(McpUiEvent::Changed);

        let approved = match tokio::time::timeout(Duration::from_secs(APPROVAL_TIMEOUT_SECS), rx).await {
            Ok(Ok(true)) => true,
            _ => {
                let mut inner = self.inner.lock().unwrap();
                inner.pending_approvals.retain(|p| p.view.id != id);
                resolve_pending_audit_locked(
                    &mut inner.audit_sessions,
                    session_id,
                    AuditVerdict::Denied,
                    "✗ 已被用户拒绝或审批超时".to_owned(),
                );
                false
            }
        };
        self.emit(McpUiEvent::Changed);
        approved
    }

    fn record_audit(
        &self,
        session_id: &str,
        client_name: &str,
        host_name: &str,
        tool_name: &str,
        summary: &str,
        verdict: AuditVerdict,
        output: &str,
    ) {
        {
            let mut inner = self.inner.lock().unwrap();
            push_audit_event_locked(
                &mut inner.audit_sessions,
                session_id,
                client_name,
                host_name,
                true,
                McpAuditEvent {
                    ts: format_hms_now(),
                    tool: tool_name.to_owned(),
                    summary: summary.to_owned(),
                    verdict,
                    output: output.to_owned(),
                },
            );
        }
        self.emit(McpUiEvent::Changed);
    }
}

async fn open_sftp_session(ssh: &Arc<client::Handle<ClientHandler>>) -> Result<SftpSession, String> {
    let channel = ssh
        .channel_open_session()
        .await
        .map_err(|e| format!("Failed to open SFTP channel: {e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("Failed to request SFTP subsystem: {e}"))?;
    SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("Failed to initialize SFTP session: {e}"))
}

fn resolve_host_arg(settings: &Settings, args: &Value) -> Result<HostProfile, String> {
    if let Some(id) = args.get("host_id").and_then(Value::as_u64) {
        return settings
            .hosts
            .iter()
            .find(|h| h.id == id)
            .cloned()
            .ok_or_else(|| format!("Host id {id} not found."));
    }
    if let Some(name) = args.get("host").and_then(Value::as_str) {
        return settings
            .hosts
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name) || h.address.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| format!("Host '{name}' not found."));
    }
    Err("Missing required argument: host or host_id".to_owned())
}

fn push_audit_event_locked(
    sessions: &mut Vec<McpAuditSession>,
    session_id: &str,
    client: &str,
    host: &str,
    active: bool,
    event: McpAuditEvent,
) {
    if let Some(sess) = sessions.iter_mut().find(|s| s.id == session_id) {
        sess.active = active;
        if matches!(event.verdict, AuditVerdict::Denied | AuditVerdict::Pending)
            || (event.verdict == AuditVerdict::Approved && sess.verdict != AuditVerdict::Denied)
        {
            sess.verdict = event.verdict;
        }
        sess.audit_hash = chain_hash(&sess.audit_hash, &event.tool, &event.summary);
        sess.events.push(event);
    } else {
        let hash = chain_hash("genesis", &event.tool, &event.summary);
        sessions.insert(
            0,
            McpAuditSession {
                id: session_id.to_owned(),
                client: client.to_owned(),
                host: host.to_owned(),
                started: format_hm_now(),
                active,
                verdict: event.verdict,
                events: vec![event],
                audit_hash: hash,
            },
        );
        if sessions.len() > MAX_AUDIT_SESSIONS {
            sessions.truncate(MAX_AUDIT_SESSIONS);
        }
    }
}

fn resolve_pending_audit_locked(
    sessions: &mut [McpAuditSession],
    session_id: &str,
    verdict: AuditVerdict,
    output: String,
) {
    if let Some(sess) = sessions.iter_mut().find(|s| s.id == session_id) {
        if let Some(ev) = sess
            .events
            .iter_mut()
            .rev()
            .find(|e| e.verdict == AuditVerdict::Pending)
        {
            ev.verdict = verdict;
            ev.output = output;
            sess.audit_hash = chain_hash(&sess.audit_hash, &ev.tool, &ev.summary);
        }
        sess.verdict = verdict;
    }
}

fn chain_hash(prev: &str, tool: &str, summary: &str) -> String {
    // Lightweight FNV-1a 64-bit tamper-evident chain digest
    let mut h: u64 = 0xcbf29ce484222325;
    for part in [prev.as_bytes(), b"|", tool.as_bytes(), b"|", summary.as_bytes()] {
        for &b in part {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

fn gen_session_id() -> String {
    let mut bytes = [0u8; 3];
    rand::rng().fill_bytes(&mut bytes);
    format!("sess_{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2])
}

fn format_hms_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn format_hm_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    format!("{h:02}:{m:02}")
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("\n… [truncated]");
        out
    }
}

pub fn find_matching_guard_rule<'a>(
    rules: &'a [McpGuardRule],
    command: &str,
) -> Option<&'a McpGuardRule> {
    rules
        .iter()
        .filter(|r| r.enabled && !r.pattern.trim().is_empty())
        .find(|r| match_guard_pattern(&r.pattern, command))
}

/// Matches simple pipe-separated guard patterns including inline `(a|b|c)` groups, case-insensitively.
pub fn match_guard_pattern(pattern: &str, command: &str) -> bool {
    let cmd_lower = command.to_lowercase();
    for branch in split_top_level_pipes(pattern) {
        let branch = branch.trim();
        if branch.is_empty() {
            continue;
        }
        for expanded in expand_single_group(branch) {
            let needle = expanded.trim().to_lowercase();
            if !needle.is_empty() && cmd_lower.contains(&needle) {
                return true;
            }
        }
    }
    false
}

fn split_top_level_pipes(pattern: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in pattern.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                parts.push(&pattern[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(&pattern[start..]);
    parts
}

fn expand_single_group(branch: &str) -> Vec<String> {
    if let Some(open) = branch.find('(')
        && let Some(rel_close) = branch[open + 1..].find(')')
    {
        let close = open + 1 + rel_close;
        let prefix = &branch[..open];
        let inner = &branch[open + 1..close];
        let suffix = &branch[close + 1..];
        return inner
            .split('|')
            .map(|opt| format!("{prefix}{opt}{suffix}"))
            .collect();
    }
    vec![branch.to_owned()]
}

pub fn is_readonly_command(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Disallow output redirection or chaining write-prone operators
    if trimmed.contains('>') || trimmed.contains("&&") || trimmed.contains(';') {
        return false;
    }
    let first = trimmed.split_whitespace().next().unwrap_or("").to_lowercase();
    matches!(
        first.as_str(),
        "ls" | "ll"
            | "pwd"
            | "whoami"
            | "id"
            | "uname"
            | "uptime"
            | "date"
            | "df"
            | "du"
            | "free"
            | "ps"
            | "top"
            | "cat"
            | "head"
            | "tail"
            | "grep"
            | "rg"
            | "find"
            | "stat"
            | "wc"
            | "env"
            | "journalctl"
            | "ss"
            | "netstat"
            | "ip"
            | "ifconfig"
            | "dig"
            | "nslookup"
    ) || trimmed.to_lowercase().starts_with("systemctl status ")
        || trimmed.to_lowercase().starts_with("docker ps")
        || trimmed.to_lowercase().starts_with("docker logs")
}

pub fn mask_secrets_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_pem = false;

    for line in input.lines() {
        if line.contains("BEGIN") && line.contains("PRIVATE KEY") {
            in_pem = true;
            out.push_str("[REDACTED: PRIVATE KEY]\n");
            continue;
        }
        if in_pem {
            if line.contains("END") && line.contains("PRIVATE KEY") {
                in_pem = false;
            }
            continue;
        }
        out.push_str(&mask_line_secrets(line));
        out.push('\n');
    }
    if !input.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn mask_line_secrets(line: &str) -> String {
    let mut result = line.to_owned();

    // Redact Bearer tokens
    if let Some(pos) = result.to_lowercase().find("bearer ") {
        let start = pos + "bearer ".len();
        let end = result[start..]
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',')
            .map_or(result.len(), |i| start + i);
        if end > start + 4 {
            result.replace_range(start..end, "***");
        }
    }

    // Redact AWS access keys (AKIA...)
    if let Some(pos) = result.find("AKIA")
        && result.len() >= pos + 20
        && result[pos..pos + 20]
            .chars()
            .all(|c| c.is_ascii_alphanumeric())
    {
        result.replace_range(pos..pos + 20, "[REDACTED:AWS_KEY]");
    }

    // Redact key=value / key: value secrets
    let lower = result.to_lowercase();
    for key in [
        "password=",
        "passwd=",
        "secret=",
        "api_key=",
        "apikey=",
        "access_token=",
        "x-internal-token=",
        "token=",
    ] {
        if let Some(pos) = lower.find(key) {
            let val_start = pos + key.len();
            let val_end = result[val_start..]
                .find(|c: char| c.is_whitespace() || c == '&' || c == ';' || c == '"' || c == '\'')
                .map_or(result.len(), |i| val_start + i);
            if val_end > val_start {
                result.replace_range(val_start..val_end, "***");
                break;
            }
        }
    }

    // Redact connection string passwords: scheme://user:pass@host
    if let Some(scheme_pos) = result.find("://") {
        let after_scheme = scheme_pos + 3;
        if let Some(at_rel) = result[after_scheme..].find('@') {
            let auth = &result[after_scheme..after_scheme + at_rel];
            if let Some(colon_rel) = auth.find(':') {
                let pass_start = after_scheme + colon_rel + 1;
                let pass_end = after_scheme + at_rel;
                if pass_end > pass_start {
                    result.replace_range(pass_start..pass_end, "***");
                }
            }
        }
    }

    result
}

fn tool_json_schema(id: &str) -> Value {
    match id {
        "list_hosts" => json!({
            "name": "list_hosts",
            "description": "List authorized SSH hosts and group metadata (no plaintext credentials).",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        "list_snippets" => json!({
            "name": "list_snippets",
            "description": "Read the available Portus command snippets and their commands.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": { "type": "string", "description": "Optional group name filter" },
                    "query": { "type": "string", "description": "Optional name or command search" }
                },
                "additionalProperties": false
            }
        }),
        "open_host_session" => json!({
            "name": "open_host_session",
            "description": "Open a TTL-scoped controlled SSH session on an authorized host.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host_id": { "type": "integer", "description": "Host ID from list_hosts" },
                    "host": { "type": "string", "description": "Host name or address" }
                }
            }
        }),
        "execute_command" => json!({
            "name": "execute_command",
            "description": "Execute a shell command in an active MCP SSH session (subject to guard rules and approval).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "command": { "type": "string" }
                },
                "required": ["session_id", "command"]
            }
        }),
        "close_host_session" => json!({
            "name": "close_host_session",
            "description": "Close and reclaim an active MCP SSH session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }
        }),
        "sftp_list" => json!({
            "name": "sftp_list",
            "description": "List files and permissions in a remote directory via SFTP.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "path": { "type": "string", "default": "." }
                },
                "required": ["session_id"]
            }
        }),
        "sftp_read" => json!({
            "name": "sftp_read",
            "description": "Read a remote text file via SFTP (secrets automatically redacted).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["session_id", "path"]
            }
        }),
        "sftp_write" => json!({
            "name": "sftp_write",
            "description": "Write content to a remote file via SFTP.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["session_id", "path", "content"]
            }
        }),
        "run_snippet" => json!({
            "name": "run_snippet",
            "description": "Execute a predefined command snippet from Portus's Snippets library.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "snippet_id": { "type": "integer" },
                    "name": { "type": "string" }
                },
                "required": ["session_id"]
            }
        }),
        _ => json!({ "name": id, "inputSchema": { "type": "object" } }),
    }
}

async fn handle_http_connection(rt: Arc<McpRuntime>, mut stream: TcpStream) {
    let mut reader = BufReader::new(&mut stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
        return;
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0];
    let path = parts[1];

    let mut content_length = 0usize;
    let mut mcp_session_id: Option<String> = None;
    let mut user_agent: Option<String> = None;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let k_lower = k.trim().to_ascii_lowercase();
            let v = v.trim();
            match k_lower.as_str() {
                "content-length" => {
                    content_length = v.parse().unwrap_or(0);
                }
                "mcp-session-id" => {
                    mcp_session_id = Some(v.to_owned());
                }
                "user-agent" => {
                    user_agent = Some(v.to_owned());
                }
                _ => {}
            }
        }
    }

    let mut body = vec![0u8; content_length.min(4 * 1024 * 1024)];
    if !body.is_empty() && reader.read_exact(&mut body).await.is_err() {
        return;
    }

    if method == "OPTIONS" {
        let _ = write_http_response(&mut stream, 204, "No Content", None, &[]).await;
        return;
    }

    if !path.starts_with("/mcp") {
        let _ = write_http_response(
            &mut stream,
            404,
            "Not Found",
            None,
            br#"{"error":"Not found. Use /mcp"}"#,
        )
        .await;
        return;
    }

    if method == "GET" {
        let info = json!({
            "name": "portus-mcp",
            "status": "running",
            "transport": "streamable-http",
        });
        let bytes = serde_json::to_vec(&info).unwrap_or_default();
        let _ = write_http_response(&mut stream, 200, "OK", None, &bytes).await;
        return;
    }

    if method == "DELETE" {
        let _ = write_http_response(&mut stream, 204, "No Content", None, &[]).await;
        return;
    }

    if method != "POST" {
        let _ = write_http_response(&mut stream, 405, "Method Not Allowed", None, &[]).await;
        return;
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": format!("Parse error: {err}") }
            });
            let bytes = serde_json::to_vec(&resp).unwrap_or_default();
            let _ = write_http_response(&mut stream, 400, "Bad Request", None, &bytes).await;
            return;
        }
    };

    let sid = mcp_session_id.unwrap_or_else(gen_session_id);
    match rt
        .handle_jsonrpc(payload, Some(&sid), user_agent.as_deref())
        .await
    {
        Some(resp) => {
            let bytes = serde_json::to_vec(&resp).unwrap_or_default();
            let _ = write_http_response(&mut stream, 200, "OK", Some(&sid), &bytes).await;
        }
        None => {
            let _ = write_http_response(&mut stream, 202, "Accepted", Some(&sid), &[]).await;
        }
    }
}

async fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    session_id: Option<&str>,
    body: &[u8],
) -> std::io::Result<()> {
    let mut header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Accept, Mcp-Session-Id\r\n\
         Connection: close\r\n",
        body.len()
    );
    if let Some(sid) = session_id {
        header.push_str(&format!("Mcp-Session-Id: {sid}\r\n"));
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes()).await?;
    if !body.is_empty() {
        stream.write_all(body).await?;
    }
    stream.flush().await
}

pub fn run_stdio_server() {
    use std::io::{BufRead as _, Write as _};

    let store = SettingsStore::open();
    let mut settings = store.current;
    settings.mcp.enabled = true;
    settings.mcp.transport = McpTransport::Stdio;
    let runtime = McpRuntime::new(settings, None);

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let rt = ssh_runtime();

    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(payload) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if let Some(resp) = rt.block_on(runtime.handle_jsonrpc(
            payload,
            Some("stdio"),
            Some("Stdio Client"),
        )) {
            if let Ok(mut out) = serde_json::to_string(&resp) {
                out.push('\n');
                let _ = stdout.write_all(out.as_bytes());
                let _ = stdout.flush();
            }
        }
    }
}

fn stdio_executable() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "/Applications/Portus.app/Contents/MacOS/portus".to_owned()
            } else {
                env!("CARGO_PKG_NAME").to_owned()
            }
        })
}

pub fn build_client_config(mcp: &McpSettings, client_id: &str) -> String {
    if mcp.transport == McpTransport::Stdio {
        let executable = stdio_executable();
        return match client_id {
            "claude" => format!(
                "claude mcp add portus -- {} --mcp-stdio",
                shell_words::quote(&executable)
            ),
            "codex" => {
                let command = json!(executable);
                format!("[mcp_servers.portus]\ncommand = {command}\nargs = [\"--mcp-stdio\"]")
            }
            "opencode" => serde_json::to_string_pretty(&json!({
                "mcp": {
                    "portus": {
                        "type": "local",
                        "command": [executable, "--mcp-stdio"],
                        "enabled": true
                    }
                }
            }))
            .unwrap_or_default(),
            _ => serde_json::to_string_pretty(&json!({
                "mcpServers": {
                    "portus": {
                        "command": executable,
                        "args": ["--mcp-stdio"]
                    }
                }
            }))
            .unwrap_or_default(),
        };
    }

    let url = format!("http://127.0.0.1:{}/mcp", mcp.port);
    match client_id {
        "claude" => format!("claude mcp add --transport http portus {url}"),
        "codex" => format!("[mcp_servers.portus]\nurl = \"{url}\""),
        "opencode" => serde_json::to_string_pretty(&json!({
            "mcp": {
                "portus": {
                    "type": "remote",
                    "url": url,
                    "enabled": true
                }
            }
        }))
        .unwrap_or_default(),
        _ => serde_json::to_string_pretty(&json!({
            "mcpServers": {
                "portus": {
                    "url": url
                }
            }
        }))
        .unwrap_or_default(),
    }
}

pub fn build_connect_prompt(mcp: &McpSettings, prompt_lang: Language) -> String {
    let url = format!("http://127.0.0.1:{}/mcp", mcp.port);
    let enabled_tools = MCP_TOOLS
        .iter()
        .filter(|t| mcp.is_tool_enabled(t.id))
        .map(|t| t.id)
        .collect::<Vec<_>>()
        .join(", ");

    let executable = stdio_executable();
    let exe_cmd = shell_words::quote(&executable);

    match prompt_lang {
        Language::English => {
            let conn_line = if mcp.transport == McpTransport::Http {
                format!(
                    "- Transport: Streamable HTTP\n- URL: {url} (localhost-only, no API key / token required)"
                )
            } else {
                format!("- Transport: stdio\n- Command: {exe_cmd} --mcp-stdio")
            };
            format!(
                "Please configure and connect to my local Portus MCP Server so you can manage authorized SSH hosts and SFTP files:\n\n{conn_line}\n- Available tools: {enabled_tools}\n\nSteps:\n1. Add this \"portus\" server to your MCP configuration and reload tools.\n2. Call `list_hosts` to verify connectivity and inspect authorized hosts.\n3. Always open a session with `open_host_session` before calling `execute_command`, and call `close_host_session` when finished."
            )
        }
        Language::ChineseSimplified => {
            let conn_line = if mcp.transport == McpTransport::Http {
                format!(
                    "- 传输协议：Streamable HTTP\n- 服务地址：{url}（仅监听本机回环地址，无需密钥）"
                )
            } else {
                format!("- 传输协议：Stdio\n- 启动命令：{exe_cmd} --mcp-stdio")
            };
            format!(
                "请帮我接入本地 Portus MCP Server，用于安全受控地访问已授权的 SSH 主机与 SFTP 文件：\n\n{conn_line}\n- 已启用工具：{enabled_tools}\n\n接入与使用要求：\n1. 将名为 \"portus\" 的 MCP 服务写入你的客户端配置并刷新工具列表。\n2. 调用 `list_hosts` 验证连通性并查看当前可访问的主机。\n3. 执行命令前须先通过 `open_host_session` 创建受控会话，任务完成后调用 `close_host_session` 关闭会话。"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{HostIcon, default_mcp_guard_rules};

    #[test]
    fn guard_pattern_matches_groups_and_pipes() {
        let rules = default_mcp_guard_rules();
        assert_eq!(
            find_matching_guard_rule(&rules, "sudo rm -rf /tmp/foo").map(|r| r.action),
            Some(McpGuardAction::Deny)
        );
        assert_eq!(
            find_matching_guard_rule(&rules, "sudo systemctl reload nginx").map(|r| r.action),
            Some(McpGuardAction::Approval)
        );
        assert_eq!(
            find_matching_guard_rule(&rules, "psql -c 'DROP TABLE users;'").map(|r| r.action),
            Some(McpGuardAction::Deny)
        );
        assert!(find_matching_guard_rule(&rules, "systemctl status nginx").is_none());
    }

    #[test]
    fn secret_masking_redacts_tokens_keys_and_urls() {
        let raw = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9\n\
                   x-internal-token=supersecret123\n\
                   postgres://admin:p4ssw0rd@db.internal:5432/app\n\
                   -----BEGIN OPENSSH PRIVATE KEY-----\n\
                   b3BlbnNzaC1rZXktdjE=\n\
                   -----END OPENSSH PRIVATE KEY-----";
        let masked = mask_secrets_text(raw);
        assert!(!masked.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(!masked.contains("supersecret123"));
        assert!(!masked.contains("p4ssw0rd"));
        assert!(!masked.contains("b3BlbnNzaC1rZXktdjE="));
        assert!(masked.contains("[REDACTED: PRIVATE KEY]"));
    }

    #[test]
    fn jsonrpc_initialize_tools_and_list_hosts() {
        let mut settings = Settings::default();
        settings.mcp.enabled = false; // avoid binding port in unit test
        settings.hosts.push(HostProfile {
            id: 1,
            name: "prod-api-01".into(),
            protocol: HostProtocol::Ssh,
            icon: HostIcon::Server,
            address: "10.0.0.1".into(),
            username: "deploy".into(),
            port: 22,
            baud_rate: 115200,
            keychain_id: None,
            identity_file: String::new(),
            password: "secret".into(),
            jump_host: String::new(),
            jump_host_id: None,
            jump_relay_command: String::new(),
            jump_relay_keychain_id: None,
            jump_relay_password: String::new(),
            group: "Production".into(),
            tags: vec!["api".into()],
            last_connected: None,
        });
        settings.mcp.host_grants.insert(1, McpHostGrant::Approval);

        let rt = McpRuntime::new(settings, None);
        ssh_runtime().block_on(async {
            let init = rt
                .handle_jsonrpc(
                    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Claude Code"}}}),
                    Some("s1"),
                    None,
                )
                .await
                .unwrap();
            assert_eq!(init["result"]["serverInfo"]["name"], "portus-mcp");

            let tools = rt
                .handle_jsonrpc(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), Some("s1"), None)
                .await
                .unwrap();
            let list = tools["result"]["tools"].as_array().unwrap();
            assert_eq!(list.len(), 8); // sftp_write disabled by default

            let snippets = rt
                .handle_jsonrpc(
                    json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_snippets","arguments":{}}}),
                    Some("s1"),
                    None,
                )
                .await
                .unwrap();
            let snippets_text = snippets["result"]["content"][0]["text"]
                .as_str()
                .unwrap();
            assert!(snippets_text.contains("Docker resource usage"));
            assert!(snippets_text.contains("docker stats"));

            let hosts = rt
                .handle_jsonrpc(
                    json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_hosts","arguments":{}}}),
                    Some("s1"),
                    None,
                )
                .await
                .unwrap();
            let text = hosts["result"]["content"][0]["text"].as_str().unwrap();
            assert!(text.contains("prod-api-01"));
            assert!(!text.contains("secret"));
        });
    }
}
