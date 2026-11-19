import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  Code2,
  KeyRound,
  Lock,
  Plug,
  RefreshCw,
  Server,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Unlock,
} from "lucide-react";
import { ApprovalModal } from "@/components/sshield/ApprovalModal";
import { AstShieldView } from "@/components/sshield/AstShieldView";
import { AuditLogsView } from "@/components/sshield/AuditLogsView";
import { HostFormDialog, type HostFormState } from "@/components/sshield/HostFormDialog";
import { HostsVaultView } from "@/components/sshield/HostsVaultView";
import { ImportSshConfigDialog } from "@/components/sshield/ImportSshConfigDialog";
import { KeychainVaultView } from "@/components/sshield/KeychainVaultView";
import { McpKeychainView } from "@/components/sshield/McpKeychainView";
import { RuleFormDialog } from "@/components/sshield/RuleFormDialog";
import { SnippetsView } from "@/components/sshield/SnippetsView";
import { VaultUnlockDialog } from "@/components/sshield/VaultUnlockDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { transport } from "@/lib/transport";
import type {
  ApprovalRequest,
  AuditChainStatus,
  AuditEvent,
  CommandSnippet,
  L2ProviderConfig,
  SecurityRule,
  ServerAsset,
  VaultCredentialMeta,
  VaultStatus,
} from "@/types";

type NavSection = "vault" | "keychain" | "snippets" | "shield" | "audits" | "mcp";

const EMPTY_HOST_FORM: HostFormState = {
  name: "",
  host: "",
  port: 22,
  username: "root",
  auth_kind: "agent_or_default_key",
  identity_file: "",
  credential_id: null,
  jump_host_id: null,
  group_name: "",
  tags: [],
  enabled: true,
};

const EMPTY_RULE_FORM: Partial<SecurityRule> = {
  name: "",
  match_kind: "command_prefix",
  pattern: "",
  decision: "ask_approval",
  priority: 50,
  enabled: true,
};

const DEFAULT_L2: L2ProviderConfig = {
  id: "default-l2",
  name: "Default L2 Reviewer",
  protocol: "system_one_fast",
  base_url: "http://127.0.0.1:8080/v1/decide",
  model: "systemone-guard-v1",
  api_key_credential_id: null,
  timeout_ms: 5000,
  risk_threshold_ask: 40,
  risk_threshold_deny: 85,
  enabled: false,
};

export function App() {
  const [nav, setNav] = useState<NavSection>("vault");
  const [servers, setServers] = useState<ServerAsset[]>([]);
  const [credentials, setCredentials] = useState<VaultCredentialMeta[]>([]);
  const [vaultStatus, setVaultStatus] = useState<VaultStatus>({
    state: "locked",
    credential_count: 0,
    auto_lock_secs: 900,
  });
  const [snippets, setSnippets] = useState<CommandSnippet[]>([]);
  const [rules, setRules] = useState<SecurityRule[]>([]);
  const [l2Config, setL2Config] = useState<L2ProviderConfig>(DEFAULT_L2);
  const [audits, setAudits] = useState<AuditEvent[]>([]);
  const [chainStatus, setChainStatus] = useState<AuditChainStatus | null>(null);
  const [approvals, setApprovals] = useState<ApprovalRequest[]>([]);
  const [selectedHostId, setSelectedHostId] = useState<string | null>(null);
  const [banner, setBanner] = useState<{ kind: "ok" | "err"; text: string } | null>(null);

  const [hostDialogOpen, setHostDialogOpen] = useState(false);
  const [hostForm, setHostForm] = useState<HostFormState>(EMPTY_HOST_FORM);
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [ruleDialogOpen, setRuleDialogOpen] = useState(false);
  const [ruleForm, setRuleForm] = useState<Partial<SecurityRule>>(EMPTY_RULE_FORM);
  const [unlockDialogOpen, setUnlockDialogOpen] = useState(false);

  const serverMap = useMemo(() => {
    const map = new Map<string, ServerAsset>();
    servers.forEach((s) => map.set(s.id, s));
    return map;
  }, [servers]);

  const loadAll = async () => {
    try {
      const [vs, creds, s, sn, r, l2, a, p] = await Promise.all([
        transport.vaultStatus(),
        transport.listCredentials(),
        transport.listServers(),
        transport.listSnippets(),
        transport.listRules(),
        transport.getL2Config(),
        transport.listAudits(200),
        transport.listApprovals(),
      ]);
      setVaultStatus(vs);
      setCredentials(creds);
      setServers(s);
      setSnippets(sn);
      setRules(r);
      setL2Config(l2);
      setAudits(a);
      setApprovals(p);
    } catch (e) {
      setBanner({ kind: "err", text: e instanceof Error ? e.message : String(e) });
    }
  };

  useEffect(() => {
    void loadAll();
    const unsub = transport.subscribeEvents((ev) => {
      if (ev.type === "approval_requested") {
        setApprovals((prev) => [ev.payload, ...prev.filter((x) => x.id !== ev.payload.id)]);
      } else if (ev.type === "approval_resolved") {
        setApprovals((prev) => prev.filter((x) => x.id !== ev.payload.id));
        void transport.listAudits(200).then(setAudits).catch(() => {});
      }
    });
    return unsub;
  }, []);

  const handleSaveHost = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      const saved = await transport.upsertServer({
        id: hostForm.id ?? "",
        name: hostForm.name || hostForm.host || "unnamed",
        host: hostForm.host || "",
        port: Number(hostForm.port) || 22,
        username: hostForm.username || "root",
        auth_kind: hostForm.credential_id
          ? "keyring_password"
          : hostForm.identity_file
            ? "identity_file"
            : "agent_or_default_key",
        identity_file: hostForm.identity_file || null,
        credential_id: hostForm.credential_id ?? null,
        jump_host_id: hostForm.jump_host_id ?? null,
        group_name: hostForm.group_name?.trim() || null,
        tags: hostForm.tags ?? [],
        description: hostForm.description ?? null,
        enabled: hostForm.enabled ?? true,
      });
      setHostDialogOpen(false);
      setSelectedHostId(saved.id);
      setBanner({ kind: "ok", text: `Saved host ${saved.name}` });
      await loadAll();
    } catch (err) {
      setBanner({ kind: "err", text: err instanceof Error ? err.message : String(err) });
    }
  };

  const handleTestHost = async (host: ServerAsset) => {
    setBanner({ kind: "ok", text: `Connecting to ${host.username}@${host.host}:${host.port}...` });
    try {
      const res = await transport.testServer(host.id);
      setBanner({ kind: res.ok ? "ok" : "err", text: `${host.name}: ${res.message}` });
    } catch (err) {
      setBanner({ kind: "err", text: err instanceof Error ? err.message : String(err) });
    }
  };

  const handleSaveRule = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      await transport.upsertRule({
        id: "",
        name: ruleForm.name || `${ruleForm.decision}: ${ruleForm.pattern}`,
        match_kind: ruleForm.match_kind ?? "command_prefix",
        pattern: ruleForm.pattern || "",
        decision: ruleForm.decision ?? "ask_approval",
        description: null,
        enabled: true,
        priority: Number(ruleForm.priority) || 50,
      });
      setRuleDialogOpen(false);
      setRuleForm(EMPTY_RULE_FORM);
      await loadAll();
    } catch (err) {
      setBanner({ kind: "err", text: err instanceof Error ? err.message : String(err) });
    }
  };

  const handleDecideApproval = async (id: string, approved: boolean) => {
    try {
      await transport.decideApproval(id, approved);
      setApprovals((prev) => prev.filter((a) => a.id !== id));
      setBanner({ kind: "ok", text: approved ? "Command approved" : "Command rejected" });
      setTimeout(() => void transport.listAudits(200).then(setAudits).catch(() => {}), 300);
    } catch (err) {
      setBanner({ kind: "err", text: err instanceof Error ? err.message : String(err) });
    }
  };

  const navItems: { id: NavSection; label: string; icon: typeof Server; count?: number }[] = [
    { id: "vault", label: "Hosts", icon: Server, count: servers.length },
    { id: "keychain", label: "Keychain", icon: KeyRound, count: credentials.length },
    { id: "snippets", label: "Snippets", icon: Code2, count: snippets.length },
    { id: "shield", label: "Shield", icon: ShieldCheck, count: rules.length },
    { id: "audits", label: "Audit & Trace", icon: Activity, count: audits.length },
    { id: "mcp", label: "MCP Setup", icon: Plug },
  ];

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-[#0D1017] text-slate-100">
      {/* Termius-style Top Tab Bar (macOS Overlay Drag Region) */}
      <header
        data-tauri-drag-region
        className="flex h-11 shrink-0 items-center justify-between border-b border-white/[0.07] bg-[#10131C] px-4 pl-20 select-none"
      >
        <div className="flex items-center gap-2" data-tauri-drag-region>
          <div className="flex items-center gap-2 rounded-md bg-[#181D2A] px-3 py-1 text-xs font-medium text-white ring-1 ring-white/10">
            <Shield className="size-3.5 text-emerald-400" />
            <span>Sshield Vault</span>
          </div>
          {l2Config.enabled && (
            <Badge variant="outline" className="border-indigo-500/30 bg-indigo-500/10 text-[10px] text-indigo-300">
              L2: {l2Config.model}
            </Badge>
          )}
        </div>

        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="ghost"
            className={`h-7 text-xs ${
              vaultStatus.state === "unlocked"
                ? "text-emerald-300 hover:bg-emerald-500/10"
                : "text-amber-300 hover:bg-amber-500/10"
            }`}
            onClick={() => {
              if (vaultStatus.state === "unlocked") {
                void transport.vaultLock().then(setVaultStatus);
              } else {
                setUnlockDialogOpen(true);
              }
            }}
          >
            {vaultStatus.state === "unlocked" ? (
              <>
                <Unlock className="mr-1 size-3.5" /> Vault Unlocked
              </>
            ) : (
              <>
                <Lock className="mr-1 size-3.5" />
                {vaultStatus.state === "uninitialized" ? "Init Vault" : "Vault Locked"}
              </>
            )}
          </Button>
          {approvals.length > 0 && (
            <Badge className="animate-pulse gap-1 border-amber-500/40 bg-amber-500/20 text-amber-300">
              <ShieldAlert className="size-3.5" /> {approvals.length} Pending
            </Badge>
          )}
          <Button size="sm" variant="ghost" className="h-7 text-xs text-slate-400 hover:text-white" onClick={loadAll}>
            <RefreshCw className="mr-1 size-3.5" /> Sync
          </Button>
        </div>
      </header>

      <div className="flex flex-1 overflow-hidden">
        {/* Termius-style Left Navigation Rail */}
        <aside className="flex w-52 shrink-0 flex-col border-r border-white/[0.07] bg-[#141824] p-2.5">
          <nav className="space-y-0.5">
            {navItems.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  key={item.id}
                  onClick={() => setNav(item.id)}
                  className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-xs font-medium transition ${
                    nav === item.id
                      ? "bg-[#1F2637] text-white ring-1 ring-white/10"
                      : "text-slate-400 hover:bg-white/[0.04] hover:text-slate-200"
                  }`}
                >
                  <Icon className={`size-4 ${nav === item.id ? "text-emerald-400" : ""}`} />
                  <span>{item.label}</span>
                  {item.count != null && <span className="ml-auto text-[11px] text-slate-500">{item.count}</span>}
                </button>
              );
            })}
          </nav>
        </aside>

        <main className="flex flex-1 flex-col overflow-hidden">
          {banner && (
            <div
              className={`mx-6 mt-4 flex items-center justify-between rounded-lg border px-4 py-2 text-xs ${
                banner.kind === "ok"
                  ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-200"
                  : "border-rose-500/30 bg-rose-500/10 text-rose-200"
              }`}
            >
              <span>{banner.text}</span>
              <button onClick={() => setBanner(null)} className="underline opacity-80 hover:opacity-100">
                Close
              </button>
            </div>
          )}

          {nav === "vault" && (
            <HostsVaultView
              servers={servers}
              serverMap={serverMap}
              credentials={credentials}
              selectedHostId={selectedHostId}
              onSelectHost={setSelectedHostId}
              onOpenNewHost={() => {
                setHostForm(EMPTY_HOST_FORM);
                setHostDialogOpen(true);
              }}
              onOpenEditHost={(host) => {
                setHostForm({ ...host });
                setHostDialogOpen(true);
              }}
              onOpenImport={() => setImportDialogOpen(true)}
              onTestHost={handleTestHost}
              onToggleEnabled={async (host, enabled) => {
                await transport.upsertServer({ ...host, enabled });
                await loadAll();
              }}
              onDeleteHost={async (id) => {
                await transport.deleteServer(id);
                setSelectedHostId(null);
                await loadAll();
              }}
            />
          )}

          {nav === "keychain" && (
            <KeychainVaultView
              vaultStatus={vaultStatus}
              credentials={credentials}
              onOpenUnlock={() => setUnlockDialogOpen(true)}
              onLockVault={async () => {
                setVaultStatus(await transport.vaultLock());
              }}
              onStoreCredential={async (input) => {
                await transport.storeCredential(input);
                await loadAll();
              }}
              onDeleteCredential={async (id) => {
                await transport.deleteCredential(id);
                await loadAll();
              }}
            />
          )}

          {nav === "snippets" && (
            <SnippetsView
              snippets={snippets}
              onSaveSnippet={async (snip) => {
                await transport.upsertSnippet(snip);
                await loadAll();
              }}
              onDeleteSnippet={async (id) => {
                await transport.deleteSnippet(id);
                await loadAll();
              }}
            />
          )}

          {nav === "shield" && (
            <AstShieldView
              rules={rules}
              l2Config={l2Config}
              credentials={credentials}
              onOpenNewRule={() => setRuleDialogOpen(true)}
              onToggleRule={async (rule, enabled) => {
                await transport.upsertRule({ ...rule, enabled });
                await loadAll();
              }}
              onDeleteRule={async (id) => {
                await transport.deleteRule(id);
                await loadAll();
              }}
              onSaveL2Config={async (cfg) => {
                const saved = await transport.upsertL2Config(cfg);
                setL2Config(saved);
                setBanner({ kind: "ok", text: "Saved L2 Semantic Reviewer strategy" });
              }}
              onEvaluateCommand={transport.evaluateCommand}
            />
          )}

          {nav === "audits" && (
            <AuditLogsView
              audits={audits}
              chainStatus={chainStatus}
              onVerifyChain={async () => {
                setChainStatus(await transport.verifyAuditChain());
              }}
            />
          )}

          {nav === "mcp" && <McpKeychainView />}
        </main>
      </div>

      <HostFormDialog
        open={hostDialogOpen}
        onOpenChange={setHostDialogOpen}
        form={hostForm}
        setForm={setHostForm}
        servers={servers}
        credentials={credentials}
        onSubmit={handleSaveHost}
      />
      <ImportSshConfigDialog
        open={importDialogOpen}
        onOpenChange={setImportDialogOpen}
        onImport={async (content) => {
          const imported = await transport.importSshConfig(content);
          setImportDialogOpen(false);
          setBanner({ kind: "ok", text: `Imported ${imported.length} SSH host(s)` });
          await loadAll();
        }}
      />
      <RuleFormDialog
        open={ruleDialogOpen}
        onOpenChange={setRuleDialogOpen}
        form={ruleForm}
        setForm={setRuleForm}
        onSubmit={handleSaveRule}
      />
      <VaultUnlockDialog
        open={unlockDialogOpen}
        onOpenChange={setUnlockDialogOpen}
        state={vaultStatus.state}
        onSubmit={async (passphrase) => {
          const next =
            vaultStatus.state === "uninitialized"
              ? await transport.vaultInitialize(passphrase)
              : await transport.vaultUnlock(passphrase);
          setVaultStatus(next);
          setBanner({ kind: "ok", text: "Vault unlocked" });
        }}
      />
      <ApprovalModal approval={approvals[0] ?? null} onDecide={handleDecideApproval} />
    </div>
  );
}
