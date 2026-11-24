import { useCallback, useEffect, useMemo, useState } from "react";
import { KeyRound, Pencil, Plus, ShieldX, Trash2, UserCheck } from "lucide-react";
import { ConfirmDialog } from "@/components/bulwark/ConfirmDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ErrorText } from "@/components/ui/shared";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import type { HostGrant, Role, ServerAsset, User, UserGroup, UserGroupMembership } from "@/types";

const ROLES: Role[] = ["admin", "member"];

export default function AdminUsers() {
  const auth = useAuth();
  const { t } = useI18n();
  const canManageAccess = auth.hasPermission("access.manage");
  const [users, setUsers] = useState<User[]>([]);
  const [groups, setGroups] = useState<UserGroup[]>([]);
  const [memberships, setMemberships] = useState<UserGroupMembership[]>([]);
  const [grants, setGrants] = useState<HostGrant[]>([]);
  const [servers, setServers] = useState<ServerAsset[]>([]);
  const [error, setError] = useState<string>();
  const [search, setSearch] = useState("");
  const [form, setForm] = useState<{
    username: string;
    display_name: string;
    password: string;
    role: Role;
    group_ids: string[];
  } | null>(null);
  const [edit, setEdit] = useState<{
    user: User;
    display_name: string;
    role: Role;
    is_active: boolean;
    group_ids: string[];
  } | null>(null);
  const [confirmAction, setConfirmAction] = useState<{
    kind: "delete" | "resetTotp";
    user: User;
  } | null>(null);
  const [tempPassword, setTempPassword] = useState<{ username: string; password: string } | null>(null);

  const q = search.trim().toLowerCase();
  const filtered = users.filter(
    (u) =>
      !q ||
      u.username.toLowerCase().includes(q) ||
      u.display_name.toLowerCase().includes(q)
  );

  const load = useCallback(async () => {
    try {
      const us = await api.get<User[]>("/api/admin/users");
      setUsers(us);
      if (canManageAccess) {
        const [gs, ms, grs, srvs] = await Promise.all([
          api.get<UserGroup[]>("/api/admin/groups"),
          api.get<UserGroupMembership[]>("/api/admin/group-memberships"),
          api.get<HostGrant[]>("/api/admin/access-grants"),
          api.get<ServerAsset[]>("/api/servers"),
        ]);
        setGroups(gs);
        setMemberships(ms);
        setGrants(grs);
        setServers(srvs);
      }
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, [canManageAccess]);

  useEffect(() => {
    void load();
  }, [load]);

  const groupById = useMemo(() => {
    const map = new Map<string, UserGroup>();
    for (const g of groups) map.set(g.id, g);
    return map;
  }, [groups]);

  const serverById = useMemo(() => {
    const map = new Map<string, ServerAsset>();
    for (const s of servers) map.set(s.id, s);
    return map;
  }, [servers]);

  const groupIdsForUser = useCallback(
    (userId: string) => memberships.filter((m) => m.user_id === userId).map((m) => m.group_id),
    [memberships]
  );

  const effectiveHosts = useMemo(() => {
    if (!edit || edit.role === "admin") return [];
    const selectedGroups = new Set(edit.group_ids);
    const byServer = new Map<
      string,
      {
        name: string;
        connect: boolean;
        terminalNoApproval: boolean;
        transfer: boolean;
        execute: boolean;
        mcpNoApproval: boolean;
      }
    >();
    for (const g of grants) {
      if (!selectedGroups.has(g.user_group_id)) continue;
      if (!g.can_connect && !g.can_transfer && !g.can_execute) continue;
      const srv = serverById.get(g.server_id);
      const cur = byServer.get(g.server_id) ?? {
        name: srv?.name ?? g.server_name ?? g.server_id,
        connect: false,
        terminalNoApproval: false,
        transfer: false,
        execute: false,
        mcpNoApproval: false,
      };
      cur.connect = cur.connect || g.can_connect;
      cur.terminalNoApproval =
        cur.terminalNoApproval || (g.can_connect && Boolean(g.terminal_no_approval));
      cur.transfer = cur.transfer || g.can_transfer;
      cur.execute = cur.execute || g.can_execute;
      cur.mcpNoApproval = cur.mcpNoApproval || (g.can_execute && Boolean(g.mcp_no_approval));
      byServer.set(g.server_id, cur);
    }
    return Array.from(byServer.entries())
      .map(([id, info]) => ({ id, ...info }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [edit, grants, serverById]);

  const create = async () => {
    if (!form) return;
    try {
      await api.post("/api/admin/users", {
        username: form.username,
        display_name: form.display_name,
        password: form.password,
        role: form.role,
        group_ids: form.role === "member" ? form.group_ids : [],
      });
      setForm(null);
      await load();
      toast.success(t("common.saved"));
    } catch (err) {
      const msg = err instanceof Error ? err.message : "create failed";
      setError(msg);
      toast.error(msg);
    }
  };

  const patch = async (id: string, body: Record<string, unknown>) => {
    try {
      await api.patch(`/api/admin/users/${id}`, body);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "update failed");
    }
  };

  const saveEdit = async () => {
    if (!edit) return;
    try {
      await api.patch(`/api/admin/users/${edit.user.id}`, {
        display_name: edit.display_name,
        role: edit.role,
        is_active: edit.is_active,
      });
      if (canManageAccess) {
        await api.put(`/api/admin/users/${edit.user.id}/groups`, {
          group_ids: edit.role === "member" ? edit.group_ids : [],
        });
      }
      setEdit(null);
      await load();
      toast.success(t("common.saved"));
    } catch (err) {
      const msg = err instanceof Error ? err.message : "update failed";
      setError(msg);
      toast.error(msg);
    }
  };

  const remove = async (u: User) => {
    try {
      await api.del(`/api/admin/users/${u.id}`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const resetPassword = async (u: User, newPassword = "") => {
    try {
      const res = await api.post<{ temporary_password: string }>(
        `/api/admin/users/${u.id}/reset-password`,
        { new_password: newPassword }
      );
      setTempPassword({ username: u.username, password: res.temporary_password });
    } catch (err) {
      setError(err instanceof Error ? err.message : "reset failed");
    }
  };

  const resetTotp = async (u: User) => {
    try {
      await api.post(`/api/admin/users/${u.id}/reset-totp`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "reset failed");
    }
  };

  const runConfirm = async () => {
    if (!confirmAction) return;
    const action = confirmAction;
    setConfirmAction(null);
    if (action.kind === "delete") {
      await remove(action.user);
    } else {
      await resetTotp(action.user);
    }
  };

  const toggleGroupId = (list: string[], id: string) =>
    list.includes(id) ? list.filter((x) => x !== id) : [...list, id];

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold tracking-tight">{t("admin.users.title")}</h2>
        <div className="flex items-center gap-2">
          <Input
            placeholder={t("users.search")}
            className="w-56"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <Button
            size="sm"
            onClick={() =>
              setForm({ username: "", display_name: "", password: "", role: "member", group_ids: [] })
            }
          >
            <Plus data-icon="inline-start" />
            {t("admin.users.add")}
          </Button>
        </div>
      </div>

      <ErrorText>{error}</ErrorText>

      <Card className="p-0">
        <Table>
          <TableHeader className="bg-secondary/50">
            <TableRow>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.colUser")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.colRole")}
              </TableHead>
              {canManageAccess && (
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                  {t("admin.users.colGroups")}
                </TableHead>
              )}
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.col2fa")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.colActive")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.colLastLogin")}
              </TableHead>
              <TableHead className="px-4" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {filtered.map((u) => {
              const userGroups = groupIdsForUser(u.id)
                .map((gid) => groupById.get(gid))
                .filter((g): g is UserGroup => Boolean(g));
              return (
                <TableRow key={u.id}>
                  <TableCell className="px-4">
                    <div className="flex items-center gap-1.5">
                      <span className="font-medium">{u.username}</span>
                      {u.must_change_password && (
                        <Badge variant="destructive">{t("users.badgeMustChangePwd")}</Badge>
                      )}
                    </div>
                    {u.display_name && (
                      <div className="text-xs text-muted-foreground">{u.display_name}</div>
                    )}
                  </TableCell>
                  <TableCell className="px-4">
                    <Select
                      value={u.role}
                      items={{
                        admin: t("role.admin"),
                        member: t("role.member"),
                      }}
                      onValueChange={(v) => v && void patch(u.id, { role: v })}
                    >
                      <SelectTrigger size="sm" className="w-32">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          {ROLES.map((r) => (
                            <SelectItem key={r} value={r}>
                              {t(r === "admin" ? "role.admin" : "role.member")}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  </TableCell>
                  {canManageAccess && (
                    <TableCell className="px-4">
                      {u.role === "admin" ? (
                        <Badge variant="outline" className="text-[11px] text-muted-foreground">
                          {t("admin.users.allAccessAdmin")}
                        </Badge>
                      ) : userGroups.length === 0 ? (
                        <span className="text-xs text-muted-foreground">
                          {t("admin.users.noGroups")}
                        </span>
                      ) : (
                        <div className="flex flex-wrap gap-1">
                          {userGroups.map((g) => (
                            <Badge key={g.id} variant="secondary" className="text-[11px]">
                              {g.name}
                            </Badge>
                          ))}
                        </div>
                      )}
                    </TableCell>
                  )}
                  <TableCell className="px-4">
                    {u.totp_enabled ? (
                      <Badge>{t("common.on")}</Badge>
                    ) : (
                      <Badge variant="secondary">{t("common.off")}</Badge>
                    )}
                  </TableCell>
                  <TableCell className="px-4">
                    <Switch
                      checked={u.is_active}
                      onCheckedChange={(v) => void patch(u.id, { is_active: v })}
                    />
                  </TableCell>
                  <TableCell className="px-4 font-mono text-xs text-muted-foreground tabular-nums">
                    {u.last_login_at ? new Date(u.last_login_at).toLocaleString() : t("common.never")}
                  </TableCell>
                  <TableCell className="px-4 text-right">
                    <div className="flex justify-end gap-1.5">
                      <Button variant="outline" size="xs" onClick={() => void resetPassword(u)}>
                        <KeyRound data-icon="inline-start" />
                        {t("admin.users.resetPwd")}
                      </Button>
                      {u.totp_enabled && (
                        <Button
                          variant="outline"
                          size="xs"
                          onClick={() => setConfirmAction({ kind: "resetTotp", user: u })}
                        >
                          <ShieldX data-icon="inline-start" />
                          {t("admin.users.resetTotp")}
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        onClick={() =>
                          setEdit({
                            user: u,
                            display_name: u.display_name,
                            role: u.role,
                            is_active: u.is_active,
                            group_ids: groupIdsForUser(u.id),
                          })
                        }
                      >
                        <Pencil />
                      </Button>
                      <Button
                        variant="destructive"
                        size="icon-xs"
                        onClick={() => setConfirmAction({ kind: "delete", user: u })}
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

      {/* Create User Dialog */}
      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("admin.users.add")}</DialogTitle>
          </DialogHeader>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>{t("admin.users.fUsername")}</Label>
              <Input
                value={form?.username ?? ""}
                onChange={(e) => setForm({ ...form!, username: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("admin.users.fDisplay")}</Label>
              <Input
                value={form?.display_name ?? ""}
                onChange={(e) => setForm({ ...form!, display_name: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("admin.users.fInitialPwd")}</Label>
              <Input
                type="password"
                value={form?.password ?? ""}
                onChange={(e) => setForm({ ...form!, password: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("admin.users.fRole")}</Label>
              <Select
                value={form?.role ?? "member"}
                items={{
                  admin: t("role.admin"),
                  member: t("role.member"),
                }}
                onValueChange={(v) => v && setForm({ ...form!, role: v as Role })}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {ROLES.map((r) => (
                      <SelectItem key={r} value={r}>
                        {t(r === "admin" ? "role.admin" : "role.member")}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            {canManageAccess && form && (
              <div className="col-span-2 flex flex-col gap-1.5">
                <Label>{t("admin.users.colGroups")}</Label>
                {form.role === "admin" ? (
                  <p className="text-xs text-muted-foreground">{t("admin.access.adminBypassHint")}</p>
                ) : groups.length === 0 ? (
                  <p className="text-xs text-muted-foreground">{t("admin.groups.empty")}</p>
                ) : (
                  <div className="flex flex-wrap gap-2 rounded-md border border-border p-2.5">
                    {groups.map((g) => (
                      <label
                        key={g.id}
                        className="flex cursor-pointer items-center gap-1.5 text-xs"
                      >
                        <input
                          type="checkbox"
                          className="accent-primary"
                          checked={form.group_ids.includes(g.id)}
                          onChange={() =>
                            setForm({ ...form, group_ids: toggleGroupId(form.group_ids, g.id) })
                          }
                        />
                        <span>{g.name}</span>
                      </label>
                    ))}
                  </div>
                )}
              </div>
            )}
            <div className="col-span-2">
              <ErrorText>{error}</ErrorText>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void create()}>
              <UserCheck data-icon="inline-start" />
              {t("common.create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={tempPassword !== null} onOpenChange={(o) => !o && setTempPassword(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("admin.users.tempPwd")}</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            {t("admin.users.tempPwdHint", { name: tempPassword?.username ?? "" })}
          </p>
          <code className="block select-all rounded-lg border border-border bg-secondary p-3 text-center font-mono text-lg">
            {tempPassword?.password}
          </code>
          <DialogFooter>
            <Button onClick={() => setTempPassword(null)}>{t("common.done")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit User Dialog */}
      <Dialog open={edit !== null} onOpenChange={(o) => !o && setEdit(null)}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("users.editTitle", { name: edit?.user.username ?? "" })}</DialogTitle>
          </DialogHeader>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>{t("users.displayName")}</Label>
              <Input
                value={edit?.display_name ?? ""}
                onChange={(e) => setEdit({ ...edit!, display_name: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("admin.users.fRole")}</Label>
              <Select
                value={edit?.role ?? "member"}
                items={{
                  admin: t("role.admin"),
                  member: t("role.member"),
                }}
                onValueChange={(v) => v && setEdit({ ...edit!, role: v as Role })}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {ROLES.map((r) => (
                      <SelectItem key={r} value={r}>
                        {t(r === "admin" ? "role.admin" : "role.member")}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            <div className="col-span-2 flex items-center gap-2">
              <Switch
                checked={edit?.is_active ?? true}
                onCheckedChange={(v) => setEdit({ ...edit!, is_active: v })}
              />
              <Label>{t("admin.users.colActive")}</Label>
            </div>

            {canManageAccess && edit && (
              <div className="col-span-2 flex flex-col gap-2">
                <Label>{t("admin.users.colGroups")}</Label>
                {edit.role === "admin" ? (
                  <p className="text-xs text-muted-foreground">{t("admin.access.adminBypassHint")}</p>
                ) : (
                  <>
                    {groups.length === 0 ? (
                      <p className="text-xs text-muted-foreground">{t("admin.groups.empty")}</p>
                    ) : (
                      <div className="flex flex-wrap gap-2 rounded-md border border-border p-2.5">
                        {groups.map((g) => (
                          <label
                            key={g.id}
                            className="flex cursor-pointer items-center gap-1.5 text-xs"
                          >
                            <input
                              type="checkbox"
                              className="accent-primary"
                              checked={edit.group_ids.includes(g.id)}
                              onChange={() =>
                                setEdit({ ...edit, group_ids: toggleGroupId(edit.group_ids, g.id) })
                              }
                            />
                            <span>{g.name}</span>
                          </label>
                        ))}
                      </div>
                    )}
                    <div className="flex flex-col gap-1.5 pt-1">
                      <Label className="text-xs text-muted-foreground">
                        {t("admin.users.effectiveAccess")}
                      </Label>
                      {effectiveHosts.length === 0 ? (
                        <p className="text-xs text-muted-foreground">
                          {t("admin.users.noEffectiveHosts")}
                        </p>
                      ) : (
                        <div className="max-h-36 space-y-1 overflow-y-auto rounded-md border border-border bg-muted/30 p-2">
                          {effectiveHosts.map((h) => (
                            <div
                              key={h.id}
                              className="flex items-center justify-between gap-2 text-xs"
                            >
                              <span className="truncate font-medium">{h.name}</span>
                                <div className="flex shrink-0 flex-wrap items-center justify-end gap-1">
                                  {h.connect && (
                                    <Badge variant="secondary" className="text-[10px]">
                                      {t("admin.access.colConnect")}
                                    </Badge>
                                  )}
                                  {h.terminalNoApproval && (
                                    <Badge variant="outline" className="text-[10px]">
                                      {t("admin.access.colTerminalNoApproval")}
                                    </Badge>
                                  )}
                                  {h.transfer && (
                                    <Badge variant="secondary" className="text-[10px]">
                                      {t("admin.access.colTransfer")}
                                    </Badge>
                                  )}
                                  {h.execute && (
                                    <Badge variant="secondary" className="text-[10px]">
                                      {t("admin.access.colExecute")}
                                    </Badge>
                                  )}
                                  {h.mcpNoApproval && (
                                    <Badge variant="outline" className="text-[10px]">
                                      {t("admin.access.colMcpNoApproval")}
                                    </Badge>
                                  )}
                                </div>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  </>
                )}
              </div>
            )}

            <div className="col-span-2">
              <ErrorText>{error}</ErrorText>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEdit(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void saveEdit()}>{t("common.save")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={confirmAction !== null}
        destructive={confirmAction?.kind === "delete"}
        title={
          confirmAction?.kind === "delete" ? t("common.delete") : t("admin.users.resetTotp")
        }
        description={
          confirmAction?.kind === "delete"
            ? t("admin.users.deleteConfirm", { name: confirmAction.user.username })
            : t("admin.users.resetTotpConfirm", { name: confirmAction?.user.username ?? "" })
        }
        confirmLabel={confirmAction?.kind === "delete" ? t("common.delete") : t("admin.users.resetTotp")}
        onConfirm={() => void runConfirm()}
        onOpenChange={(o) => !o && setConfirmAction(null)}
      />
    </div>
  );
}
