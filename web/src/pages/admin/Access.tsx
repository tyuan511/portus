import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { HostGrant, ServerAsset, UserGroup } from "@/types";

// Access = user groups × machine groups. Roles decide what a user *can* do;
// these grants decide which machine groups they can do it on.
export default function AdminAccess() {
  const { t } = useI18n();
  const [grants, setGrants] = useState<HostGrant[]>([]);
  const [groups, setGroups] = useState<UserGroup[]>([]);
  const [servers, setServers] = useState<ServerAsset[]>([]);
  const [serverGroups, setServerGroups] = useState<string[]>([]);
  const [error, setError] = useState<string>();
  const [newGroup, setNewGroup] = useState("");
  const [form, setForm] = useState<{ user_group_id: string; group_name: string; can_connect: boolean; can_execute: boolean } | null>(null);

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

  const createGroup = async () => {
    if (!newGroup.trim()) return;
    try {
      await api.post("/api/admin/groups", { name: newGroup.trim() });
      setNewGroup("");
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "create failed");
    }
  };

  const removeGroup = async (g: UserGroup) => {
    if (!confirm(t("admin.access.deleteGroupConfirm", { name: g.name }))) return;
    try { await api.del(`/api/admin/groups/${g.id}`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const create = async () => {
    if (!form) return;
    try {
      await api.post("/api/admin/access-grants", {
        user_group_id: form.user_group_id,
        group_name: form.group_name,
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
    if (!confirm(t("admin.access.revokeConfirm"))) return;
    try { await api.del(`/api/admin/access-grants/${id}`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  return (
    <Page
      header={
        <PageHeader
          title={t("admin.access.title")}
          actions={<Button size="sm" onClick={() => setForm({ user_group_id: groups[0]?.id ?? "", group_name: serverGroups[0] ?? "", can_connect: true, can_execute: false })} disabled={groups.length === 0 || serverGroups.length === 0}><Plus className="size-3.5" /> {t("admin.access.add")}</Button>}
        />
      }
    >
      <ErrorText>{error}</ErrorText>

      <div className="mb-4">
        <div className="mb-1.5 text-xs font-medium uppercase text-muted-foreground">{t("admin.access.groupsTitle")}</div>
        <div className="flex flex-wrap items-center gap-1.5">
          {groups.map((g) => (
            <Badge key={g.id} variant="outline" className="gap-1 pr-1">
              {g.name}
              <button type="button" className="rounded-full p-0.5 text-muted-foreground hover:text-foreground" onClick={() => void removeGroup(g)}>
                <X className="size-3" />
              </button>
            </Badge>
          ))}
          <div className="flex items-center gap-1">
            <Input className="h-7 w-36 text-xs" placeholder={t("admin.access.phGroupName")} value={newGroup}
              onChange={(e) => setNewGroup(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void createGroup()} />
            <Button variant="outline" size="icon-xs" onClick={() => void createGroup()} disabled={!newGroup.trim()}>
              <Plus className="size-3" />
            </Button>
          </div>
        </div>
        <p className="mt-1.5 text-xs text-muted-foreground">{t("admin.access.groupsHint")}</p>
      </div>

      <div className="overflow-hidden rounded-xl border">
        <table className="w-full text-sm">
          <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
            <tr><th className="px-3 py-2">{t("admin.access.colGroup")}</th><th className="px-3 py-2">{t("admin.access.colTarget")}</th><th className="px-3 py-2">{t("admin.access.colConnect")}</th><th className="px-3 py-2">{t("admin.access.colExecute")}</th><th className="px-3 py-2">{t("admin.access.colBy")}</th><th className="px-3 py-2" /></tr>
          </thead>
          <tbody>
            {grants.map((g) => (
              <tr key={g.id} className="border-b last:border-0">
                <td className="px-3 py-1.5 font-medium">{g.user_group?.name ?? g.user_group_id}</td>
                <td className="px-3 py-1.5"><Badge variant="secondary">{g.group_name}</Badge></td>
                <td className="px-3 py-1.5">{g.can_connect ? "✔" : "—"}</td>
                <td className="px-3 py-1.5">{g.can_execute ? "✔" : "—"}</td>
                <td className="px-3 py-1.5 text-xs text-muted-foreground">{g.created_by}</td>
                <td className="px-3 py-1.5 text-right">
                  <Button variant="destructive" size="icon-xs" onClick={() => void remove(g.id)}><Trash2 className="size-3" /></Button>
                </td>
              </tr>
            ))}
            {grants.length === 0 && <tr><td colSpan={6} className="px-3 py-8 text-center text-sm text-muted-foreground">{t("admin.access.empty")}</td></tr>}
          </tbody>
        </table>
      </div>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("admin.access.add")}</DialogTitle></DialogHeader>
          <div className="space-y-3">
            <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.user_group_id}
              onChange={(e) => setForm({ ...form!, user_group_id: e.target.value })}>
              <option value="">{t("admin.access.selectGroup")}</option>
              {groups.map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}
            </select>
            <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.group_name}
              onChange={(e) => setForm({ ...form!, group_name: e.target.value })}>
              <option value="">{t("admin.access.selectHostGroup")}</option>
              {serverGroups.map((g) => <option key={g} value={g}>{g}</option>)}
            </select>
            <div className="flex gap-4 text-sm">
              <label className="flex items-center gap-1.5">
                <input type="checkbox" checked={form?.can_connect} onChange={(e) => setForm({ ...form!, can_connect: e.target.checked })} /> {t("admin.access.allowConnect")}
              </label>
              <label className="flex items-center gap-1.5">
                <input type="checkbox" checked={form?.can_execute} onChange={(e) => setForm({ ...form!, can_execute: e.target.checked })} /> {t("admin.access.allowExecute")}
              </label>
            </div>
            <ErrorText>{error}</ErrorText>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>{t("common.cancel")}</Button>
            <Button onClick={() => void create()} disabled={!form?.user_group_id || !form?.group_name}>{t("admin.access.grantBtn")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
