import { useCallback, useEffect, useState } from "react";
import { Check, Plus, Trash2, X } from "lucide-react";
import { cn } from "cn";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ErrorText } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { HostGrant, User, UserGroup } from "@/types";

export default function AdminAccess() {
  const { t } = useI18n();
  const [grants, setGrants] = useState<HostGrant[]>([]);
  const [groups, setGroups] = useState<UserGroup[]>([]);
  const [serverGroups, setServerGroups] = useState<string[]>([]);
  const [error, setError] = useState<string>();
  const [newGroup, setNewGroup] = useState("");
  const [selectedGroupId, setSelectedGroupId] = useState<string>();
  const [members, setMembers] = useState<User[]>([]);
  const [users, setUsers] = useState<User[]>([]);
  const [addUserId, setAddUserId] = useState("");
  const [form, setForm] = useState<{
    user_group_id: string;
    group_name: string;
    can_connect: boolean;
    can_execute: boolean;
  } | null>(null);

  const load = useCallback(async () => {
    try {
      setGrants(await api.get<HostGrant[]>("/api/admin/access-grants"));
      setGroups(await api.get<UserGroup[]>("/api/admin/groups"));
      setServerGroups(await api.get<string[]>("/api/servers/groups"));
      setUsers(await api.get<User[]>("/api/admin/users"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (!selectedGroupId) {
      setMembers([]);
      return;
    }
    api
      .get<User[]>(`/api/admin/groups/${selectedGroupId}/members`)
      .then(setMembers)
      .catch(() => setMembers([]));
    setAddUserId("");
  }, [selectedGroupId]);

  const selectedGroup = groups.find((g) => g.id === selectedGroupId);
  const addableUsers = users.filter((u) => !members.some((m) => m.id === u.id));

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
    try {
      await api.del(`/api/admin/groups/${g.id}`);
      if (selectedGroupId === g.id) setSelectedGroupId(undefined);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const addMember = async () => {
    if (!selectedGroupId || !addUserId) return;
    try {
      await api.post(`/api/admin/groups/${selectedGroupId}/members`, { user_id: addUserId });
      setMembers(await api.get<User[]>(`/api/admin/groups/${selectedGroupId}/members`));
      setAddUserId("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "add failed");
    }
  };

  const removeMember = async (u: User) => {
    if (!selectedGroupId) return;
    try {
      await api.del(`/api/admin/groups/${selectedGroupId}/members/${u.id}`);
      setMembers(members.filter((m) => m.id !== u.id));
    } catch (err) {
      setError(err instanceof Error ? err.message : "remove failed");
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
    try {
      await api.del(`/api/admin/access-grants/${id}`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  return (
    <div className="flex flex-col gap-5">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold tracking-tight">{t("admin.access.title")}</h2>
        <Button
          size="sm"
          onClick={() =>
            setForm({
              user_group_id: groups[0]?.id ?? "",
              group_name: serverGroups[0] ?? "",
              can_connect: true,
              can_execute: false,
            })
          }
          disabled={groups.length === 0 || serverGroups.length === 0}
        >
          <Plus data-icon="inline-start" />
          {t("admin.access.add")}
        </Button>
      </div>

      <ErrorText>{error}</ErrorText>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">{t("admin.access.groupsTitle")}</CardTitle>
          <CardDescription className="text-xs">{t("admin.access.groupsHint")}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-1.5">
            {groups.map((g) => (
              <div
                key={g.id}
                className={cn(
                  "flex items-center gap-1 rounded-full border py-0.5 pr-1 pl-2.5 text-xs transition-colors",
                  selectedGroupId === g.id
                    ? "border-primary bg-primary text-primary-foreground"
                    : "border-border bg-secondary/40 text-foreground hover:bg-secondary"
                )}
              >
                <button
                  type="button"
                  onClick={() => setSelectedGroupId(selectedGroupId === g.id ? undefined : g.id)}
                  className="font-medium"
                >
                  {g.name}
                </button>
                <button
                  type="button"
                  className={cn(
                    "rounded-full p-0.5 transition-colors",
                    selectedGroupId === g.id
                      ? "text-primary-foreground/70 hover:text-primary-foreground"
                      : "text-muted-foreground hover:text-foreground"
                  )}
                  onClick={() => void removeGroup(g)}
                >
                  <X className="size-3" />
                </button>
              </div>
            ))}

            <div className="flex items-center gap-1.5">
              <Input
                className="h-7 w-44 text-xs"
                placeholder={t("admin.access.phGroupName")}
                value={newGroup}
                onChange={(e) => setNewGroup(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && void createGroup()}
              />
              <Button
                variant="outline"
                size="icon-xs"
                onClick={() => void createGroup()}
                disabled={!newGroup.trim()}
              >
                <Plus />
              </Button>
            </div>
          </div>

          {selectedGroup && (
            <div className="flex flex-wrap items-center gap-2 rounded-lg border border-border bg-secondary/30 p-3">
              <span className="font-mono text-xs text-muted-foreground">
                {selectedGroup.name} · {t("admin.access.membersTitle")}
              </span>
              {members.map((m) => (
                <Badge key={m.id} variant="outline" className="gap-1 bg-card pr-1 font-normal">
                  {m.username}
                  <button
                    type="button"
                    className="rounded-full p-0.5 text-muted-foreground hover:text-foreground"
                    onClick={() => void removeMember(m)}
                  >
                    <X className="size-3" />
                  </button>
                </Badge>
              ))}
              {members.length === 0 && (
                <span className="text-xs text-muted-foreground">{t("admin.access.noMembers")}</span>
              )}
              <div className="ml-auto flex items-center gap-1.5">
                <Select value={addUserId} onValueChange={(v) => setAddUserId(v ?? "")}>
                  <SelectTrigger size="sm" className="w-40">
                    <SelectValue placeholder={t("admin.access.selectUser")} />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {addableUsers.map((u) => (
                        <SelectItem key={u.id} value={u.id}>
                          {u.username}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
                <Button variant="outline" size="xs" onClick={() => void addMember()} disabled={!addUserId}>
                  <Plus data-icon="inline-start" />
                  {t("admin.access.addMember")}
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

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
                  {g.can_connect ? (
                    <Check className="size-4 text-geist-success" />
                  ) : (
                    <span className="text-muted-foreground">—</span>
                  )}
                </TableCell>
                <TableCell className="px-4">
                  {g.can_execute ? (
                    <Check className="size-4 text-geist-success" />
                  ) : (
                    <span className="text-muted-foreground">—</span>
                  )}
                </TableCell>
                <TableCell className="px-4 font-mono text-xs text-muted-foreground">
                  {g.created_by}
                </TableCell>
                <TableCell className="px-4 text-right">
                  <Button variant="destructive" size="icon-xs" onClick={() => void remove(g.id)}>
                    <Trash2 />
                  </Button>
                </TableCell>
              </TableRow>
            ))}
            {grants.length === 0 && (
              <TableRow>
                <TableCell colSpan={6} className="px-4 py-10 text-center text-sm text-muted-foreground">
                  {t("admin.access.empty")}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </Card>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("admin.access.add")}</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <Select
              value={form?.user_group_id ?? ""}
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
              value={form?.group_name ?? ""}
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
    </div>
  );
}
