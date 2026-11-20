export type AuthKind = "agent_or_default_key" | "identity_file" | "keyring_password";

export interface ServerAsset {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_kind: AuthKind;
  identity_file?: string | null;
  credential_id?: string | null;
  jump_host_id?: string | null;
  group_name?: string | null;
  tags: string[];
  description?: string | null;
  enabled: boolean;
}

export type VaultCredentialKind = "password" | "private_key" | "api_key";

export interface VaultCredentialMeta {
  id: string;
  label: string;
  kind: VaultCredentialKind;
  username?: string | null;
  preview: string;
  created_at: string;
}

export type VaultLockState = "uninitialized" | "locked" | "unlocked";

export interface VaultStatus {
  state: VaultLockState;
  credential_count: number;
  auto_lock_secs: number;
}

export interface CommandSnippet {
  id: string;
  name: string;
  command: string;
  description?: string | null;
  tags: string[];
}

export type PolicyDecision = "allow" | "ask_approval" | "deny";
export type RuleMatchKind = "program_exact" | "command_prefix" | "regex";

export interface SecurityRule {
  id: string;
  name: string;
  match_kind: RuleMatchKind;
  pattern: string;
  decision: PolicyDecision;
  description?: string | null;
  enabled: boolean;
  priority: number;
}

export interface ParsedCommandInfo {
  program: string;
  args: string[];
  raw: string;
  context: string;
  decision: PolicyDecision;
  matched_rule?: string | null;
}

export interface CommandVerdict {
  decision: PolicyDecision;
  parse_ok: boolean;
  parsed_commands: ParsedCommandInfo[];
  matched_rules: string[];
  reason: string;
}

export type L2Protocol =
  | "open_ai_chat"
  | "open_ai_responses"
  | "anthropic_messages"
  | "system_one_fast";

export interface L2ProviderConfig {
  id: string;
  name: string;
  protocol: L2Protocol;
  base_url: string;
  model: string;
  api_key_credential_id?: string | null;
  timeout_ms: number;
  risk_threshold_ask: number;
  risk_threshold_deny: number;
  enabled: boolean;
}

export interface SemanticReviewVerdict {
  decision: PolicyDecision;
  risk_score: number;
  intent_aligned: boolean;
  reasoning: string;
  provider_name: string;
  protocol: L2Protocol;
  model: string;
  latency_ms: number;
  fallback_triggered: boolean;
}

export interface DualEvaluateResp {
  l1: CommandVerdict;
  l2?: SemanticReviewVerdict | null;
  final_decision: PolicyDecision;
}

export type ApprovalDecision = "approved" | "rejected" | "timed_out";

export interface ApprovalRequest {
  id: string;
  trace_id: string;
  created_at: string;
  server_id: string;
  server_name: string;
  host: string;
  username: string;
  command: string;
  intent: string;
  verdict: CommandVerdict;
  l2_verdict?: SemanticReviewVerdict | null;
  timeout_secs: number;
  user_id?: string;
  requested_by?: string;
}

export interface AuditEvent {
  id: string;
  trace_id: string;
  actor: string;
  timestamp: string;
  user_id?: string;
  username?: string;
  operator?: string;
  ssh_user?: string;
  server_host?: string;
  server_id: string;
  server_name: string;
  command: string;
  intent: string;
  dry_run: boolean;
  decision: PolicyDecision;
  l1_verdict?: CommandVerdict | null;
  l2_verdict?: SemanticReviewVerdict | null;
  approval_id?: string | null;
  approval_status?: ApprovalDecision | null;
  exit_code?: number | null;
  duration_ms?: number | null;
  stdout_preview?: string | null;
  stderr_preview?: string | null;
  error?: string | null;
  prev_hash: string;
  event_hash: string;
}

export interface AuditChainStatus {
  total_events: number;
  valid: boolean;
  broken_at_id?: string | null;
}

export interface AuditStats {
  total_commands: number;
  allowed_count: number;
  approval_count: number;
  denied_count: number;
  failed_count: number;
  avg_duration_ms?: number | null;
  by_actor: { key: string; count: number }[];
  by_user: { key: string; count: number }[];
  by_server: { server_id: string; server_name: string; count: number; failed_count: number }[];
  top_commands: { key: string; count: number }[];
  daily_trend: { date: string; total: number; denied: number; failed: number }[];
}

export type WsEvent =
  | { type: "approval_requested"; payload: ApprovalRequest }
  | { type: "approval_resolved"; payload: { id: string; decision: ApprovalDecision } };

// ===== Enterprise (multi-user) =====

export type Permission =
  | "host.read" | "host.connect" | "host.execute" | "host.manage"
  | "vault.manage" | "rules.read" | "rules.manage"
  | "snippets.use" | "snippets.manage"
  | "audit.view_self" | "audit.view_all" | "approvals.decide" | "mcp.access"
  | "users.manage" | "roles.manage" | "groups.manage" | "access.manage"
  | "settings.manage";

export interface Role {
  id: string;
  name: string;
  permissions: Permission[];
  is_builtin: boolean;
  description?: string;
  created_at?: string;
}

export interface User {
  id: string;
  username: string;
  display_name: string;
  role_id: string;
  role?: Role;
  is_active: boolean;
  must_change_password: boolean;
  totp_enabled: boolean;
  created_at?: string;
  updated_at?: string;
  last_login_at?: string | null;
}

export interface Identity {
  user_id: string;
  username: string;
  display_name: string;
  role_id: string;
  role_name: string;
  permissions: Permission[];
  is_admin: boolean;
  totp_enabled: boolean;
}

export interface UserGroup {
  id: string;
  name: string;
  description?: string;
  created_at?: string;
}

export interface HostGrant {
  id: string;
  user_group_id: string;
  user_group?: UserGroup;
  server_id?: string;
  group_name?: string;
  can_connect: boolean;
  can_execute: boolean;
  created_by?: string;
  created_at?: string;
}

export interface ApiToken {
  id: string;
  user_id: string;
  name: string;
  prefix: string;
  created_at: string;
  last_used_at?: string | null;
  expires_at?: string | null;
  revoked_at?: string | null;
}

export interface UserSession {
  id: string;
  user_id: string;
  ip: string;
  user_agent: string;
  created_at: string;
  last_seen_at: string;
  expires_at: string;
}

export interface CredentialMeta {
  id: string;
  kind: "password" | "private_key" | "api_key";
  label: string;
  username?: string;
  created_by?: string;
  created_at: string;
}

export interface LoginResponse {
  user: Identity;
  must_change_password: boolean;
  recovery_code_used: boolean;
}
