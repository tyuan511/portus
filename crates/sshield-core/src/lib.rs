//! Core domain models, policy interfaces, L2 semantic review types,
//! vault metadata, and tamper-evident audit types for Sshield.

use std::fmt;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Unified error type across Sshield crates.
#[derive(Debug, Error)]
pub enum SshieldError {
    #[error("Asset not found: {0}")]
    AssetNotFound(String),

    #[error("Asset disabled: {0}")]
    AssetDisabled(String),

    #[error("Vault is locked; unlock with master passphrase or keychain first")]
    VaultLocked,

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("Security policy denied command: {0}")]
    PolicyDenied(String),

    #[error("Shell syntax parse failed (fail-closed): {0}")]
    ParseFailed(String),

    #[error("L2 semantic review failed: {0}")]
    SemanticReview(String),

    #[error("Approval rejected or timed out: {0}")]
    ApprovalRejected(String),

    #[error("SSH error: {0}")]
    Ssh(String),

    #[error("SSH config import error: {0}")]
    ConfigImport(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Command execution timed out after {0}s")]
    Timeout(u64),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, SshieldError>;

/// Authentication method metadata for a server asset (secrets are never stored here).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    /// Use local SSH agent or default keys (~/.ssh/id_ed25519, id_rsa).
    #[default]
    AgentOrDefaultKey,
    /// Specific private key file path.
    IdentityFile,
    /// Encrypted credential stored in Sshield Vault (`vault.enc`), referenced by `credential_id`.
    KeyringPassword,
}

/// Represents a managed SSH server or bastion jump host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerAsset {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub auth_kind: AuthKind,
    #[serde(default)]
    pub identity_file: Option<String>,
    /// Reference to an encrypted credential in `sshield-vault` (never a raw secret).
    #[serde(default)]
    pub credential_id: Option<String>,
    #[serde(default)]
    pub jump_host_id: Option<String>,
    /// Termius-style host group (e.g. "Production", "Bastion", "Staging").
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Public view of a server asset exposed to AI agents via MCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedServerInfo {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub jump_host_id: Option<String>,
    pub group_name: Option<String>,
    pub tags: Vec<String>,
    pub description: Option<String>,
}

impl From<&ServerAsset> for AuthorizedServerInfo {
    fn from(asset: &ServerAsset) -> Self {
        Self {
            id: asset.id.clone(),
            name: asset.name.clone(),
            host: asset.host.clone(),
            port: asset.port,
            username: asset.username.clone(),
            jump_host_id: asset.jump_host_id.clone(),
            group_name: asset.group_name.clone(),
            tags: asset.tags.clone(),
            description: asset.description.clone(),
        }
    }
}

/// Credential types stored inside the AES-256-GCM sealed vault.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VaultCredentialKind {
    Password,
    PrivateKey,
    ApiKey,
}

/// Non-secret metadata for a credential stored in `sshield-vault`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultCredentialMeta {
    pub id: String,
    pub label: String,
    pub kind: VaultCredentialKind,
    #[serde(default)]
    pub username: Option<String>,
    /// Masked preview (e.g. `sk-***9a2f` or `SHA256:...`), never the plaintext secret.
    pub preview: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VaultLockState {
    Uninitialized,
    Locked,
    Unlocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultStatus {
    pub state: VaultLockState,
    pub credential_count: usize,
    pub auto_lock_secs: u64,
}

/// Termius-style reusable command snippet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSnippet {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// L1/L2 security decision for a command.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    /// Safe to execute automatically.
    Allow,
    /// Requires explicit human-in-the-loop confirmation.
    AskApproval,
    /// Strictly forbidden; blocked immediately.
    Deny,
}

impl fmt::Display for PolicyDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow => write!(f, "allow"),
            Self::AskApproval => write!(f, "ask_approval"),
            Self::Deny => write!(f, "deny"),
        }
    }
}

/// How a security rule matches an extracted command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleMatchKind {
    /// Matches the exact program name (e.g., "ls", "rm").
    ProgramExact,
    /// Matches program + leading subcommand/prefix (e.g., "docker ps", "git status").
    CommandPrefix,
    /// Matches the full reconstructed command string against a regex.
    Regex,
}

/// A configurable L1 security rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityRule {
    pub id: String,
    pub name: String,
    pub match_kind: RuleMatchKind,
    pub pattern: String,
    pub decision: PolicyDecision,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
}

/// A single command invocation extracted from the Shell AST.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedCommandInfo {
    pub program: String,
    pub args: Vec<String>,
    pub raw: String,
    pub context: String,
    pub decision: PolicyDecision,
    pub matched_rule: Option<String>,
}

/// Result of L1 AST security evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandVerdict {
    pub decision: PolicyDecision,
    pub parse_ok: bool,
    pub parsed_commands: Vec<ParsedCommandInfo>,
    pub matched_rules: Vec<String>,
    pub reason: String,
}

/// Supported L2 AI Semantic Reviewer protocols.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum L2Protocol {
    /// OpenAI-compatible `/v1/chat/completions` (OpenAI, DeepSeek, Qwen, vLLM, Ollama).
    OpenAiChat,
    /// OpenAI `/v1/responses` protocol.
    OpenAiResponses,
    /// Anthropic `/v1/messages` protocol.
    AnthropicMessages,
    /// SystemOne ultra-fast dedicated decision model endpoint.
    SystemOneFast,
}

/// Configuration for the L2 AI Semantic Review provider (API key lives in `sshield-vault`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2ProviderConfig {
    pub id: String,
    pub name: String,
    pub protocol: L2Protocol,
    pub base_url: String,
    pub model: String,
    /// Reference ID in `sshield-vault` for the API key (never stored plaintext in SQLite).
    #[serde(default)]
    pub api_key_credential_id: Option<String>,
    /// Request timeout in milliseconds (e.g. 1500ms for SystemOne, 8000ms for LLM).
    #[serde(default = "default_l2_timeout_ms")]
    pub timeout_ms: u64,
    /// Risk score (0-100) at or above which `Allow` escalates to `AskApproval`.
    #[serde(default = "default_risk_ask")]
    pub risk_threshold_ask: u8,
    /// Risk score (0-100) at or above which command is blocked (`Deny`).
    #[serde(default = "default_risk_deny")]
    pub risk_threshold_deny: u8,
    #[serde(default)]
    pub enabled: bool,
}

fn default_l2_timeout_ms() -> u64 {
    5000
}
fn default_risk_ask() -> u8 {
    40
}
fn default_risk_deny() -> u8 {
    85
}

impl Default for L2ProviderConfig {
    fn default() -> Self {
        Self {
            id: "default-l2".to_string(),
            name: "Default L2 Reviewer".to_string(),
            protocol: L2Protocol::SystemOneFast,
            base_url: "http://127.0.0.1:8080/v1/decide".to_string(),
            model: "systemone-guard-v1".to_string(),
            api_key_credential_id: None,
            timeout_ms: default_l2_timeout_ms(),
            risk_threshold_ask: default_risk_ask(),
            risk_threshold_deny: default_risk_deny(),
            enabled: false,
        }
    }
}

/// Input to the unified L2 semantic reviewer interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticReviewRequest {
    pub server_name: String,
    pub server_tags: Vec<String>,
    pub command: String,
    pub intent: String,
    pub l1_verdict: CommandVerdict,
}

/// Structured output from any L2 semantic reviewer (LLM or SystemOne).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticReviewVerdict {
    pub decision: PolicyDecision,
    pub risk_score: u8,
    pub intent_aligned: bool,
    pub reasoning: String,
    pub provider_name: String,
    pub protocol: L2Protocol,
    pub model: String,
    pub latency_ms: u64,
    #[serde(default)]
    pub fallback_triggered: bool,
}

/// Unified async trait for L2 semantic review engines (LLM protocols & SystemOne).
#[async_trait]
pub trait SemanticReviewer: Send + Sync {
    async fn review(&self, req: &SemanticReviewRequest) -> Result<SemanticReviewVerdict>;
}

/// Output of executing a remote command over SSH.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

/// Request sent to the human approval handler when a command is classified as `AskApproval`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub trace_id: String,
    pub created_at: DateTime<Utc>,
    pub server_id: String,
    pub server_name: String,
    pub host: String,
    pub username: String,
    pub command: String,
    pub intent: String,
    pub verdict: CommandVerdict,
    #[serde(default)]
    pub l2_verdict: Option<SemanticReviewVerdict>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Rejected,
    TimedOut,
}

impl fmt::Display for ApprovalDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Approved => write!(f, "approved"),
            Self::Rejected => write!(f, "rejected"),
            Self::TimedOut => write!(f, "timed_out"),
        }
    }
}

/// Pluggable human-in-the-loop approval handler (WebSocket UI, Tauri dialog, CLI prompt, etc.).
#[async_trait]
pub trait ApprovalHandler: Send + Sync {
    async fn request_approval(&self, req: ApprovalRequest) -> Result<ApprovalDecision>;
}

/// Default approval handler that always rejects (used when no interactive UI is attached).
pub struct RejectAllApprovalHandler;

#[async_trait]
impl ApprovalHandler for RejectAllApprovalHandler {
    async fn request_approval(&self, req: ApprovalRequest) -> Result<ApprovalDecision> {
        tracing::warn!(
            approval_id = %req.id,
            trace_id = %req.trace_id,
            server = %req.server_name,
            command = %req.command,
            "No interactive approval handler configured; rejecting command"
        );
        Ok(ApprovalDecision::Rejected)
    }
}

/// Tamper-evident, fully traceable JSON audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default = "default_actor")]
    pub actor: String,
    pub timestamp: DateTime<Utc>,
    pub server_id: String,
    pub server_name: String,
    pub command: String,
    pub intent: String,
    pub dry_run: bool,
    pub decision: PolicyDecision,
    #[serde(default)]
    pub l1_verdict: Option<CommandVerdict>,
    #[serde(default)]
    pub l2_verdict: Option<SemanticReviewVerdict>,
    #[serde(default)]
    pub approval_id: Option<String>,
    pub approval_status: Option<ApprovalDecision>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub stdout_preview: Option<String>,
    pub stderr_preview: Option<String>,
    pub error: Option<String>,
    /// SHA-256 hash of the previous audit record ("GENESIS" for the first record).
    #[serde(default)]
    pub prev_hash: String,
    /// SHA-256 hash linking `prev_hash` + canonical event fields.
    #[serde(default)]
    pub event_hash: String,
}

fn default_actor() -> String {
    "mcp".to_string()
}

/// Verification result for the tamper-evident audit hash chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditChainStatus {
    pub total_events: usize,
    pub valid: bool,
    pub broken_at_id: Option<String>,
}

/// Helper to truncate long output previews for audit logs.
pub fn truncate_preview(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("...[truncated]");
        out
    }
}
