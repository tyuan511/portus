#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use sshield_core::{
    ApprovalDecision, ApprovalHandler, ApprovalRequest, AuditChainStatus, AuditEvent,
    CommandSnippet, CommandVerdict, L2ProviderConfig, PolicyDecision, SecurityRule,
    SemanticReviewVerdict, ServerAsset, VaultCredentialKind, VaultCredentialMeta, VaultStatus,
};
use sshield_mcp::SshieldContext;
use sshield_security::SecurityEngine;
use sshield_ssh::{SshPool, import_default_ssh_config, import_ssh_config_str};
use sshield_storage::Storage;
use sshield_vault::SealedVault;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, oneshot};
use uuid::Uuid;

struct PendingApproval {
    request: ApprovalRequest,
    responder: oneshot::Sender<ApprovalDecision>,
}

struct TauriApprovalBroker {
    app: AppHandle,
    pending: Mutex<HashMap<String, PendingApproval>>,
}

impl TauriApprovalBroker {
    fn new(app: AppHandle) -> Self {
        Self {
            app,
            pending: Mutex::new(HashMap::new()),
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
            .ok_or_else(|| format!("Approval request `{id}` not found"))?;
        let decision = if approved {
            ApprovalDecision::Approved
        } else {
            ApprovalDecision::Rejected
        };
        let _ = entry.responder.send(decision);
        let _ = self.app.emit(
            "approval-resolved",
            serde_json::json!({ "id": id, "decision": decision }),
        );
        Ok(decision)
    }
}

#[async_trait]
impl ApprovalHandler for TauriApprovalBroker {
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

        let _ = self.app.emit("approval-requested", &req);
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = win.set_focus();
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(decision)) => Ok(decision),
            Ok(Err(_)) => Ok(ApprovalDecision::Rejected),
            Err(_) => {
                let mut guard = self.pending.lock().await;
                guard.remove(&id);
                let _ = self.app.emit(
                    "approval-resolved",
                    serde_json::json!({ "id": id, "decision": ApprovalDecision::TimedOut }),
                );
                Ok(ApprovalDecision::TimedOut)
            }
        }
    }
}

struct DesktopState {
    ctx: Arc<SshieldContext>,
    broker: Arc<TauriApprovalBroker>,
}

#[derive(Serialize)]
struct TestResult {
    ok: bool,
    message: String,
}

#[derive(Serialize)]
struct DecideResult {
    id: String,
    decision: ApprovalDecision,
}

#[derive(Serialize)]
struct DualEvaluateResp {
    l1: CommandVerdict,
    l2: Option<SemanticReviewVerdict>,
    final_decision: PolicyDecision,
}

#[tauri::command]
async fn vault_status(state: State<'_, DesktopState>) -> Result<VaultStatus, String> {
    state.ctx.vault.status().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn vault_initialize(
    state: State<'_, DesktopState>,
    passphrase: String,
) -> Result<VaultStatus, String> {
    state
        .ctx
        .vault
        .initialize(&passphrase)
        .await
        .map_err(|e| e.to_string())?;
    state.ctx.vault.status().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn vault_unlock(
    state: State<'_, DesktopState>,
    passphrase: String,
) -> Result<VaultStatus, String> {
    state
        .ctx
        .vault
        .unlock(&passphrase)
        .await
        .map_err(|e| e.to_string())?;
    state.ctx.vault.status().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn vault_lock(state: State<'_, DesktopState>) -> Result<VaultStatus, String> {
    state.ctx.vault.lock().await;
    state.ctx.vault.status().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_credentials(state: State<'_, DesktopState>) -> Result<Vec<VaultCredentialMeta>, String> {
    state
        .ctx
        .vault
        .list_credentials_meta()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn store_credential(
    state: State<'_, DesktopState>,
    id: Option<String>,
    label: String,
    kind: VaultCredentialKind,
    username: Option<String>,
    secret: String,
) -> Result<VaultCredentialMeta, String> {
    state
        .ctx
        .vault
        .store_secret(id, label, kind, username, &secret)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_credential(state: State<'_, DesktopState>, id: String) -> Result<bool, String> {
    state
        .ctx
        .vault
        .delete_secret(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_servers(state: State<'_, DesktopState>) -> Result<Vec<ServerAsset>, String> {
    state.ctx.storage.list_servers().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn upsert_server(
    state: State<'_, DesktopState>,
    mut asset: ServerAsset,
) -> Result<ServerAsset, String> {
    if asset.id.trim().is_empty() {
        asset.id = format!("srv-{}", &Uuid::new_v4().to_string()[..8]);
    }
    state
        .ctx
        .storage
        .upsert_server(&asset)
        .await
        .map_err(|e| e.to_string())?;
    state.ctx.ssh_pool.invalidate(&asset.id).await;
    Ok(asset)
}

#[tauri::command]
async fn delete_server(state: State<'_, DesktopState>, id: String) -> Result<(), String> {
    state
        .ctx
        .storage
        .delete_server(&id)
        .await
        .map_err(|e| e.to_string())?;
    state.ctx.ssh_pool.invalidate(&id).await;
    Ok(())
}

#[tauri::command]
async fn test_server(state: State<'_, DesktopState>, id: String) -> Result<TestResult, String> {
    let all = state.ctx.storage.list_servers().await.map_err(|e| e.to_string())?;
    let map: HashMap<String, ServerAsset> = all.into_iter().map(|s| (s.id.clone(), s)).collect();
    let asset = map.get(&id).ok_or_else(|| format!("Server `{id}` not found"))?;
    match state.ctx.ssh_pool.test_connection(asset, &map).await {
        Ok(()) => Ok(TestResult {
            ok: true,
            message: "SSH connection & auth succeeded".to_string(),
        }),
        Err(e) => Ok(TestResult {
            ok: false,
            message: e.to_string(),
        }),
    }
}

#[tauri::command]
async fn import_ssh_config(
    state: State<'_, DesktopState>,
    content: Option<String>,
) -> Result<Vec<ServerAsset>, String> {
    let imported = match content {
        Some(c) if !c.trim().is_empty() => import_ssh_config_str(&c),
        _ => import_default_ssh_config(),
    }
    .map_err(|e| e.to_string())?;

    for asset in &imported {
        state
            .ctx
            .storage
            .upsert_server(asset)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(imported)
}

#[tauri::command]
async fn list_rules(state: State<'_, DesktopState>) -> Result<Vec<SecurityRule>, String> {
    state.ctx.storage.list_rules().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn upsert_rule(
    state: State<'_, DesktopState>,
    mut rule: SecurityRule,
) -> Result<SecurityRule, String> {
    if rule.id.trim().is_empty() {
        rule.id = format!("rule-{}", &Uuid::new_v4().to_string()[..8]);
    }
    state
        .ctx
        .storage
        .upsert_rule(&rule)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rule)
}

#[tauri::command]
async fn delete_rule(state: State<'_, DesktopState>, id: String) -> Result<(), String> {
    state
        .ctx
        .storage
        .delete_rule(&id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn get_l2_config(state: State<'_, DesktopState>) -> Result<L2ProviderConfig, String> {
    state
        .ctx
        .storage
        .get_l2_config()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn upsert_l2_config(
    state: State<'_, DesktopState>,
    mut config: L2ProviderConfig,
) -> Result<L2ProviderConfig, String> {
    if config.id.trim().is_empty() {
        config.id = "default-l2".to_string();
    }
    state
        .ctx
        .storage
        .upsert_l2_config(&config)
        .await
        .map_err(|e| e.to_string())?;
    Ok(config)
}

#[tauri::command]
async fn list_snippets(state: State<'_, DesktopState>) -> Result<Vec<CommandSnippet>, String> {
    state
        .ctx
        .storage
        .list_snippets()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn upsert_snippet(
    state: State<'_, DesktopState>,
    mut snippet: CommandSnippet,
) -> Result<CommandSnippet, String> {
    if snippet.id.trim().is_empty() {
        snippet.id = format!("snip-{}", &Uuid::new_v4().to_string()[..8]);
    }
    state
        .ctx
        .storage
        .upsert_snippet(&snippet)
        .await
        .map_err(|e| e.to_string())?;
    Ok(snippet)
}

#[tauri::command]
async fn delete_snippet(state: State<'_, DesktopState>, id: String) -> Result<(), String> {
    state
        .ctx
        .storage
        .delete_snippet(&id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn evaluate_command(
    state: State<'_, DesktopState>,
    command: String,
    intent: Option<String>,
    server_id: Option<String>,
) -> Result<DualEvaluateResp, String> {
    let asset = if let Some(ref sid) = server_id
        && let Some(found) = state.ctx.storage.get_server(sid).await.map_err(|e| e.to_string())?
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
            &asset,
            &command,
            intent.as_deref().unwrap_or("Sandbox evaluation"),
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(DualEvaluateResp {
        l1,
        l2,
        final_decision,
    })
}

#[tauri::command]
async fn list_audits(
    state: State<'_, DesktopState>,
    limit: Option<i64>,
    server_id: Option<String>,
) -> Result<Vec<AuditEvent>, String> {
    state
        .ctx
        .storage
        .list_audits(limit.unwrap_or(200).clamp(1, 500), server_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn verify_audit_chain(state: State<'_, DesktopState>) -> Result<AuditChainStatus, String> {
    state
        .ctx
        .storage
        .verify_audit_chain()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_approvals(state: State<'_, DesktopState>) -> Result<Vec<ApprovalRequest>, String> {
    Ok(state.broker.list_pending().await)
}

#[tauri::command]
async fn decide_approval(
    state: State<'_, DesktopState>,
    id: String,
    approved: bool,
) -> Result<DecideResult, String> {
    let decision = state.broker.resolve(&id, approved).await?;
    Ok(DecideResult { id, decision })
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,sshield=debug".to_string()),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("sshield");
            let db_url = format!("sqlite://{}", data_dir.join("sshield.db").display());
            let audit_jsonl = data_dir.join("audit.jsonl");
            let vault_path = data_dir.join("vault.enc");

            let (broker, ctx) = tauri::async_runtime::block_on(async {
                let storage = Storage::connect(&db_url, Some(audit_jsonl)).await?;
                storage
                    .seed_default_rules_if_empty(&SecurityEngine::default_rules())
                    .await?;
                let vault = Arc::new(SealedVault::new(vault_path, 900));
                let broker = Arc::new(TauriApprovalBroker::new(handle));
                let ctx = Arc::new(SshieldContext::new(
                    storage,
                    Arc::clone(&vault),
                    Arc::new(SshPool::with_vault(vault)),
                    Arc::new(SecurityEngine::new()),
                    broker.clone(),
                    "desktop",
                ));
                Ok::<_, sshield_core::SshieldError>((broker, ctx))
            })?;

            app.manage(DesktopState { ctx, broker });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            vault_status,
            vault_initialize,
            vault_unlock,
            vault_lock,
            list_credentials,
            store_credential,
            delete_credential,
            list_servers,
            upsert_server,
            delete_server,
            test_server,
            import_ssh_config,
            list_rules,
            upsert_rule,
            delete_rule,
            get_l2_config,
            upsert_l2_config,
            list_snippets,
            upsert_snippet,
            delete_snippet,
            evaluate_command,
            list_audits,
            verify_audit_chain,
            list_approvals,
            decide_approval,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sshield desktop");
}
