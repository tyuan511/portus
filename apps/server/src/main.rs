use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{delete, get, post};
use axum::Router;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::{Deserialize, Serialize};
use sshield_core::{
    ApprovalDecision, ApprovalHandler, ApprovalRequest, AuditChainStatus, AuditEvent,
    CommandSnippet, CommandVerdict, L2ProviderConfig, PolicyDecision, RejectAllApprovalHandler,
    SecurityRule, SemanticReviewVerdict, ServerAsset, VaultCredentialKind, VaultCredentialMeta,
    VaultStatus,
};
use sshield_mcp::{SshieldContext, SshieldMcpServer, serve_stdio};
use sshield_security::SecurityEngine;
use sshield_ssh::{SshPool, import_default_ssh_config, import_ssh_config_str};
use sshield_storage::Storage;
use sshield_vault::SealedVault;
use tokio::sync::{Mutex, broadcast, oneshot};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
enum WsEvent {
    ApprovalRequested(ApprovalRequest),
    ApprovalResolved {
        id: String,
        decision: ApprovalDecision,
    },
}

struct PendingApproval {
    request: ApprovalRequest,
    responder: oneshot::Sender<ApprovalDecision>,
}

struct InteractiveApprovalBroker {
    pending: Mutex<HashMap<String, PendingApproval>>,
    events: broadcast::Sender<WsEvent>,
}

impl InteractiveApprovalBroker {
    fn new(events: broadcast::Sender<WsEvent>) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            events,
        }
    }

    async fn list_pending(&self) -> Vec<ApprovalRequest> {
        let guard = self.pending.lock().await;
        guard.values().map(|p| p.request.clone()).collect()
    }

    async fn resolve(&self, id: &str, approved: bool) -> Result<ApprovalDecision, String> {
        let mut guard = self.pending.lock().await;
        let entry = guard
            .remove(id)
            .ok_or_else(|| format!("Approval request `{id}` not found or already resolved"))?;
        let decision = if approved {
            ApprovalDecision::Approved
        } else {
            ApprovalDecision::Rejected
        };
        let _ = entry.responder.send(decision);
        let _ = self.events.send(WsEvent::ApprovalResolved {
            id: id.to_string(),
            decision,
        });
        Ok(decision)
    }
}

#[async_trait]
impl ApprovalHandler for InteractiveApprovalBroker {
    async fn request_approval(&self, req: ApprovalRequest) -> sshield_core::Result<ApprovalDecision> {
        let (tx, rx) = oneshot::channel();
        let id = req.id.clone();
        let timeout = Duration::from_secs(req.timeout_secs.max(5));

        {
            let mut guard = self.pending.lock().await;
            guard.insert(
                id.clone(),
                PendingApproval {
                    request: req.clone(),
                    responder: tx,
                },
            );
        }

        let _ = self.events.send(WsEvent::ApprovalRequested(req));

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(decision)) => Ok(decision),
            Ok(Err(_)) => Ok(ApprovalDecision::Rejected),
            Err(_) => {
                let mut guard = self.pending.lock().await;
                guard.remove(&id);
                let _ = self.events.send(WsEvent::ApprovalResolved {
                    id,
                    decision: ApprovalDecision::TimedOut,
                });
                Ok(ApprovalDecision::TimedOut)
            }
        }
    }
}

#[derive(Clone)]
struct AppState {
    ctx: Arc<SshieldContext>,
    broker: Arc<InteractiveApprovalBroker>,
    ws_tx: broadcast::Sender<WsEvent>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let is_stdio = std::env::args().any(|arg| arg == "--mcp-stdio");

    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,sshield=debug".to_string()),
        )
        .with_writer(std::io::stderr)
        .init();

    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sshield");
    let db_url = std::env::var("SSHIELD_DB_URL")
        .unwrap_or_else(|_| format!("sqlite://{}", data_dir.join("sshield.db").display()));
    let audit_jsonl = std::env::var("SSHIELD_AUDIT_JSONL")
        .map(PathBuf::from)
        .unwrap_or_else(|_| data_dir.join("audit.jsonl"));
    let vault_path = std::env::var("SSHIELD_VAULT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| data_dir.join("vault.enc"));

    let storage = Storage::connect(&db_url, Some(audit_jsonl)).await?;
    storage
        .seed_default_rules_if_empty(&SecurityEngine::default_rules())
        .await?;

    let vault = Arc::new(SealedVault::new(vault_path, 900));
    let ssh_pool = Arc::new(SshPool::with_vault(Arc::clone(&vault)));
    let security = Arc::new(SecurityEngine::new());

    if is_stdio {
        let ctx = Arc::new(SshieldContext::new(
            storage,
            vault,
            ssh_pool,
            security,
            Arc::new(RejectAllApprovalHandler),
            "mcp-stdio",
        ));
        let mcp = SshieldMcpServer::new(ctx);
        tracing::info!("Starting Sshield MCP Server in stdio mode");
        return serve_stdio(mcp).await;
    }

    let (ws_tx, _) = broadcast::channel::<WsEvent>(128);
    let broker = Arc::new(InteractiveApprovalBroker::new(ws_tx.clone()));
    let ctx = Arc::new(SshieldContext::new(
        storage,
        vault,
        ssh_pool,
        security,
        broker.clone(),
        "mcp-http",
    ));

    let mcp_ctx = ctx.clone();
    let mcp_service = StreamableHttpService::new(
        move || Ok(SshieldMcpServer::new(mcp_ctx.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let state = AppState {
        ctx,
        broker,
        ws_tx,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/vault/status", get(vault_status))
        .route("/api/vault/initialize", post(vault_initialize))
        .route("/api/vault/unlock", post(vault_unlock))
        .route("/api/vault/lock", post(vault_lock))
        .route("/api/vault/credentials", get(list_credentials).post(store_credential))
        .route("/api/vault/credentials/{id}", delete(delete_credential))
        .route("/api/servers", get(list_servers).post(upsert_server))
        .route("/api/servers/{id}", delete(delete_server))
        .route("/api/servers/{id}/test", post(test_server))
        .route("/api/servers/import-ssh-config", post(import_ssh_config))
        .route("/api/rules", get(list_rules).post(upsert_rule))
        .route("/api/rules/{id}", delete(delete_rule))
        .route("/api/l2-config", get(get_l2_config).post(upsert_l2_config))
        .route("/api/snippets", get(list_snippets).post(upsert_snippet))
        .route("/api/snippets/{id}", delete(delete_snippet))
        .route("/api/security/evaluate", post(evaluate_command))
        .route("/api/audit-logs", get(list_audits))
        .route("/api/audit-logs/verify", get(verify_audit_chain))
        .route("/api/approvals", get(list_approvals))
        .route("/api/approvals/{id}/decide", post(decide_approval))
        .route("/ws", get(ws_handler))
        .nest_service("/mcp", mcp_service)
        .layer(cors)
        .with_state(state);

    let addr: SocketAddr = std::env::var("SSHIELD_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_string())
        .parse()?;

    tracing::info!(%addr, "Sshield server listening (HTTP API + WebSocket + Streamable HTTP MCP at /mcp)");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "sshield-server",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

#[derive(Deserialize)]
struct PassphraseReq {
    passphrase: String,
}

async fn vault_status(State(state): State<AppState>) -> Result<Json<VaultStatus>, (StatusCode, String)> {
    state.ctx.vault.status().await.map(Json).map_err(internal_err)
}

async fn vault_initialize(
    State(state): State<AppState>,
    Json(req): Json<PassphraseReq>,
) -> Result<Json<VaultStatus>, (StatusCode, String)> {
    state.ctx.vault.initialize(&req.passphrase).await.map_err(bad_req)?;
    state.ctx.vault.status().await.map(Json).map_err(internal_err)
}

async fn vault_unlock(
    State(state): State<AppState>,
    Json(req): Json<PassphraseReq>,
) -> Result<Json<VaultStatus>, (StatusCode, String)> {
    state.ctx.vault.unlock(&req.passphrase).await.map_err(bad_req)?;
    state.ctx.vault.status().await.map(Json).map_err(internal_err)
}

async fn vault_lock(State(state): State<AppState>) -> Result<Json<VaultStatus>, (StatusCode, String)> {
    state.ctx.vault.lock().await;
    state.ctx.vault.status().await.map(Json).map_err(internal_err)
}

async fn list_credentials(
    State(state): State<AppState>,
) -> Result<Json<Vec<VaultCredentialMeta>>, (StatusCode, String)> {
    state.ctx.vault.list_credentials_meta().map(Json).map_err(internal_err)
}

#[derive(Deserialize)]
struct StoreCredentialReq {
    id: Option<String>,
    label: String,
    kind: VaultCredentialKind,
    username: Option<String>,
    secret: String,
}

async fn store_credential(
    State(state): State<AppState>,
    Json(req): Json<StoreCredentialReq>,
) -> Result<Json<VaultCredentialMeta>, (StatusCode, String)> {
    state
        .ctx
        .vault
        .store_secret(req.id, req.label, req.kind, req.username, &req.secret)
        .await
        .map(Json)
        .map_err(bad_req)
}

async fn delete_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let deleted = state.ctx.vault.delete_secret(&id).await.map_err(bad_req)?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

async fn list_servers(State(state): State<AppState>) -> Result<Json<Vec<ServerAsset>>, (StatusCode, String)> {
    state.ctx.storage.list_servers().await.map(Json).map_err(internal_err)
}

async fn upsert_server(
    State(state): State<AppState>,
    Json(mut asset): Json<ServerAsset>,
) -> Result<Json<ServerAsset>, (StatusCode, String)> {
    if asset.id.trim().is_empty() {
        asset.id = format!("srv-{}", &Uuid::new_v4().to_string()[..8]);
    }
    state.ctx.storage.upsert_server(&asset).await.map_err(internal_err)?;
    state.ctx.ssh_pool.invalidate(&asset.id).await;
    Ok(Json(asset))
}

async fn delete_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let deleted = state.ctx.storage.delete_server(&id).await.map_err(internal_err)?;
    state.ctx.ssh_pool.invalidate(&id).await;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

async fn test_server(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let all = state.ctx.storage.list_servers().await.map_err(internal_err)?;
    let map: HashMap<String, ServerAsset> = all.into_iter().map(|s| (s.id.clone(), s)).collect();
    let asset = map
        .get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Server `{id}` not found")))?;

    match state.ctx.ssh_pool.test_connection(asset, &map).await {
        Ok(()) => Ok(Json(serde_json::json!({ "ok": true, "message": "SSH connection & auth succeeded" }))),
        Err(e) => Ok(Json(serde_json::json!({ "ok": false, "message": e.to_string() }))),
    }
}

#[derive(Deserialize)]
struct ImportSshConfigReq {
    content: Option<String>,
}

async fn import_ssh_config(
    State(state): State<AppState>,
    Json(req): Json<ImportSshConfigReq>,
) -> Result<Json<Vec<ServerAsset>>, (StatusCode, String)> {
    let imported = match req.content {
        Some(c) if !c.trim().is_empty() => import_ssh_config_str(&c),
        _ => import_default_ssh_config(),
    }
    .map_err(bad_req)?;

    for asset in &imported {
        state.ctx.storage.upsert_server(asset).await.map_err(internal_err)?;
    }

    Ok(Json(imported))
}

async fn list_rules(State(state): State<AppState>) -> Result<Json<Vec<SecurityRule>>, (StatusCode, String)> {
    state.ctx.storage.list_rules().await.map(Json).map_err(internal_err)
}

async fn upsert_rule(
    State(state): State<AppState>,
    Json(mut rule): Json<SecurityRule>,
) -> Result<Json<SecurityRule>, (StatusCode, String)> {
    if rule.id.trim().is_empty() {
        rule.id = format!("rule-{}", &Uuid::new_v4().to_string()[..8]);
    }
    state.ctx.storage.upsert_rule(&rule).await.map_err(internal_err)?;
    Ok(Json(rule))
}

async fn delete_rule(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let deleted = state.ctx.storage.delete_rule(&id).await.map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

async fn get_l2_config(State(state): State<AppState>) -> Result<Json<L2ProviderConfig>, (StatusCode, String)> {
    state.ctx.storage.get_l2_config().await.map(Json).map_err(internal_err)
}

async fn upsert_l2_config(
    State(state): State<AppState>,
    Json(mut cfg): Json<L2ProviderConfig>,
) -> Result<Json<L2ProviderConfig>, (StatusCode, String)> {
    if cfg.id.trim().is_empty() {
        cfg.id = "default-l2".to_string();
    }
    state.ctx.storage.upsert_l2_config(&cfg).await.map_err(internal_err)?;
    Ok(Json(cfg))
}

async fn list_snippets(State(state): State<AppState>) -> Result<Json<Vec<CommandSnippet>>, (StatusCode, String)> {
    state.ctx.storage.list_snippets().await.map(Json).map_err(internal_err)
}

async fn upsert_snippet(
    State(state): State<AppState>,
    Json(mut snippet): Json<CommandSnippet>,
) -> Result<Json<CommandSnippet>, (StatusCode, String)> {
    if snippet.id.trim().is_empty() {
        snippet.id = format!("snip-{}", &Uuid::new_v4().to_string()[..8]);
    }
    state.ctx.storage.upsert_snippet(&snippet).await.map_err(internal_err)?;
    Ok(Json(snippet))
}

async fn delete_snippet(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let deleted = state.ctx.storage.delete_snippet(&id).await.map_err(internal_err)?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

#[derive(Deserialize)]
struct EvaluateReq {
    command: String,
    intent: Option<String>,
    server_id: Option<String>,
}

#[derive(Serialize)]
struct DualEvaluateResp {
    l1: CommandVerdict,
    l2: Option<SemanticReviewVerdict>,
    final_decision: PolicyDecision,
}

async fn evaluate_command(
    State(state): State<AppState>,
    Json(req): Json<EvaluateReq>,
) -> Result<Json<DualEvaluateResp>, (StatusCode, String)> {
    let dummy_asset = if let Some(ref sid) = req.server_id
        && let Some(found) = state.ctx.storage.get_server(sid).await.map_err(internal_err)?
    {
        found
    } else {
        ServerAsset {
            id: "sandbox".into(),
            name: "sandbox".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "ops".into(),
            auth_kind: Default::default(),
            identity_file: None,
            credential_id: None,
            jump_host_id: None,
            group_name: None,
            tags: vec![],
            description: None,
            enabled: true,
        }
    };

    let (l1, l2, final_decision) = state
        .ctx
        .evaluate_dual_layer(
            &dummy_asset,
            &req.command,
            req.intent.as_deref().unwrap_or("Sandbox evaluation"),
        )
        .await
        .map_err(internal_err)?;

    Ok(Json(DualEvaluateResp {
        l1,
        l2,
        final_decision,
    }))
}

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
    server_id: Option<String>,
}

async fn list_audits(
    State(state): State<AppState>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEvent>>, (StatusCode, String)> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    state
        .ctx
        .storage
        .list_audits(limit, q.server_id.as_deref())
        .await
        .map(Json)
        .map_err(internal_err)
}

async fn verify_audit_chain(
    State(state): State<AppState>,
) -> Result<Json<AuditChainStatus>, (StatusCode, String)> {
    state.ctx.storage.verify_audit_chain().await.map(Json).map_err(internal_err)
}

async fn list_approvals(State(state): State<AppState>) -> Json<Vec<ApprovalRequest>> {
    Json(state.broker.list_pending().await)
}

#[derive(Deserialize)]
struct DecideApprovalReq {
    approved: bool,
}

async fn decide_approval(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<DecideApprovalReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let decision = state
        .broker
        .resolve(&id, req.approved)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    Ok(Json(serde_json::json!({ "id": id, "decision": decision })))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut rx = state.ws_tx.subscribe();
    for req in state.broker.list_pending().await {
        if let Ok(json) = serde_json::to_string(&WsEvent::ApprovalRequested(req))
            && socket.send(Message::Text(json.into())).await.is_err()
        {
            return;
        }
    }

    while let Ok(event) = rx.recv().await {
        if let Ok(json) = serde_json::to_string(&event)
            && socket.send(Message::Text(json.into())).await.is_err()
        {
            break;
        }
    }
}

fn internal_err(err: sshield_core::SshieldError) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn bad_req(err: sshield_core::SshieldError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}
