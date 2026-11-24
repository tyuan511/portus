import { useCallback, useEffect, useState } from "react";
import { Pencil, Plus, Trash2, Users, X } from "lucide-react";
import { cn } from "cn";
import { ConfirmDialog } from "@/components/bulwark/ConfirmDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ErrorText } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { User, UserGroup } from "@/types";

export default function AdminUserGroups() {
  const { t } = useI18n();
  const [groups, setGroups] = useState<UserGroup[]>([]);
  const [users, setUsers] = useState<User[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [members, setMembers] = useState<User[]>([]);
  const [addUserId, setAddUserId] = useState("");
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<{ id?: string; name: string; description: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<UserGroup | null>(null);

  const load = useCallback(async () => {
    try {
      const [gs, us] = await Promise.all([
        api.get<UserGroup[]>("/api/admin/groups"),
        api.get<User[]>("/api/admin/users"),
      ]);
      setGroups(gs);
      setUsers(us);
      setSelectedId((cur) => (cur && gs.some((g) => g.id === cur) ? cur : gs[0]?.id));
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (!selectedId) {
      setMembers([]);
      return;
    }
    api
      .get<User[]>(`/api/admin/groups/${selectedId}/members`)
      .then(setMembers)
      .catch(() => setMembers([]));
    setAddUserId("");
  }, [selectedId]);

  const selected = groups.find((g) => g.id === selectedId);
  const addableUsers = users.filter((u) => !members.some((m) => m.id === u.id));

  const saveGroup = async () => {
    if (!form?.name.trim()) return;
    try {
      if (form.id) {
        await api.patch(`/api/admin/groups/${form.id}`, {
          name: form.name.trim(),
          description: form.description.trim(),
        });
      } else {
        const created = await api.post<UserGroup>("/api/admin/groups", {
          name: form.name.trim(),
          description: form.description.trim(),
        });
        setSelectedId(created.id);
      }
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "save failed");
    }
  };

  const removeGroup = async (g: UserGroup) => {
    try {
      await api.del(`/api/admin/groups/${g.id}`);
      setDeleteTarget(null);
      if (selectedId === g.id) setSelectedId(undefined);
      await load();
    } catch (err) {
      setDeleteTarget(null);
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const addMember = async () => {
    if (!selectedId || !addUserId) return;
    try {
      await api.post(`/api/admin/groups/${selectedId}/members`, { user_id: addUserId });
      setMembers(await api.get<User[]>(`/api/admin/groups/${selectedId}/members`));
      setAddUserId("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "add failed");
    }
  };

  const removeMember = async (u: User) => {
    if (!selectedId) return;
    try {
      await api.del(`/api/admin/groups/${selectedId}/members/${u.id}`);
      setMembers((prev) => prev.filter((m) => m.id !== u.id));
    } catch (err) {
      setError(err instanceof Error ? err.message : "remove failed");
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h2 className="text-sm font-semibold tracking-tight">{t("admin.groups.title")}</h2>
          <p className="text-xs text-muted-foreground">{t("admin.groups.sub")}</p>
        </div>
        <Button size="sm" onClick={() => setForm({ name: "", description: "" })}>
          <Plus data-icon="inline-start" />
          {t("admin.groups.add")}
        </Button>
      </div>

      <ErrorText>{error}</ErrorText>

      {groups.length === 0 ? (
        <Empty className="border border-border bg-card py-14">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Users />
            </EmptyMedia>
            <EmptyTitle>{t("admin.groups.title")}</EmptyTitle>
            <EmptyDescription>{t("admin.groups.empty")}</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button size="sm" onClick={() => setForm({ name: "", description: "" })}>
              <Plus data-icon="inline-start" />
              {t("admin.groups.add")}
            </Button>
          </EmptyContent>
        </Empty>
      ) : (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1.15fr_1fr]">
          {/* Groups Table */}
          <Card className="p-0">
            <Table>
              <TableHeader className="bg-secondary/50">
                <TableRow>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("admin.groups.colName")}
                  </TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("admin.groups.colDesc")}
                  </TableHead>
                  <TableHead className="px-4" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {groups.map((g) => {
                  const isSelected = g.id === selectedId;
                  return (
                    <TableRow
                      key={g.id}
                      onClick={() => setSelectedId(g.id)}
                      className={cn("cursor-pointer", isSelected && "bg-secondary/70")}
                    >
                      <TableCell className="px-4 font-medium">
                        <div className="flex items-center gap-2">
                          <span
                            className={cn(
                              "size-1.5 rounded-full",
                              isSelected ? "bg-geist-blue" : "bg-muted-foreground/30"
                            )}
                          />
                          <span>{g.name}</span>
                        </div>
                      </TableCell>
                      <TableCell className="max-w-52 truncate px-4 text-xs text-muted-foreground">
                        {g.description || "—"}
                      </TableCell>
                      <TableCell className="px-4 text-right">
                        <div
                          className="flex justify-end gap-1"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <Button
                            variant="ghost"
                            size="icon-xs"
                            onClick={() =>
                              setForm({ id: g.id, name: g.name, description: g.description ?? "" })
                            }
                          >
                            <Pencil />
                          </Button>
                          <Button
                            variant="destructive"
                            size="icon-xs"
                            onClick={() => setDeleteTarget(g)}
                          >
                            <Trash2 />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </Card>

          {/* Members Inspector */}
          <Card>
            {selected ? (
              <>
                <CardHeader className="border-b border-border/70 pb-3">
                  <div className="flex items-center justify-between gap-2">
                    <CardTitle className="text-sm">{selected.name}</CardTitle>
                    <Badge variant="secondary" className="font-mono text-[11px] tabular-nums">
                      {members.length} {t("admin.access.membersTitle")}
                    </Badge>
                  </div>
                  {selected.description && (
                    <CardDescription className="text-xs">{selected.description}</CardDescription>
                  )}
                </CardHeader>

                <CardContent className="flex flex-col gap-3">
                  <div className="flex gap-2">
                    <Select
                      value={addUserId || null}
                      items={Object.fromEntries(
                        addableUsers.map((u) => [
                          u.id,
                          u.display_name ? `${u.username} (${u.display_name})` : u.username,
                        ])
                      )}
                      onValueChange={(v) => setAddUserId(v ?? "")}
                    >
                      <SelectTrigger size="sm" className="flex-1">
                        <SelectValue placeholder={t("admin.access.selectUser")} />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          {addableUsers.map((u) => (
                            <SelectItem key={u.id} value={u.id}>
                              {u.username}
                              {u.display_name ? ` (${u.display_name})` : ""}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                    <Button size="sm" onClick={() => void addMember()} disabled={!addUserId}>
                      <Plus data-icon="inline-start" />
                      {t("admin.access.addMember")}
                    </Button>
                  </div>

                  {members.length === 0 ? (
                    <p className="rounded-lg border border-dashed border-border p-4 text-center text-xs text-muted-foreground">
                      {t("admin.access.noMembers")}
                    </p>
                  ) : (
                    <div className="flex flex-col divide-y divide-border overflow-hidden rounded-lg border border-border">
                      {members.map((m) => (
                        <div
                          key={m.id}
                          className="flex items-center justify-between gap-2 bg-background px-3 py-2"
                        >
                          <div className="min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="truncate text-xs font-medium">{m.username}</span>
                              <Badge variant="outline" className="font-mono text-[10px]">
                                {t(m.role === "admin" ? "role.admin" : "role.member")}
                              </Badge>
                            </div>
                            {m.display_name && (
                              <div className="truncate text-[11px] text-muted-foreground">
                                {m.display_name}
                              </div>
                            )}
                          </div>
                          <Button
                            variant="ghost"
                            size="icon-xs"
                            onClick={() => void removeMember(m)}
                          >
                            <X />
                          </Button>
                        </div>
                      ))}
                    </div>
                  )}
                </CardContent>
              </>
            ) : (
              <CardContent className="py-12 text-center text-xs text-muted-foreground">
                {t("admin.groups.selectPrompt")}
              </CardContent>
            )}
          </Card>
        </div>
      )}

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {form?.id ? t("admin.groups.edit") : t("admin.groups.add")}
            </DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>{t("common.name")}</Label>
              <Input
                placeholder={t("admin.access.phGroupName")}
                value={form?.name ?? ""}
                onChange={(e) => setForm({ ...form!, name: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("common.description")}</Label>
              <Input
                placeholder={t("admin.groups.phDesc")}
                value={form?.description ?? ""}
                onChange={(e) => setForm({ ...form!, description: e.target.value })}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void saveGroup()} disabled={!form?.name.trim()}>
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("common.delete")}
        description={t("admin.access.deleteGroupConfirm", { name: deleteTarget?.name ?? "" })}
        confirmLabel={t("common.delete")}
        onConfirm={() => deleteTarget && void removeGroup(deleteTarget)}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
      />
    </div>
  );
}
