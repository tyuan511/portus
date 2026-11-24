import { useCallback, useEffect, useMemo, useState } from "react";
import { Pencil, Plus, Server as ServerIcon, ShieldCheck, Trash2, Users, X } from "lucide-react";
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
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ErrorText } from "@/components/ui/shared";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { HostGrant, ServerAsset, User, UserGroup, UserGroupMembership } from "@/types";

export default function AdminAccess() {
  const { t } = useI18n();
  const [groups, setGroups] = useState<UserGroup[]>([]);
  const [users, setUsers] = useState<User[]>([]);
  const [memberships, setMemberships] = useState<UserGroupMembership[]>([]);
  const [servers, setServers] = useState<ServerAsset[]>([]);
  const [grants, setGrants] = useState<HostGrant[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [addUserId, setAddUserId] = useState("");
  const [hostSearch, setHostSearch] = useState("");
  const [error, setError] = useState<string>();
  const [savingKey, setSavingKey] = useState<string | null>(null);
  const [form, setForm] = useState<{ id?: string; name: string; description: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<UserGroup | null>(null);

  const load = useCallback(async () => {
    try {
      const [gs, us, ms, srvs, grs] = await Promise.all([
        api.get<UserGroup[]>("/api/admin/groups"),
        api.get<User[]>("/api/admin/users"),
        api.get<UserGroupMembership[]>("/api/admin/group-memberships"),
        api.get<ServerAsset[]>("/api/servers"),
        api.get<HostGrant[]>("/api/admin/access-grants"),
      ]);
      setGroups(gs);
      setUsers(us);
      setMemberships(ms);
      setServers(srvs);
      setGrants(grs);
      setSelectedId((cur) => (cur && gs.some((g) => g.id === cur) ? cur : gs[0]?.id));
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const userById = useMemo(() => {
    const map = new Map<string, User>();
    for (const u of users) map.set(u.id, u);
    return map;
  }, [users]);

  const memberCountByGroup = useMemo(() => {
    const map = new Map<string, number>();
    for (const m of memberships) {
      map.set(m.group_id, (map.get(m.group_id) ?? 0) + 1);
    }
    return map;
  }, [memberships]);

  const grantCountByGroup = useMemo(() => {
    const map = new Map<string, number>();
    for (const g of grants) {
      if (g.can_connect || g.can_execute || g.can_transfer) {
        map.set(g.user_group_id, (map.get(g.user_group_id) ?? 0) + 1);
      }
    }
    return map;
  }, [grants]);

  const selected = groups.find((g) => g.id === selectedId);

  const selectedMembers = useMemo(() => {
    if (!selectedId) return [];
    return memberships
      .filter((m) => m.group_id === selectedId)
      .map((m) => userById.get(m.user_id))
      .filter((u): u is User => Boolean(u))
      .sort((a, b) => a.username.localeCompare(b.username));
  }, [memberships, selectedId, userById]);

  const addableUsers = useMemo(() => {
    const memberSet = new Set(selectedMembers.map((m) => m.id));
    return users
      .filter((u) => u.role !== "admin" && !memberSet.has(u.id))
      .sort((a, b) => a.username.localeCompare(b.username));
  }, [users, selectedMembers]);

  const grantByServerId = useMemo(() => {
    const map = new Map<string, HostGrant>();
    if (!selectedId) return map;
    for (const g of grants) {
      if (g.user_group_id === selectedId) {
        map.set(g.server_id, g);
      }
    }
    return map;
  }, [grants, selectedId]);

  const filteredServers = useMemo(() => {
    const q = hostSearch.trim().toLowerCase();
    if (!q) return servers;
    return servers.filter(
      (s) =>
        s.name.toLowerCase().includes(q) ||
        s.host.toLowerCase().includes(q) ||
        s.username.toLowerCase().includes(q)
    );
  }, [servers, hostSearch]);

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
      toast.success(t("common.saved"));
    } catch (err) {
      const msg = err instanceof Error ? err.message : "save failed";
      setError(msg);
      toast.error(msg);
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
      setMemberships((prev) => [...prev, { group_id: selectedId, user_id: addUserId }]);
      setAddUserId("");
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "add failed");
    }
  };

  const removeMember = async (u: User) => {
    if (!selectedId) return;
    try {
      await api.del(`/api/admin/groups/${selectedId}/members/${u.id}`);
      setMemberships((prev) =>
        prev.filter((m) => !(m.group_id === selectedId && m.user_id === u.id))
      );
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "remove failed");
    }
  };

  const upsertHostGrant = async (
    serverId: string,
    next: {
      can_connect: boolean;
      can_execute: boolean;
      can_transfer: boolean;
      terminal_no_approval: boolean;
      mcp_no_approval: boolean;
    }
  ) => {
    if (!selectedId) return;
    const normalized = {
      can_connect: next.can_connect,
      can_execute: next.can_execute,
      can_transfer: next.can_transfer,
      terminal_no_approval: next.can_connect && next.terminal_no_approval,
      mcp_no_approval: next.can_execute && next.mcp_no_approval,
    };
    const key = `${selectedId}:${serverId}`;
    setSavingKey(key);
    try {
      const hasAny = normalized.can_connect || normalized.can_execute || normalized.can_transfer;
      const res = await api.post<HostGrant | { deleted: boolean }>("/api/admin/access-grants", {
        user_group_id: selectedId,
        server_id: serverId,
        ...normalized,
      });
      setGrants((prev) => {
        const rest = prev.filter(
          (g) => !(g.user_group_id === selectedId && g.server_id === serverId)
        );
        if (!hasAny) return rest;
        const created = res as HostGrant;
        return [
          ...rest,
          {
            id: created.id ?? key,
            user_group_id: selectedId,
            server_id: serverId,
            ...normalized,
          },
        ];
      });
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "update failed");
    } finally {
      setSavingKey(null);
    }
  };

  const toggleBit = (
    serverId: string,
    field:
      | "can_connect"
      | "can_execute"
      | "can_transfer"
      | "terminal_no_approval"
      | "mcp_no_approval",
    value: boolean
  ) => {
    const cur = grantByServerId.get(serverId);
    const next = {
      can_connect: cur?.can_connect ?? false,
      can_execute: cur?.can_execute ?? false,
      can_transfer: cur?.can_transfer ?? false,
      terminal_no_approval: cur?.terminal_no_approval ?? false,
      mcp_no_approval: cur?.mcp_no_approval ?? false,
      [field]: value,
    };
    if (field === "can_connect" && !value) {
      next.terminal_no_approval = false;
    }
    if (field === "can_execute" && !value) {
      next.mcp_no_approval = false;
    }
    void upsertHostGrant(serverId, next);
  };

  const toggleAllForHost = (serverId: string) => {
    const cur = grantByServerId.get(serverId);
    const allBaseOn = Boolean(cur?.can_connect && cur?.can_transfer && cur?.can_execute);
    if (allBaseOn) {
      void upsertHostGrant(serverId, {
        can_connect: false,
        can_transfer: false,
        can_execute: false,
        terminal_no_approval: false,
        mcp_no_approval: false,
      });
      return;
    }
    void upsertHostGrant(serverId, {
      can_connect: true,
      can_transfer: true,
      can_execute: true,
      terminal_no_approval: cur?.terminal_no_approval ?? false,
      mcp_no_approval: cur?.mcp_no_approval ?? false,
    });
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h2 className="text-sm font-semibold tracking-tight">{t("admin.access.title")}</h2>
          <p className="text-xs text-muted-foreground">{t("admin.access.subtitle")}</p>
        </div>
        <Button size="sm" onClick={() => setForm({ name: "", description: "" })}>
          <Plus data-icon="inline-start" />
          {t("admin.groups.new")}
        </Button>
      </div>

      <ErrorText>{error}</ErrorText>

      {groups.length === 0 ? (
        <Empty className="border border-border bg-card py-14">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <ShieldCheck />
            </EmptyMedia>
            <EmptyTitle>{t("admin.groups.empty")}</EmptyTitle>
            <EmptyDescription>{t("admin.access.emptyHint")}</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button size="sm" onClick={() => setForm({ name: "", description: "" })}>
              <Plus data-icon="inline-start" />
              {t("admin.groups.new")}
            </Button>
          </EmptyContent>
        </Empty>
      ) : (
        <div className="flex flex-col gap-4 lg:flex-row lg:items-start">
          {/* Left: Permission Groups */}
          <Card className="gap-0 overflow-hidden py-0 lg:w-72 lg:shrink-0">
            <CardHeader className="border-b border-border px-4 py-3">
              <CardTitle className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                {t("admin.groups.title")} ({groups.length})
              </CardTitle>
            </CardHeader>
            <div className="divide-y divide-border">
              {groups.map((g) => {
                const active = g.id === selectedId;
                const mCount = memberCountByGroup.get(g.id) ?? 0;
                const hCount = grantCountByGroup.get(g.id) ?? 0;
                return (
                  <div
                    key={g.id}
                    role="button"
                    tabIndex={0}
                    onClick={() => {
                      setSelectedId(g.id);
                      setAddUserId("");
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        setSelectedId(g.id);
                      }
                    }}
                    className={cn(
                      "flex cursor-pointer items-center justify-between gap-2 px-4 py-3 transition-colors",
                      active ? "bg-muted/70" : "hover:bg-muted/35"
                    )}
                  >
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium">{g.name}</div>
                      {g.description && (
                        <div className="truncate text-xs text-muted-foreground">
                          {g.description}
                        </div>
                      )}
                      <div className="mt-1 flex items-center gap-2 text-[11px] text-muted-foreground">
                        <span>{t("admin.access.memberCount", { count: mCount })}</span>
                        <span>·</span>
                        <span>{t("admin.access.hostCount", { count: hCount })}</span>
                      </div>
                    </div>
                    <div
                      className="flex shrink-0 items-center gap-0.5"
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
                        variant="ghost"
                        size="icon-xs"
                        className="text-destructive hover:text-destructive"
                        onClick={() => setDeleteTarget(g)}
                      >
                        <Trash2 />
                      </Button>
                    </div>
                  </div>
                );
              })}
            </div>
          </Card>

          {/* Right: Selected Group Members + Host Permission Matrix */}
          {selected && (
            <div className="flex min-w-0 flex-1 flex-col gap-4">
              {/* Members */}
              <Card className="gap-0 overflow-hidden py-0">
                <CardHeader className="border-b border-border px-4 py-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                      <Users className="size-4 text-muted-foreground" />
                      <CardTitle className="text-sm">{t("admin.groups.members")}</CardTitle>
                      <Badge variant="secondary" className="text-[11px]">
                        {selectedMembers.length}
                      </Badge>
                    </div>
                    <span className="text-xs text-muted-foreground">
                      {t("admin.access.adminBypassHint")}
                    </span>
                  </div>
                </CardHeader>
                <CardContent className="flex flex-col gap-3 p-4">
                  <div className="flex flex-wrap items-center gap-2">
                    <Select
                      value={addUserId || null}
                      items={Object.fromEntries(
                        addableUsers.map((u) => [
                          u.id,
                          u.display_name ? `${u.username} (${u.display_name})` : u.username,
                        ])
                      )}
                      onValueChange={(v) => setAddUserId(v ?? "")}
                      disabled={addableUsers.length === 0}
                    >
                      <SelectTrigger className="w-64">
                        <SelectValue
                          placeholder={
                            addableUsers.length === 0
                              ? t("admin.access.noAddableMembers")
                              : t("admin.groups.pickUser")
                          }
                        />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          {addableUsers.map((u) => (
                            <SelectItem key={u.id} value={u.id}>
                              {u.display_name ? `${u.username} (${u.display_name})` : u.username}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                    <Button size="sm" onClick={addMember} disabled={!addUserId}>
                      <Plus data-icon="inline-start" />
                      {t("admin.groups.addMember")}
                    </Button>
                  </div>

                  {selectedMembers.length === 0 ? (
                    <p className="text-xs text-muted-foreground">{t("admin.groups.noMembers")}</p>
                  ) : (
                    <div className="flex flex-wrap gap-2">
                      {selectedMembers.map((m) => (
                        <Badge
                          key={m.id}
                          variant="secondary"
                          className="gap-1.5 py-1 pl-2.5 pr-1.5 text-xs font-normal"
                        >
                          <span className="font-mono font-medium">{m.username}</span>
                          {m.display_name && (
                            <span className="text-muted-foreground">({m.display_name})</span>
                          )}
                          <button
                            type="button"
                            className="ml-0.5 rounded-sm p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
                            onClick={() => void removeMember(m)}
                            title={t("common.delete")}
                          >
                            <X className="size-3" />
                          </button>
                        </Badge>
                      ))}
                    </div>
                  )}
                </CardContent>
              </Card>

              {/* Host Permission Matrix */}
              <Card className="gap-0 overflow-hidden py-0">
                <CardHeader className="border-b border-border px-4 py-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <div className="flex items-center gap-2">
                        <ServerIcon className="size-4 text-muted-foreground" />
                        <CardTitle className="text-sm">{t("admin.access.hostMatrix")}</CardTitle>
                        <Badge variant="secondary" className="text-[11px]">
                          {grantCountByGroup.get(selected.id) ?? 0} / {servers.length}
                        </Badge>
                      </div>
                      <CardDescription className="mt-0.5 text-xs">
                        {t("admin.access.hostMatrixHint")}
                      </CardDescription>
                    </div>
                    <Input
                      placeholder={t("hosts.search")}
                      className="h-8 w-52 text-xs"
                      value={hostSearch}
                      onChange={(e) => setHostSearch(e.target.value)}
                    />
                  </div>
                </CardHeader>

                {servers.length === 0 ? (
                  <CardContent className="py-10 text-center text-xs text-muted-foreground">
                    {t("admin.access.noHosts")}
                  </CardContent>
                ) : (
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead className="px-4">{t("admin.access.colHost")}</TableHead>
                        <TableHead className="px-3 text-center whitespace-nowrap">
                          {t("admin.access.colConnect")}
                        </TableHead>
                        <TableHead className="px-3 text-center whitespace-nowrap">
                          {t("admin.access.colTerminalNoApproval")}
                        </TableHead>
                        <TableHead className="px-3 text-center whitespace-nowrap">
                          {t("admin.access.colTransfer")}
                        </TableHead>
                        <TableHead className="px-3 text-center whitespace-nowrap">
                          {t("admin.access.colExecute")}
                        </TableHead>
                        <TableHead className="px-3 text-center whitespace-nowrap">
                          {t("admin.access.colMcpNoApproval")}
                        </TableHead>
                        <TableHead className="px-4 text-right">{t("common.actions")}</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {filteredServers.map((s) => {
                        const g = grantByServerId.get(s.id);
                        const canConnect = g?.can_connect ?? false;
                        const terminalNoApproval = canConnect && (g?.terminal_no_approval ?? false);
                        const canTransfer = g?.can_transfer ?? false;
                        const canExecute = g?.can_execute ?? false;
                        const mcpNoApproval = canExecute && (g?.mcp_no_approval ?? false);
                        const allOn = canConnect && canTransfer && canExecute;
                        const busy = savingKey === `${selected.id}:${s.id}`;
                        return (
                          <TableRow key={s.id}>
                            <TableCell className="px-4 py-2.5">
                              <div className="flex items-center gap-2">
                                <span className="font-medium">{s.name}</span>
                                {!s.enabled && (
                                  <Badge variant="secondary" className="text-[10px]">
                                    {t("hosts.disabled")}
                                  </Badge>
                                )}
                              </div>
                              <div className="font-mono text-xs text-muted-foreground">
                                {s.username}@{s.host}:{s.port}
                              </div>
                            </TableCell>
                            <TableCell className="px-3 py-2.5 text-center">
                              <Switch
                                checked={canConnect}
                                disabled={busy}
                                onCheckedChange={(v) => toggleBit(s.id, "can_connect", v)}
                              />
                            </TableCell>
                            <TableCell className="px-3 py-2.5 text-center">
                              <Switch
                                checked={terminalNoApproval}
                                disabled={busy || !canConnect}
                                onCheckedChange={(v) => toggleBit(s.id, "terminal_no_approval", v)}
                              />
                            </TableCell>
                            <TableCell className="px-3 py-2.5 text-center">
                              <Switch
                                checked={canTransfer}
                                disabled={busy}
                                onCheckedChange={(v) => toggleBit(s.id, "can_transfer", v)}
                              />
                            </TableCell>
                            <TableCell className="px-3 py-2.5 text-center">
                              <Switch
                                checked={canExecute}
                                disabled={busy}
                                onCheckedChange={(v) => toggleBit(s.id, "can_execute", v)}
                              />
                            </TableCell>
                            <TableCell className="px-3 py-2.5 text-center">
                              <Switch
                                checked={mcpNoApproval}
                                disabled={busy || !canExecute}
                                onCheckedChange={(v) => toggleBit(s.id, "mcp_no_approval", v)}
                              />
                            </TableCell>
                            <TableCell className="px-4 py-2.5 text-right">
                              <Button
                                variant="ghost"
                                size="xs"
                                disabled={busy}
                                onClick={() => toggleAllForHost(s.id)}
                              >
                                {allOn ? t("admin.access.clearRow") : t("admin.access.grantAllRow")}
                              </Button>
                            </TableCell>
                          </TableRow>
                        );
                      })}
                    </TableBody>
                  </Table>
                )}
              </Card>
            </div>
          )}
        </div>
      )}

      {/* Create / Edit Group Dialog */}
      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>
              {form?.id ? t("admin.groups.edit") : t("admin.groups.new")}
            </DialogTitle>
          </DialogHeader>
          {form && (
            <div className="flex flex-col gap-3">
              <div className="flex flex-col gap-1.5">
                <Label>{t("admin.groups.name")}</Label>
                <Input
                  value={form.name}
                  placeholder={t("admin.access.phGroupName")}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>{t("admin.groups.description")}</Label>
                <Input
                  value={form.description}
                  onChange={(e) => setForm({ ...form, description: e.target.value })}
                />
              </div>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={saveGroup} disabled={!form?.name.trim()}>
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("admin.groups.deleteTitle")}
        description={
          deleteTarget ? t("admin.groups.deleteConfirm", { name: deleteTarget.name }) : ""
        }
        confirmLabel={t("common.delete")}
        destructive
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        onConfirm={() => {
          if (deleteTarget) void removeGroup(deleteTarget);
        }}
      />
    </div>
  );
}
