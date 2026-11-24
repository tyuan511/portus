import { useEffect, useMemo, useState } from "react";
import { FolderOpen, FolderTree, Pencil, Plug, Plus, Trash2, X } from "lucide-react";
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ErrorText } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { ServerAsset } from "@/types";

interface HostGroupsPanelProps {
  servers: ServerAsset[];
  canManage: boolean;
  canConnect: boolean;
  canTransfer?: boolean;
  onConnect: (server: ServerAsset) => void;
  onFiles?: (server: ServerAsset) => void;
  onReload: () => Promise<void>;
}

export function HostGroupsPanel({
  servers,
  canManage,
  canConnect,
  canTransfer = false,
  onConnect,
  onFiles,
  onReload,
}: HostGroupsPanelProps) {
  const { t } = useI18n();
  const [selectedGroup, setSelectedGroup] = useState<string>("");
  const [assignHostId, setAssignHostId] = useState("");
  const [error, setError] = useState<string>();
  const [createOpen, setCreateOpen] = useState(false);
  const [newGroupName, setNewGroupName] = useState("");
  const [pickedHostIds, setPickedHostIds] = useState<string[]>([]);
  const [renameTarget, setRenameTarget] = useState<{ oldName: string; newName: string } | null>(null);
  const [dissolveTarget, setDissolveTarget] = useState<string | null>(null);

  const grouped = useMemo(() => {
    const map = new Map<string, ServerAsset[]>();
    for (const s of servers) {
      const key = s.group_name || "";
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(s);
    }
    return [...map.entries()].sort(([a], [b]) => {
      if (a === "") return 1;
      if (b === "") return -1;
      return a.localeCompare(b);
    });
  }, [servers]);

  useEffect(() => {
    if (grouped.length === 0) {
      setSelectedGroup("");
      return;
    }
    const stillExists = grouped.some(([name]) => name === selectedGroup);
    if (!stillExists) {
      setSelectedGroup(grouped[0][0]);
    }
  }, [grouped, selectedGroup]);

  const currentList = useMemo(
    () => grouped.find(([name]) => name === selectedGroup)?.[1] ?? [],
    [grouped, selectedGroup]
  );

  const assignableServers = useMemo(
    () => servers.filter((s) => (s.group_name || "") !== selectedGroup),
    [servers, selectedGroup]
  );

  const updateServerGroup = async (server: ServerAsset, groupName: string) => {
    await api.post("/api/servers", {
      ...server,
      group_name: groupName || undefined,
      tags: server.tags ?? [],
      description: server.description ?? "",
    });
  };

  const assignHost = async () => {
    if (!assignHostId || !selectedGroup) return;
    const srv = servers.find((s) => s.id === assignHostId);
    if (!srv) return;
    try {
      await updateServerGroup(srv, selectedGroup);
      setAssignHostId("");
      await onReload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "update failed");
    }
  };

  const removeHostFromGroup = async (srv: ServerAsset) => {
    try {
      await updateServerGroup(srv, "");
      await onReload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "update failed");
    }
  };

  const createGroup = async () => {
    const name = newGroupName.trim();
    if (!name || pickedHostIds.length === 0) return;
    try {
      const targets = servers.filter((s) => pickedHostIds.includes(s.id));
      await Promise.all(targets.map((s) => updateServerGroup(s, name)));
      setCreateOpen(false);
      setNewGroupName("");
      setPickedHostIds([]);
      setSelectedGroup(name);
      await onReload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "create group failed");
    }
  };

  const doRename = async () => {
    if (!renameTarget) return;
    const nextName = renameTarget.newName.trim();
    if (!nextName || nextName === renameTarget.oldName) {
      setRenameTarget(null);
      return;
    }
    try {
      const targets = servers.filter((s) => (s.group_name || "") === renameTarget.oldName);
      await Promise.all(targets.map((s) => updateServerGroup(s, nextName)));
      setRenameTarget(null);
      setSelectedGroup(nextName);
      await onReload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "rename failed");
    }
  };

  const dissolveGroup = async (groupName: string) => {
    if (!groupName) return;
    try {
      const targets = servers.filter((s) => (s.group_name || "") === groupName);
      await Promise.all(targets.map((s) => updateServerGroup(s, "")));
      setDissolveTarget(null);
      await onReload();
    } catch (err) {
      setDissolveTarget(null);
      setError(err instanceof Error ? err.message : "dissolve failed");
    }
  };

  const togglePicked = (id: string) => {
    setPickedHostIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]
    );
  };

  const displayName = (name: string) => name || t("hosts.ungrouped");

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h2 className="text-sm font-semibold tracking-tight">{t("hosts.groups.title")}</h2>
          <p className="text-xs text-muted-foreground">{t("hosts.groups.sub")}</p>
        </div>
        {canManage && servers.length > 0 && (
          <Button size="sm" onClick={() => setCreateOpen(true)}>
            <Plus data-icon="inline-start" />
            {t("hosts.groups.add")}
          </Button>
        )}
      </div>

      <ErrorText>{error}</ErrorText>

      {grouped.length === 0 ? (
        <Empty className="border border-border bg-card py-14">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <FolderTree />
            </EmptyMedia>
            <EmptyTitle>{t("hosts.groups.title")}</EmptyTitle>
            <EmptyDescription>{t("hosts.groups.empty")}</EmptyDescription>
          </EmptyHeader>
          {canManage && servers.length > 0 && (
            <EmptyContent>
              <Button size="sm" onClick={() => setCreateOpen(true)}>
                <Plus data-icon="inline-start" />
                {t("hosts.groups.add")}
              </Button>
            </EmptyContent>
          )}
        </Empty>
      ) : (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1.1fr_1fr]">
          {/* Host Groups Table */}
          <Card className="p-0">
            <Table>
              <TableHeader className="bg-secondary/50">
                <TableRow>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("hosts.groups.colGroup")}
                  </TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("hosts.groups.colCount")}
                  </TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("hosts.groups.colActive")}
                  </TableHead>
                  <TableHead className="px-4" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {grouped.map(([name, list]) => {
                  const isSelected = name === selectedGroup;
                  const activeCount = list.filter((s) => s.enabled).length;
                  return (
                    <TableRow
                      key={name || "__ungrouped__"}
                      onClick={() => setSelectedGroup(name)}
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
                          <span>{displayName(name)}</span>
                        </div>
                      </TableCell>
                      <TableCell className="px-4 font-mono text-xs tabular-nums">
                        {list.length}
                      </TableCell>
                      <TableCell className="px-4 font-mono text-xs text-muted-foreground tabular-nums">
                        {activeCount}
                      </TableCell>
                      <TableCell className="px-4 text-right">
                        {canManage && name !== "" && (
                          <div
                            className="flex justify-end gap-1"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <Button
                              variant="ghost"
                              size="icon-xs"
                              title={t("hosts.groups.rename")}
                              onClick={() => setRenameTarget({ oldName: name, newName: name })}
                            >
                              <Pencil />
                            </Button>
                            <Button
                              variant="destructive"
                              size="icon-xs"
                              title={t("hosts.groups.dissolve")}
                              onClick={() => setDissolveTarget(name)}
                            >
                              <Trash2 />
                            </Button>
                          </div>
                        )}
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </Card>

          {/* Group Hosts Inspector */}
          <Card>
            <CardHeader className="border-b border-border/70 pb-3">
              <div className="flex items-center justify-between gap-2">
                <CardTitle className="text-sm">{displayName(selectedGroup)}</CardTitle>
                <Badge variant="secondary" className="font-mono text-[11px] tabular-nums">
                  {currentList.length}
                </Badge>
              </div>
              <CardDescription className="text-xs">
                {t("hosts.groups.selectPrompt")}
              </CardDescription>
            </CardHeader>

            <CardContent className="flex flex-col gap-3">
              {canManage && selectedGroup !== "" && assignableServers.length > 0 && (
                <div className="flex gap-2">
                  <Select
                    value={assignHostId || null}
                    items={Object.fromEntries(
                      assignableServers.map((s) => [s.id, `${s.name} (${s.host})`])
                    )}
                    onValueChange={(v) => setAssignHostId(v ?? "")}
                  >
                    <SelectTrigger size="sm" className="flex-1">
                      <SelectValue placeholder={t("hosts.groups.selectHost")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        {assignableServers.map((s) => (
                          <SelectItem key={s.id} value={s.id}>
                            {s.name} ({s.host})
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                  <Button size="sm" onClick={() => void assignHost()} disabled={!assignHostId}>
                    <Plus data-icon="inline-start" />
                    {t("hosts.groups.assignHost")}
                  </Button>
                </div>
              )}

              {currentList.length === 0 ? (
                <p className="rounded-lg border border-dashed border-border p-4 text-center text-xs text-muted-foreground">
                  {t("hosts.groups.noHostsInGroup")}
                </p>
              ) : (
                <div className="flex flex-col divide-y divide-border overflow-hidden rounded-lg border border-border">
                  {currentList.map((s) => (
                    <div
                      key={s.id}
                      className="flex items-center justify-between gap-2 bg-background px-3 py-2"
                    >
                      <div className="min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="truncate text-xs font-medium">{s.name}</span>
                          {!s.enabled && (
                            <Badge variant="secondary" className="text-[10px]">
                              {t("hosts.disabled")}
                            </Badge>
                          )}
                        </div>
                        <div className="truncate font-mono text-[11px] text-muted-foreground">
                          {s.username}@{s.host}:{s.port}
                        </div>
                      </div>
                      <div className="flex shrink-0 items-center gap-1">
                        {canConnect && (
                          <Button
                            size="xs"
                            variant="outline"
                            onClick={() => onConnect(s)}
                            disabled={!s.enabled}
                            title={!s.enabled ? t("hosts.disabledHint") : undefined}
                          >
                            <Plug data-icon="inline-start" />
                            {t("hosts.connect")}
                          </Button>
                        )}
                        {canTransfer && onFiles && (
                          <Tooltip>
                            <TooltipTrigger
                              render={<span className="inline-flex" />}
                            >
                              <Button
                                size="xs"
                                variant="outline"
                                disabled={!s.enabled || s.jump_mode === "su_ssh"}
                                onClick={() => onFiles(s)}
                              >
                                <FolderOpen data-icon="inline-start" />
                                {t("files.title")}
                              </Button>
                            </TooltipTrigger>
                            <TooltipContent>
                              {!s.enabled
                                ? t("hosts.disabledHint")
                                : s.jump_mode === "su_ssh"
                                  ? t("hosts.sftpUnsupportedSuSSH")
                                  : t("files.open")}
                            </TooltipContent>
                          </Tooltip>
                        )}
                        {canManage && selectedGroup !== "" && (
                          <Button
                            variant="ghost"
                            size="icon-xs"
                            title={t("hosts.groups.removeFromGroup")}
                            onClick={() => void removeHostFromGroup(s)}
                          >
                            <X />
                          </Button>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      )}

      {/* Create group dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("hosts.groups.add")}</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>{t("hosts.f.group")}</Label>
              <Input
                placeholder={t("hosts.groups.phName")}
                value={newGroupName}
                onChange={(e) => setNewGroupName(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("hosts.groups.selectHosts")}</Label>
              <div className="max-h-52 divide-y divide-border overflow-y-auto rounded-lg border border-border">
                {servers.map((s) => {
                  const checked = pickedHostIds.includes(s.id);
                  return (
                    <label
                      key={s.id}
                      className="flex cursor-pointer items-center justify-between gap-2 px-3 py-2 text-xs hover:bg-muted/50"
                    >
                      <div className="flex items-center gap-2">
                        <input
                          type="checkbox"
                          checked={checked}
                          onChange={() => togglePicked(s.id)}
                        />
                        <span className="font-medium">{s.name}</span>
                        <span className="font-mono text-muted-foreground">{s.host}</span>
                      </div>
                      {s.group_name && (
                        <Badge variant="outline" className="font-mono text-[10px]">
                          {s.group_name}
                        </Badge>
                      )}
                    </label>
                  );
                })}
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={() => void createGroup()}
              disabled={!newGroupName.trim() || pickedHostIds.length === 0}
            >
              {t("common.create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Rename group dialog */}
      <Dialog open={renameTarget !== null} onOpenChange={(o) => !o && setRenameTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("hosts.groups.rename")}</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-1.5">
            <Label>{t("hosts.f.group")}</Label>
            <Input
              value={renameTarget?.newName ?? ""}
              onChange={(e) =>
                renameTarget && setRenameTarget({ ...renameTarget, newName: e.target.value })
              }
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRenameTarget(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void doRename()} disabled={!renameTarget?.newName.trim()}>
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={dissolveTarget !== null}
        title={t("hosts.groups.dissolve")}
        description={t("hosts.groups.dissolveConfirm", { name: dissolveTarget ?? "" })}
        confirmLabel={t("hosts.groups.dissolve")}
        onConfirm={() => dissolveTarget && void dissolveGroup(dissolveTarget)}
        onOpenChange={(o) => !o && setDissolveTarget(null)}
      />
    </div>
  );
}
