import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ApprovalDecision,
  ApprovalRequest,
  AuditChainStatus,
  AuditEvent,
  CommandSnippet,
  DualEvaluateResp,
  L2ProviderConfig,
  SecurityRule,
  ServerAsset,
  VaultCredentialKind,
  VaultCredentialMeta,
  VaultStatus,
  WsEvent,
} from "@/types";

export interface TransportAdapter {
  isTauri: boolean;
  vaultStatus(): Promise<VaultStatus>;
  vaultInitialize(passphrase: string): Promise<VaultStatus>;
  vaultUnlock(passphrase: string): Promise<VaultStatus>;
  vaultLock(): Promise<VaultStatus>;
  listCredentials(): Promise<VaultCredentialMeta[]>;
  storeCredential(input: {
    id?: string;
    label: string;
    kind: VaultCredentialKind;
    username?: string;
    secret: string;
  }): Promise<VaultCredentialMeta>;
  deleteCredential(id: string): Promise<void>;

  listServers(): Promise<ServerAsset[]>;
  upsertServer(asset: ServerAsset): Promise<ServerAsset>;
  deleteServer(id: string): Promise<void>;
  testServer(id: string): Promise<{ ok: boolean; message: string }>;
  importSshConfig(content?: string): Promise<ServerAsset[]>;

  listRules(): Promise<SecurityRule[]>;
  upsertRule(rule: SecurityRule): Promise<SecurityRule>;
  deleteRule(id: string): Promise<void>;

  getL2Config(): Promise<L2ProviderConfig>;
  upsertL2Config(config: L2ProviderConfig): Promise<L2ProviderConfig>;
  evaluateCommand(command: string, intent?: string, serverId?: string): Promise<DualEvaluateResp>;

  listSnippets(): Promise<CommandSnippet[]>;
  upsertSnippet(snippet: CommandSnippet): Promise<CommandSnippet>;
  deleteSnippet(id: string): Promise<void>;

  listAudits(limit?: number, serverId?: string): Promise<AuditEvent[]>;
  verifyAuditChain(): Promise<AuditChainStatus>;
  listApprovals(): Promise<ApprovalRequest[]>;
  decideApproval(id: string, approved: boolean): Promise<{ id: string; decision: ApprovalDecision }>;
  subscribeEvents(onEvent: (ev: WsEvent) => void): () => void;
}

function detectTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function apiJson<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
    ...init,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `HTTP ${res.status}`);
  }
  return (await res.json()) as T;
}

const tauriTransport: TransportAdapter = {
  isTauri: true,
  vaultStatus: () => invoke("vault_status"),
  vaultInitialize: (passphrase) => invoke("vault_initialize", { passphrase }),
  vaultUnlock: (passphrase) => invoke("vault_unlock", { passphrase }),
  vaultLock: () => invoke("vault_lock"),
  listCredentials: () => invoke("list_credentials"),
  storeCredential: (input) => invoke("store_credential", input),
  deleteCredential: (id) => invoke("delete_credential", { id }),

  listServers: () => invoke("list_servers"),
  upsertServer: (asset) => invoke("upsert_server", { asset }),
  deleteServer: (id) => invoke("delete_server", { id }),
  testServer: (id) => invoke("test_server", { id }),
  importSshConfig: (content) => invoke("import_ssh_config", { content: content ?? null }),

  listRules: () => invoke("list_rules"),
  upsertRule: (rule) => invoke("upsert_rule", { rule }),
  deleteRule: (id) => invoke("delete_rule", { id }),

  getL2Config: () => invoke("get_l2_config"),
  upsertL2Config: (config) => invoke("upsert_l2_config", { config }),
  evaluateCommand: (command, intent, serverId) =>
    invoke("evaluate_command", { command, intent: intent ?? null, serverId: serverId ?? null }),

  listSnippets: () => invoke("list_snippets"),
  upsertSnippet: (snippet) => invoke("upsert_snippet", { snippet }),
  deleteSnippet: (id) => invoke("delete_snippet", { id }),

  listAudits: (limit = 200, serverId) =>
    invoke("list_audits", { limit, serverId: serverId ?? null }),
  verifyAuditChain: () => invoke("verify_audit_chain"),
  listApprovals: () => invoke("list_approvals"),
  decideApproval: (id, approved) => invoke("decide_approval", { id, approved }),

  subscribeEvents(onEvent) {
    let unlistenReq: (() => void) | undefined;
    let unlistenRes: (() => void) | undefined;

    void listen<ApprovalRequest>("approval-requested", (e) => {
      onEvent({ type: "approval_requested", payload: e.payload });
    }).then((fn) => {
      unlistenReq = fn;
    });

    void listen<{ id: string; decision: ApprovalDecision }>("approval-resolved", (e) => {
      onEvent({ type: "approval_resolved", payload: e.payload });
    }).then((fn) => {
      unlistenRes = fn;
    });

    return () => {
      unlistenReq?.();
      unlistenRes?.();
    };
  },
};

const httpFallbackTransport: TransportAdapter = {
  isTauri: false,
  vaultStatus: () => apiJson("/api/vault/status"),
  vaultInitialize: (passphrase) =>
    apiJson("/api/vault/initialize", { method: "POST", body: JSON.stringify({ passphrase }) }),
  vaultUnlock: (passphrase) =>
    apiJson("/api/vault/unlock", { method: "POST", body: JSON.stringify({ passphrase }) }),
  vaultLock: () => apiJson("/api/vault/lock", { method: "POST" }),
  listCredentials: () => apiJson("/api/vault/credentials"),
  storeCredential: (input) =>
    apiJson("/api/vault/credentials", { method: "POST", body: JSON.stringify(input) }),
  deleteCredential: async (id) => {
    await apiJson(`/api/vault/credentials/${encodeURIComponent(id)}`, { method: "DELETE" });
  },

  listServers: () => apiJson("/api/servers"),
  upsertServer: (asset) =>
    apiJson("/api/servers", { method: "POST", body: JSON.stringify(asset) }),
  deleteServer: async (id) => {
    await apiJson(`/api/servers/${encodeURIComponent(id)}`, { method: "DELETE" });
  },
  testServer: (id) => apiJson(`/api/servers/${encodeURIComponent(id)}/test`, { method: "POST" }),
  importSshConfig: (content) =>
    apiJson("/api/servers/import-ssh-config", {
      method: "POST",
      body: JSON.stringify({ content: content ?? null }),
    }),

  listRules: () => apiJson("/api/rules"),
  upsertRule: (rule) =>
    apiJson("/api/rules", { method: "POST", body: JSON.stringify(rule) }),
  deleteRule: async (id) => {
    await apiJson(`/api/rules/${encodeURIComponent(id)}`, { method: "DELETE" });
  },

  getL2Config: () => apiJson("/api/l2-config"),
  upsertL2Config: (config) =>
    apiJson("/api/l2-config", { method: "POST", body: JSON.stringify(config) }),
  evaluateCommand: (command, intent, serverId) =>
    apiJson("/api/security/evaluate", {
      method: "POST",
      body: JSON.stringify({ command, intent, server_id: serverId }),
    }),

  listSnippets: () => apiJson("/api/snippets"),
  upsertSnippet: (snippet) =>
    apiJson("/api/snippets", { method: "POST", body: JSON.stringify(snippet) }),
  deleteSnippet: async (id) => {
    await apiJson(`/api/snippets/${encodeURIComponent(id)}`, { method: "DELETE" });
  },

  listAudits: (limit = 200, serverId) => {
    const params = new URLSearchParams({ limit: String(limit) });
    if (serverId) params.set("server_id", serverId);
    return apiJson(`/api/audit-logs?${params.toString()}`);
  },
  verifyAuditChain: () => apiJson("/api/audit-logs/verify"),
  listApprovals: () => apiJson("/api/approvals"),
  decideApproval: (id, approved) =>
    apiJson(`/api/approvals/${encodeURIComponent(id)}/decide`, {
      method: "POST",
      body: JSON.stringify({ approved }),
    }),

  subscribeEvents(onEvent) {
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${proto}//${window.location.host}/ws`;
    let ws: WebSocket | null = null;
    let closed = false;
    let timer: number | undefined;

    const connect = () => {
      if (closed) return;
      ws = new WebSocket(wsUrl);
      ws.onmessage = (msg) => {
        try {
          onEvent(JSON.parse(msg.data) as WsEvent);
        } catch {
          // ignore
        }
      };
      ws.onclose = () => {
        if (!closed) {
          timer = window.setTimeout(connect, 2000);
        }
      };
    };

    connect();
    return () => {
      closed = true;
      if (timer) window.clearTimeout(timer);
      ws?.close();
    };
  },
};

export const transport: TransportAdapter = detectTauri()
  ? tauriTransport
  : httpFallbackTransport;
