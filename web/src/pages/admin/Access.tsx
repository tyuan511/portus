import { useCallback, useEffect, useState } from "react";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import { ConfirmDialog } from "@/components/sshield/ConfirmDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ErrorText } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { HostGrant, UserGroup } from "@/types";

export default function AdminAccess() {
  const { t } = useI18n();
  const [grants, setGrants] = useState<HostGrant[]>([]);
  const [groups, setGroups] = useState<UserGroup[]>([]);
  const [serverGroups, setServerGroups] = useState<string[]>([]);
  const [error, setError] = useState<string>();
  const [deleteGrantId, setDeleteGrantId] = useState<string | null>(null);
  const [form, setForm] = useState<{
    user_group_id: string;
    group_name: string;
    can_connect: boolean;
    can_execute: boolean;
    can_transfer: boolean;
  } | null>(null);

  const load = useCallback(async () => {
    try {
      const [gr, gs, sg] = await Promise.all([
        api.get<HostGrant[]>("/api/admin/access-grants"),
        api.get<UserGroup[]>("/api/admin/groups"),
        api.get<string[]>("/api/servers/groups"),
      ]);
      setGrants(gr);
      setGroups(gs);
      setServerGroups(sg);
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const openCreate = () =>
    setForm({
      user_group_id: groups[0]?.id ?? "",
      group_name: serverGroups[0] ?? "",
      can_connect: true,
      can_execute: false,
      can_transfer: true,
    });

  const create = async () => {
    if (!form) return;
    try {
      await api.post("/api/admin/access-grants", {
        user_group_id: form.user_group_id,
        group_name: form.group_name,
        can_connect: form.can_connect,
        can_execute: form.can_execute,
        can_transfer: form.can_transfer,
      });
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "create failed");
    }
  };

  const toggleGrant = async (
    id: string,
    patch: { can_connect?: boolean; can_execute?: boolean; can_transfer?: boolean }
  ) => {
    try {
      await api.patch(`/api/admin/access-grants/${id}`, patch);
      setGrants((prev) => prev.map((g) => (g.id === id ? { ...g, ...patch } : g)));
    } catch (err) {
      setError(err instanceof Error ? err.message : "update failed");
    }
  };

  const remove = async (id: string) => {
    try {
      await api.del(`/api/admin/access-grants/${id}`);
      setDeleteGrantId(null);
      await load();
    } catch (err) {
      setDeleteGrantId(null);
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold tracking-tight">{t("admin.access.title")}</h2>
        <Button
          size="sm"
          onClick={openCreate}
          disabled={groups.length === 0 || serverGroups.length === 0}
        >
          <Plus data-icon="inline-start" />
          {t("admin.access.add")}
        </Button>
      </div>

      <ErrorText>{error}</ErrorText>

      {grants.length === 0 ? (
        <Empty className="border border-border bg-card py-14">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <KeyRound />
            </EmptyMedia>
            <EmptyTitle>{t("admin.access.title")}</EmptyTitle>
            <EmptyDescription>{t("admin.access.empty")}</EmptyDescription>
          </EmptyHeader>
          {groups.length > 0 && serverGroups.length > 0 && (
            <EmptyContent>
              <Button size="sm" onClick={openCreate}>
                <Plus data-icon="inline-start" />
                {t("admin.access.add")}
              </Button>
            </EmptyContent>
          )}
        </Empty>
      ) : (
        <Card className="p-0">
          <Table>
            <TableHeader className="bg-secondary/50">
              <TableRow>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                  {t("admin.access.colGroup")}
                </TableHead>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                  {t("admin.access.colTarget")}
                </TableHead>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                  {t("admin.access.colConnect")}
                </TableHead>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                  {t("admin.access.colExecute")}
                </TableHead>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                  {t("admin.access.colTransfer")}
                </TableHead>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                  {t("admin.access.colBy")}
                </TableHead>
                <TableHead className="px-4" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {grants.map((g) => (
                <TableRow key={g.id}>
                  <TableCell className="px-4 font-medium">
                    {g.user_group?.name ?? g.user_group_id}
                  </TableCell>
                  <TableCell className="px-4">
                    <Badge variant="secondary" className="font-mono text-xs">
                      {g.group_name}
                    </Badge>
                  </TableCell>
                  <TableCell className="px-4">
                    <Switch
                      checked={g.can_connect}
                      onCheckedChange={(v) => void toggleGrant(g.id, { can_connect: v })}
                    />
                  </TableCell>
                  <TableCell className="px-4">
                    <Switch
                      checked={g.can_execute}
                      onCheckedChange={(v) => void toggleGrant(g.id, { can_execute: v })}
                    />
                  </TableCell>
                  <TableCell className="px-4">
                    <Switch
                      checked={g.can_transfer}
                      onCheckedChange={(v) => void toggleGrant(g.id, { can_transfer: v })}
                    />
                  </TableCell>
                  <TableCell className="px-4 font-mono text-xs text-muted-foreground">
                    {g.created_by}
                  </TableCell>
                  <TableCell className="px-4 text-right">
                    <Button variant="destructive" size="icon-xs" onClick={() => setDeleteGrantId(g.id)}>
                      <Trash2 />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("admin.access.add")}</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <Select
              value={form?.user_group_id || null}
              items={Object.fromEntries(groups.map((g) => [g.id, g.name]))}
              onValueChange={(v) => v && setForm({ ...form!, user_group_id: v })}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder={t("admin.access.selectGroup")} />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {groups.map((g) => (
                    <SelectItem key={g.id} value={g.id}>
                      {g.name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>

            <Select
              value={form?.group_name || null}
              items={Object.fromEntries(serverGroups.map((g) => [g, g]))}
              onValueChange={(v) => v && setForm({ ...form!, group_name: v })}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder={t("admin.access.selectHostGroup")} />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {serverGroups.map((g) => (
                    <SelectItem key={g} value={g}>
                      {g}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>

            <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
              <div className="flex items-center justify-between text-sm">
                <span>{t("admin.access.allowConnect")}</span>
                <Switch
                  checked={form?.can_connect ?? false}
                  onCheckedChange={(v) => setForm({ ...form!, can_connect: v })}
                />
              </div>
              <div className="flex items-center justify-between text-sm">
                <span>{t("admin.access.allowExecute")}</span>
                <Switch
                  checked={form?.can_execute ?? false}
                  onCheckedChange={(v) => setForm({ ...form!, can_execute: v })}
                />
              </div>
              <div className="flex items-center justify-between text-sm">
                <span>{t("admin.access.allowTransfer")}</span>
                <Switch
                  checked={form?.can_transfer ?? false}
                  onCheckedChange={(v) => setForm({ ...form!, can_transfer: v })}
                />
              </div>
            </div>

            <ErrorText>{error}</ErrorText>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={() => void create()}
              disabled={!form?.user_group_id || !form?.group_name}
            >
              {t("admin.access.grantBtn")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={deleteGrantId !== null}
        title={t("common.delete")}
        description={t("admin.access.revokeConfirm")}
        confirmLabel={t("common.delete")}
        onConfirm={() => deleteGrantId && void remove(deleteGrantId)}
        onOpenChange={(o) => !o && setDeleteGrantId(null)}
      />
    </div>
  );
}
