//! Native MCP Server implementation (via `rmcp`) supporting `stdio` & `Streamable HTTP` transports,
//! dual-layer L1 AST + L2 AI semantic review, output masking, and full audit traceability.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sshield_core::{
    ApprovalDecision, ApprovalHandler, ApprovalRequest, AuditEvent, AuthorizedServerInfo,
    PolicyDecision, SemanticReviewRequest, SemanticReviewVerdict, SemanticReviewer, ServerAsset,
    truncate_preview,
};
use sshield_security::SecurityEngine;
use sshield_security::l2::UnifiedL2Reviewer;
use sshield_security::masking::mask_sensitive_output;
use sshield_ssh::SshPool;
use sshield_storage::Storage;
use sshield_vault::SealedVault;
use uuid::Uuid;

/// Shared runtime context used by MCP servers, Axum server, and Tauri desktop.
#[derive(Clone)]
pub struct SshieldContext {
    pub storage: Storage,
    pub vault: Arc<SealedVault>,
    pub ssh_pool: Arc<SshPool>,
    pub security: Arc<SecurityEngine>,
    pub approval_handler: Arc<dyn ApprovalHandler>,
    pub actor: String,
}

impl SshieldContext {
    pub fn new(
        storage: Storage,
        vault: Arc<SealedVault>,
        ssh_pool: Arc<SshPool>,
        security: Arc<SecurityEngine>,
        approval_handler: Arc<dyn ApprovalHandler>,
        actor: impl Into<String>,
    ) -> Self {
        Self {
            storage,
            vault,
            ssh_pool,
            security,
            approval_handler,
            actor: actor.into(),
        }
    }

    /// Evaluate L1 AST + L2 AI semantic review (if enabled) for any command.
    pub async fn evaluate_dual_layer(
        &self,
        asset: &ServerAsset,
        command: &str,
        intent: &str,
    ) -> sshield_core::Result<(
        sshield_core::CommandVerdict,
        Option<SemanticReviewVerdict>,
        PolicyDecision,
    )> {
        let rules = self.storage.list_rules().await?;
        let l1_verdict = self.security.evaluate(command, &rules);

        if l1_verdict.decision == PolicyDecision::Deny {
            return Ok((l1_verdict, None, PolicyDecision::Deny));
        }

        let l2_cfg = self.storage.get_l2_config().await?;
        if !l2_cfg.enabled {
            let final_decision = l1_verdict.decision;
            return Ok((l1_verdict, None, final_decision));
        }

        let reviewer = UnifiedL2Reviewer::new(Arc::clone(&self.vault), l2_cfg);
        let l2_req = SemanticReviewRequest {
            server_name: asset.name.clone(),
            server_tags: asset.tags.clone(),
            command: command.to_string(),
            intent: intent.to_string(),
            l1_verdict: l1_verdict.clone(),
        };
        let l2_verdict = reviewer.review(&l2_req).await?;
        let final_decision = l1_verdict.decision.max(l2_verdict.decision);

        Ok((l1_verdict, Some(l2_verdict), final_decision))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecuteCommandParams {
    /// Target server asset ID (obtained from `list_authorized_servers`).
    pub server_id: String,
    /// Shell command to execute on the target server.
    pub command: String,
    /// Clear explanation of why this command is being run and what it intends to achieve.
    pub intent: String,
    /// If true, only parse the command AST and evaluate L1/L2 security rules without executing.
    #[serde(default)]
    pub dry_run: Option<bool>,
    /// Execution timeout in seconds (default: 30, max: 300).
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ExecuteCommandResponse {
    trace_id: String,
    status: String,
    dry_run: bool,
    decision: PolicyDecision,
    approval_status: Option<ApprovalDecision>,
    verdict_reason: String,
    parsed_commands: Vec<sshield_core::ParsedCommandInfo>,
    l2_verdict: Option<SemanticReviewVerdict>,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    stdout: Option<String>,
    stderr: Option<String>,
}

/// MCP Server handler exposing Sshield tools to AI agents.
#[derive(Clone)]
pub struct SshieldMcpServer {
    ctx: Arc<SshieldContext>,
}

impl SshieldMcpServer {
    pub fn new(ctx: Arc<SshieldContext>) -> Self {
        Self { ctx }
    }
}

#[tool_router]
impl SshieldMcpServer {
    #[tool(description = "List all authorized and enabled SSH server assets that can be targeted by execute_command.")]
    async fn list_authorized_servers(&self) -> Result<CallToolResult, McpError> {
        let servers = self
            .ctx
            .storage
            .list_servers()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let authorized: Vec<AuthorizedServerInfo> = servers
            .iter()
            .filter(|s| s.enabled)
            .map(AuthorizedServerInfo::from)
            .collect();

        let json = serde_json::to_string_pretty(&authorized)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(description = "Execute a shell command on an authorized SSH server after L1 AST + L2 AI semantic evaluation, optional human approval, and output masking.")]
    async fn execute_command(
        &self,
        Parameters(params): Parameters<ExecuteCommandParams>,
    ) -> Result<CallToolResult, McpError> {
        let dry_run = params.dry_run.unwrap_or(false);
        let timeout_secs = params.timeout.unwrap_or(30).clamp(1, 300);
        let trace_id = Uuid::new_v4().to_string();
        let audit_id = Uuid::new_v4().to_string();

        let all_servers = self
            .ctx
            .storage
            .list_servers()
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let asset_map: HashMap<String, ServerAsset> = all_servers
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();

        let asset = asset_map.get(&params.server_id).ok_or_else(|| {
            McpError::invalid_params(
                format!("Server asset `{}` not found", params.server_id),
                None,
            )
        })?;

        if !asset.enabled {
            return Err(McpError::invalid_params(
                format!("Server asset `{}` is disabled", asset.name),
                None,
            ));
        }

        let (l1_verdict, l2_verdict, final_decision) = self
            .ctx
            .evaluate_dual_layer(asset, &params.command, &params.intent)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let verdict_reason = l2_verdict
            .as_ref()
            .map(|l2| format!("L1: {} | L2: {}", l1_verdict.reason, l2.reasoning))
            .unwrap_or_else(|| l1_verdict.reason.clone());

        // 1. Denied by L1 or L2
        if final_decision == PolicyDecision::Deny {
            let event = AuditEvent {
                id: audit_id,
                trace_id: trace_id.clone(),
                actor: self.ctx.actor.clone(),
                timestamp: Utc::now(),
                server_id: asset.id.clone(),
                server_name: asset.name.clone(),
                command: params.command.clone(),
                intent: params.intent.clone(),
                dry_run,
                decision: final_decision,
                l1_verdict: Some(l1_verdict.clone()),
                l2_verdict: l2_verdict.clone(),
                approval_id: None,
                approval_status: None,
                exit_code: None,
                duration_ms: None,
                stdout_preview: None,
                stderr_preview: None,
                error: Some(verdict_reason.clone()),
                prev_hash: String::new(),
                event_hash: String::new(),
            };
            let _ = self.ctx.storage.append_audit(&event).await;

            let resp = ExecuteCommandResponse {
                trace_id,
                status: "denied".to_string(),
                dry_run,
                decision: final_decision,
                approval_status: None,
                verdict_reason,
                parsed_commands: l1_verdict.parsed_commands,
                l2_verdict,
                exit_code: None,
                duration_ms: None,
                stdout: None,
                stderr: None,
            };
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )]));
        }

        // 2. Dry-run returns L1 + L2 breakdown without executing
        if dry_run {
            let event = AuditEvent {
                id: audit_id,
                trace_id: trace_id.clone(),
                actor: self.ctx.actor.clone(),
                timestamp: Utc::now(),
                server_id: asset.id.clone(),
                server_name: asset.name.clone(),
                command: params.command.clone(),
                intent: params.intent.clone(),
                dry_run: true,
                decision: final_decision,
                l1_verdict: Some(l1_verdict.clone()),
                l2_verdict: l2_verdict.clone(),
                approval_id: None,
                approval_status: None,
                exit_code: None,
                duration_ms: None,
                stdout_preview: None,
                stderr_preview: None,
                error: None,
                prev_hash: String::new(),
                event_hash: String::new(),
            };
            let _ = self.ctx.storage.append_audit(&event).await;

            let resp = ExecuteCommandResponse {
                trace_id,
                status: "dry_run".to_string(),
                dry_run: true,
                decision: final_decision,
                approval_status: None,
                verdict_reason,
                parsed_commands: l1_verdict.parsed_commands,
                l2_verdict,
                exit_code: None,
                duration_ms: None,
                stdout: None,
                stderr: None,
            };
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )]));
        }

        // 3. Human-in-the-loop approval if required
        let mut approval_id = None;
        let mut approval_status = None;
        if final_decision == PolicyDecision::AskApproval {
            let app_id = Uuid::new_v4().to_string();
            approval_id = Some(app_id.clone());
            let approval_req = ApprovalRequest {
                id: app_id,
                trace_id: trace_id.clone(),
                created_at: Utc::now(),
                server_id: asset.id.clone(),
                server_name: asset.name.clone(),
                host: asset.host.clone(),
                username: asset.username.clone(),
                command: params.command.clone(),
                intent: params.intent.clone(),
                verdict: l1_verdict.clone(),
                l2_verdict: l2_verdict.clone(),
                timeout_secs: 120,
            };

            let decision = self
                .ctx
                .approval_handler
                .request_approval(approval_req)
                .await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            approval_status = Some(decision);

            if decision != ApprovalDecision::Approved {
                let reason = format!("Command not approved by operator: {decision}");
                let event = AuditEvent {
                    id: audit_id,
                    trace_id: trace_id.clone(),
                    actor: self.ctx.actor.clone(),
                    timestamp: Utc::now(),
                    server_id: asset.id.clone(),
                    server_name: asset.name.clone(),
                    command: params.command.clone(),
                    intent: params.intent.clone(),
                    dry_run: false,
                    decision: final_decision,
                    l1_verdict: Some(l1_verdict.clone()),
                    l2_verdict: l2_verdict.clone(),
                    approval_id,
                    approval_status,
                    exit_code: None,
                    duration_ms: None,
                    stdout_preview: None,
                    stderr_preview: None,
                    error: Some(reason.clone()),
                    prev_hash: String::new(),
                    event_hash: String::new(),
                };
                let _ = self.ctx.storage.append_audit(&event).await;

                let resp = ExecuteCommandResponse {
                    trace_id,
                    status: format!("{decision}"),
                    dry_run: false,
                    decision: final_decision,
                    approval_status,
                    verdict_reason: reason,
                    parsed_commands: l1_verdict.parsed_commands,
                    l2_verdict,
                    exit_code: None,
                    duration_ms: None,
                    stdout: None,
                    stderr: None,
                };
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    serde_json::to_string_pretty(&resp).unwrap_or_default(),
                )]));
            }
        }

        // 4. Execute via SSH pool + redact sensitive secrets in stdout/stderr
        match self
            .ctx
            .ssh_pool
            .execute_command(
                asset,
                &asset_map,
                &params.command,
                Duration::from_secs(timeout_secs),
            )
            .await
        {
            Ok(out) => {
                let masked_stdout = mask_sensitive_output(&out.stdout);
                let masked_stderr = mask_sensitive_output(&out.stderr);

                let event = AuditEvent {
                    id: audit_id,
                    trace_id: trace_id.clone(),
                    actor: self.ctx.actor.clone(),
                    timestamp: Utc::now(),
                    server_id: asset.id.clone(),
                    server_name: asset.name.clone(),
                    command: params.command.clone(),
                    intent: params.intent.clone(),
                    dry_run: false,
                    decision: final_decision,
                    l1_verdict: Some(l1_verdict.clone()),
                    l2_verdict: l2_verdict.clone(),
                    approval_id,
                    approval_status,
                    exit_code: Some(out.exit_code),
                    duration_ms: Some(out.duration_ms),
                    stdout_preview: Some(truncate_preview(&masked_stdout, 2000)),
                    stderr_preview: if masked_stderr.is_empty() {
                        None
                    } else {
                        Some(truncate_preview(&masked_stderr, 1000))
                    },
                    error: None,
                    prev_hash: String::new(),
                    event_hash: String::new(),
                };
                let _ = self.ctx.storage.append_audit(&event).await;

                let resp = ExecuteCommandResponse {
                    trace_id,
                    status: "executed".to_string(),
                    dry_run: false,
                    decision: final_decision,
                    approval_status,
                    verdict_reason,
                    parsed_commands: l1_verdict.parsed_commands,
                    l2_verdict,
                    exit_code: Some(out.exit_code),
                    duration_ms: Some(out.duration_ms),
                    stdout: Some(masked_stdout),
                    stderr: if masked_stderr.is_empty() {
                        None
                    } else {
                        Some(masked_stderr)
                    },
                };
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    serde_json::to_string_pretty(&resp).unwrap_or_default(),
                )]))
            }
            Err(err) => {
                let err_msg = err.to_string();
                let event = AuditEvent {
                    id: audit_id,
                    trace_id,
                    actor: self.ctx.actor.clone(),
                    timestamp: Utc::now(),
                    server_id: asset.id.clone(),
                    server_name: asset.name.clone(),
                    command: params.command.clone(),
                    intent: params.intent.clone(),
                    dry_run: false,
                    decision: final_decision,
                    l1_verdict: Some(l1_verdict),
                    l2_verdict,
                    approval_id,
                    approval_status,
                    exit_code: None,
                    duration_ms: None,
                    stdout_preview: None,
                    stderr_preview: None,
                    error: Some(err_msg.clone()),
                    prev_hash: String::new(),
                    event_hash: String::new(),
                };
                let _ = self.ctx.storage.append_audit(&event).await;

                Err(McpError::internal_error(err_msg, None))
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for SshieldMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("sshield-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Sshield is an AI-native SSH security gateway. Always call `list_authorized_servers` first to discover server IDs, then use `execute_command` with a clear `intent`.",
            )
    }
}

/// Run the MCP server over `stdin`/`stdout` (for local desktop & Cursor/Claude integration).
pub async fn serve_stdio(server: SshieldMcpServer) -> anyhow::Result<()> {
    let running = server.serve(rmcp::transport::stdio()).await?;
    running.waiting().await?;
    Ok(())
}
