import { useMemo, useState } from "react";
import {
  FolderInput,
  GitBranch,
  KeyRound,
  LayoutGrid,
  List,
  Plus,
  Search,
  Server,
  Terminal,
  Trash2,
  Wifi,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import type { ServerAsset, VaultCredentialMeta } from "@/types";

interface HostsVaultViewProps {
  servers: ServerAsset[];
  serverMap: Map<string, ServerAsset>;
  credentials: VaultCredentialMeta[];
  selectedHostId: string | null;
  onSelectHost: (id: string | null) => void;
  onOpenNewHost: () => void;
  onOpenEditHost: (host: ServerAsset) => void;
  onOpenImport: () => void;
  onTestHost: (host: ServerAsset) => void;
  onToggleEnabled: (host: ServerAsset, enabled: boolean) => void;
  onDeleteHost: (id: string) => void;
}

export function HostsVaultView({
  servers,
  serverMap,
  credentials,
  selectedHostId,
  onSelectHost,
  onOpenNewHost,
  onOpenEditHost,
  onOpenImport,
  onTestHost,
  onToggleEnabled,
  onDeleteHost,
}: HostsVaultViewProps) {
  const [hostSearch, setHostSearch] = useState("");
  const [activeGroup, setActiveGroup] = useState<string>("all");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");

  const credMap = useMemo(() => {
    const m = new Map<string, VaultCredentialMeta>();
    credentials.forEach((c) => m.set(c.id, c));
    return m;
  }, [credentials]);

  const allGroups = useMemo(() => {
    const set = new Set<string>();
    servers.forEach((s) => {
      if (s.group_name) set.add(s.group_name);
    });
    return ["all", ...Array.from(set)];
  }, [servers]);

  const filteredServers = useMemo(() => {
    const q = hostSearch.trim().toLowerCase();
    return servers.filter((s) => {
      const grpMatch = activeGroup === "all" || s.group_name === activeGroup;
      const qMatch =
        !q ||
        s.name.toLowerCase().includes(q) ||
        s.host.toLowerCase().includes(q) ||
        s.username.toLowerCase().includes(q);
      return grpMatch && qMatch;
    });
  }, [servers, hostSearch, activeGroup]);

  const selectedHost = selectedHostId ? serverMap.get(selectedHostId) ?? null : null;

  return (
    <div className="flex flex-1 overflow-hidden">
      <div className="flex-1 overflow-y-auto p-6">
        <div className="mb-5 flex flex-wrap items-center justify-between gap-3">
          <div className="relative w-72">
            <Search className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-slate-500" />
            <Input
              value={hostSearch}
              onChange={(e) => setHostSearch(e.target.value)}
              placeholder="Find a host or ssh user@host..."
              className="border-white/10 bg-[#141824] pl-8 text-sm"
            />
          </div>
          <div className="flex items-center gap-2">
            <div className="flex rounded-lg border border-white/10 bg-[#141824] p-0.5">
              <button
                onClick={() => setViewMode("grid")}
                className={`rounded-md p-1.5 ${viewMode === "grid" ? "bg-white/10 text-white" : "text-slate-400"}`}
              >
                <LayoutGrid className="size-3.5" />
              </button>
              <button
                onClick={() => setViewMode("list")}
                className={`rounded-md p-1.5 ${viewMode === "list" ? "bg-white/10 text-white" : "text-slate-400"}`}
              >
                <List className="size-3.5" />
              </button>
            </div>
            <Button
              variant="outline"
              size="sm"
              className="border-white/10 bg-[#141824] text-slate-200 hover:bg-white/[0.06]"
              onClick={onOpenImport}
            >
              <FolderInput className="mr-1.5 size-4" /> Import SSH Config
            </Button>
            <Button
              size="sm"
              className="bg-emerald-600 text-white hover:bg-emerald-500"
              onClick={onOpenNewHost}
            >
              <Plus className="mr-1.5 size-4" /> NEW HOST
            </Button>
          </div>
        </div>

        {/* Termius-style Group Pills */}
        <div className="mb-5 flex flex-wrap gap-1.5">
          {allGroups.map((grp) => (
            <button
              key={grp}
              onClick={() => setActiveGroup(grp)}
              className={`rounded-lg px-3 py-1 text-xs font-medium transition ${
                activeGroup === grp
                  ? "bg-indigo-500/20 text-indigo-300 ring-1 ring-indigo-500/40"
                  : "bg-white/[0.04] text-slate-400 hover:bg-white/[0.08] hover:text-slate-200"
              }`}
            >
              {grp === "all" ? "All Hosts" : grp}
            </button>
          ))}
        </div>

        {filteredServers.length === 0 ? (
          <Card className="border-white/[0.07] bg-[#141824]">
            <CardContent className="flex flex-col items-center justify-center py-16 text-center">
              <Server className="mb-3 size-10 text-slate-600" />
              <div className="text-sm font-medium text-slate-300">No Hosts in Vault</div>
              <p className="mt-1 max-w-sm text-xs text-slate-500">
                Click &quot;Import SSH Config&quot; to parse ~/.ssh/config &amp; ProxyJump chains, or click &quot;NEW HOST&quot;.
              </p>
            </CardContent>
          </Card>
        ) : (
          <div
            className={
              viewMode === "grid"
                ? "grid grid-cols-1 gap-3 lg:grid-cols-2 xl:grid-cols-3"
                : "space-y-2"
            }
          >
            {filteredServers.map((s) => {
              const jump = s.jump_host_id ? serverMap.get(s.jump_host_id) : null;
              const cred = s.credential_id ? credMap.get(s.credential_id) : null;
              const isSelected = selectedHostId === s.id;
              return (
                <Card
                  key={s.id}
                  onClick={() => onSelectHost(s.id)}
                  className={`group cursor-pointer border transition ${
                    isSelected
                      ? "border-indigo-500/60 bg-[#1A2030] ring-1 ring-indigo-500/30"
                      : "border-white/[0.07] bg-[#141824] hover:border-white/20 hover:bg-[#181D2B]"
                  }`}
                >
                  <CardHeader className="pb-2">
                    <div className="flex items-start justify-between gap-2">
                      <div className="flex items-center gap-2.5">
                        <div className="flex size-8 items-center justify-center rounded-lg bg-emerald-500/15 text-emerald-400">
                          <Terminal className="size-4" />
                        </div>
                        <div>
                          <CardTitle className="text-sm font-semibold text-white">{s.name}</CardTitle>
                          <CardDescription className="font-mono text-xs text-slate-400">
                            {s.username}@{s.host}:{s.port}
                          </CardDescription>
                        </div>
                      </div>
                      <span
                        className={`size-2 rounded-full ${
                          s.enabled ? "bg-emerald-400" : "bg-slate-600"
                        }`}
                      />
                    </div>
                  </CardHeader>
                  <CardContent className="flex flex-wrap items-center gap-1.5 pt-1">
                    {s.group_name && (
                      <Badge variant="outline" className="border-indigo-500/30 bg-indigo-500/10 text-[10px] text-indigo-300">
                        {s.group_name}
                      </Badge>
                    )}
                    {jump && (
                      <Badge
                        variant="outline"
                        className="gap-1 border-cyan-500/30 bg-cyan-500/10 text-[10px] text-cyan-300"
                      >
                        <GitBranch className="size-3" /> via {jump.name}
                      </Badge>
                    )}
                    {cred && (
                      <Badge
                        variant="outline"
                        className="gap-1 border-emerald-500/30 bg-emerald-500/10 text-[10px] text-emerald-300"
                      >
                        <KeyRound className="size-3" /> {cred.label}
                      </Badge>
                    )}
                  </CardContent>
                </Card>
              );
            })}
          </div>
        )}
      </div>

      {/* Termius-style Right Host Properties Drawer */}
      {selectedHost && (
        <aside className="w-80 shrink-0 overflow-y-auto border-l border-white/[0.07] bg-[#10131C] p-5">
          <div className="flex items-center justify-between">
            <h3 className="text-base font-semibold text-white">{selectedHost.name}</h3>
            <Switch
              checked={selectedHost.enabled}
              onCheckedChange={(checked) => onToggleEnabled(selectedHost, checked)}
            />
          </div>
          <p className="mt-0.5 font-mono text-xs text-slate-400">
            {selectedHost.username}@{selectedHost.host}:{selectedHost.port}
          </p>

          <Separator className="my-4 bg-white/[0.07]" />

          <div className="space-y-3 text-xs">
            <div>
              <span className="text-slate-500">Asset ID (MCP Target)</span>
              <div className="mt-1 rounded bg-[#161B28] px-2.5 py-1.5 font-mono text-slate-200">
                {selectedHost.id}
              </div>
            </div>
            <div>
              <span className="text-slate-500">Group</span>
              <div className="mt-1 text-slate-200">{selectedHost.group_name || "Ungrouped"}</div>
            </div>
            <div>
              <span className="text-slate-500">Keychain Credential</span>
              <div className="mt-1 text-slate-200">
                {selectedHost.credential_id
                  ? credMap.get(selectedHost.credential_id)?.label ?? selectedHost.credential_id
                  : "SSH Agent / Default Key"}
              </div>
            </div>
            {selectedHost.identity_file && (
              <div>
                <span className="text-slate-500">Identity File</span>
                <div className="mt-1 truncate font-mono text-slate-300">
                  {selectedHost.identity_file}
                </div>
              </div>
            )}
            {selectedHost.jump_host_id && (
              <div>
                <span className="text-slate-500">ProxyJump Chain</span>
                <div className="mt-1 flex items-center gap-1.5 text-cyan-300">
                  <GitBranch className="size-3.5" />
                  {serverMap.get(selectedHost.jump_host_id)?.name ?? selectedHost.jump_host_id}
                  {" -> "}
                  {selectedHost.name}
                </div>
              </div>
            )}
          </div>

          <div className="mt-6 flex flex-col gap-2">
            <Button
              size="sm"
              className="w-full bg-emerald-600 text-white hover:bg-emerald-500"
              onClick={() => onTestHost(selectedHost)}
            >
              <Wifi className="mr-1.5 size-4" /> Test Connection
            </Button>
            <Button
              size="sm"
              variant="outline"
              className="w-full border-white/10 bg-[#161B28]"
              onClick={() => onOpenEditHost(selectedHost)}
            >
              Edit Host Properties
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="w-full text-rose-400 hover:bg-rose-500/10 hover:text-rose-300"
              onClick={() => onDeleteHost(selectedHost.id)}
            >
              <Trash2 className="mr-1.5 size-4" /> Remove Host
            </Button>
          </div>
        </aside>
      )}
    </div>
  );
}
