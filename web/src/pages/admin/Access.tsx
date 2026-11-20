import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/api";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { HostGrant, ServerAsset, UserGroup } from "@/types";

export default function AdminAccess() {
  const [grants, setGrants] = useState<HostGrant[]>([]);
  const [groups, setGroups] = useState<UserGroup[]>([]);
  const [servers, setServers] = useState<ServerAsset[]>([]);
  const [serverGroups, setServerGroups] = useState<string[]>([]);
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<{
    user_group_id: string; target: "server" | "group"; server_id: string; group_name: string;
    can_connect: boolean; can_execute: boolean;
  } | null>(null);

  const load = useCallback(async () => {
    try {
      setGrants(await api.get<HostGrant[]>("/api/admin/access-grants"));
      setGroups(await api.get<UserGroup[]>("/api/admin/groups"));
      setServers(await api.get<ServerAsset[]>("/api/servers"));
      setServerGroups(await api.get<string[]>("/api/servers/groups"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const create = async () => {
    if (!form) return;
    try {
      await api.post("/api/admin/access-grants", {
        user_group_id: form.user_group_id,
        server_id: form.target === "server" ? form.server_id : "",
        group_name: form.target === "group" ? form.group_name : "",
        can_connect: form.can_connect,
        can_execute: form.can_execute,
      });
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "create failed");
    }
  };

  const remove = async (id: string) => {
    if (!confirm("Revoke this grant?")) return;
    try { await api.del(`/api/admin/access-grants/${id}`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  return (
    <Page
      header={
        <PageHeader
          title="Host Access Grants"
          subtitle="Grant hosts or host groups to user groups · members only see what is granted"
          actions={<Button size="sm" onClick={() => setForm({ user_group_id: "", target: "group", server_id: "", group_name: "", can_connect: true, can_execute: false })}><Plus className="size-3.5" /> New grant</Button>}
        />
      }
    >
      <ErrorText>{error}</ErrorText>
      <div className="overflow-hidden rounded-xl border">
        <table className="w-full text-sm">
          <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
            <tr><th className="px-3 py-2">User group</th><th className="px-3 py-2">Target</th><th className="px-3 py-2">Connect</th><th className="px-3 py-2">Execute</th><th className="px-3 py-2">By</th><th className="px-3 py-2" /></tr>
          </thead>
          <tbody>
            {grants.map((g) => (
              <tr key={g.id} className="border-b last:border-0">
                <td className="px-3 py-1.5 font-medium">{g.user_group?.name ?? g.user_group_id}</td>
                <td className="px-3 py-1.5">
                  {g.server_id
                    ? <Badge variant="outline">host: {servers.find((s) => s.id === g.server_id)?.name ?? g.server_id}</Badge>
                    : <Badge variant="secondary">group: {g.group_name}</Badge>}
                </td>
                <td className="px-3 py-1.5">{g.can_connect ? "✔" : "—"}</td>
                <td className="px-3 py-1.5">{g.can_execute ? "✔" : "—"}</td>
                <td className="px-3 py-1.5 text-xs text-muted-foreground">{g.created_by}</td>
                <td className="px-3 py-1.5 text-right">
                  <Button variant="destructive" size="icon-xs" onClick={() => void remove(g.id)}><Trash2 className="size-3" /></Button>
                </td>
              </tr>
            ))}
            {grants.length === 0 && <tr><td colSpan={6} className="px-3 py-8 text-center text-sm text-muted-foreground">No grants. Members cannot see any host until you grant access.</td></tr>}
          </tbody>
        </table>
      </div>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>New grant</DialogTitle></DialogHeader>
          <div className="space-y-3">
            <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.user_group_id}
              onChange={(e) => setForm({ ...form!, user_group_id: e.target.value })}>
              <option value="">User group…</option>
              {groups.map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}
            </select>
            <div className="flex gap-3 text-sm">
              <label className="flex items-center gap-1.5">
                <input type="radio" checked={form?.target === "group"} onChange={() => setForm({ ...form!, target: "group" })} /> Host group
              </label>
              <label className="flex items-center gap-1.5">
                <input type="radio" checked={form?.target === "server"} onChange={() => setForm({ ...form!, target: "server" })} /> Single host
              </label>
            </div>
            {form?.target === "group" ? (
              <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.group_name}
                onChange={(e) => setForm({ ...form!, group_name: e.target.value })}>
                <option value="">Host group…</option>
                {serverGroups.map((g) => <option key={g} value={g}>{g}</option>)}
              </select>
            ) : (
              <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.server_id}
                onChange={(e) => setForm({ ...form!, server_id: e.target.value })}>
                <option value="">Host…</option>
                {servers.map((s) => <option key={s.id} value={s.id}>{s.name} ({s.host})</option>)}
              </select>
            )}
            <div className="flex gap-4 text-sm">
              <label className="flex items-center gap-1.5">
                <input type="checkbox" checked={form?.can_connect} onChange={(e) => setForm({ ...form!, can_connect: e.target.checked })} /> Connect (terminal/test)
              </label>
              <label className="flex items-center gap-1.5">
                <input type="checkbox" checked={form?.can_execute} onChange={(e) => setForm({ ...form!, can_execute: e.target.checked })} /> Execute commands (MCP)
              </label>
            </div>
            <ErrorText>{error}</ErrorText>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>Cancel</Button>
            <Button onClick={() => void create()} disabled={!form?.user_group_id}>Grant</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
