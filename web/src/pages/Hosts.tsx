import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, FileUp, FolderOpen, Pencil, Plug, Plus, Search, Server, Trash2, Upload } from "lucide-react";
import { cn } from "cn";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import { ConfirmDialog } from "@/components/bulwark/ConfirmDialog";
import { StatusPill } from "@/components/bulwark/StatusDot";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { useWorkspaceTabs } from "@/lib/workspace-tabs";
import type { CredentialMeta, ServerAsset } from "@/types";

const EMPTY_FORM = {
  id: "",
  name: "",
  host: "",
  port: 22,
  username: "root",
  auth_kind: "agent_or_default_key" as ServerAsset["auth_kind"],
  identity_file: "",
  credential_id: "",
  jump_host_id: "",
  jump_mode: "direct_tcpip" as NonNullable<ServerAsset["jump_mode"]>,
  jump_su_username: "",
  jump_su_credential_id: "",
  group_name: "",
  tags: "",
  enabled: true,
};

export default function Hosts() {
  const auth = useAuth();
  const { t } = useI18n();
  const { openTerminal, openFiles } = useWorkspaceTabs();
  const [servers, setServers] = useState<ServerAsset[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<typeof EMPTY_FORM | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [importTab, setImportTab] = useState<"ssh" | "csv">("ssh");
  const [csvText, setCsvText] = useState("");
  const [csvBusy, setCsvBusy] = useState(false);
  const [testResult, setTestResult] = useState<Record<string, { ok?: boolean; text: string }>>({});
  const [credentials, setCredentials] = useState<CredentialMeta[]>([]);
  const [newCred, setNewCred] = useState<{ label: string; username: string; secret: string } | null>(null);
  const [credError, setCredError] = useState<string>();
  const [localKeyFile, setLocalKeyFile] = useState<{ name: string; content: string } | null>(null);
  const [jumpSuPassword, setJumpSuPassword] = useState("");
  const keyFileInputRef = useRef<HTMLInputElement>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [bulkBusy, setBulkBusy] = useState(false);
  const [bulkDeleteOpen, setBulkDeleteOpen] = useState(false);
  const [deleteHostId, setDeleteHostId] = useState<string | null>(null);

  const canManage = auth.hasPermission("host.manage");
  const canConnect = auth.hasPermission("host.connect");
  const canTransfer = auth.hasPermission("host.transfer");

  const load = useCallback(async () => {
    try {
      setServers(await api.get<ServerAsset[]>("/api/servers"));
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    if (auth.hasPermission("host.read")) void load();
  }, [load, auth.identity?.user_id]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return servers.filter((s) => {
      if (!q) return true;
      return (
        s.name.toLowerCase().includes(q) ||
        s.host.toLowerCase().includes(q) ||
        s.username.toLowerCase().includes(q) ||
        (s.tags ?? []).some((tag) => tag.toLowerCase().includes(q))
      );
    });
  }, [servers, query]);

  const jumpOptions = useMemo(
    () => servers.filter((s) => s.id !== form?.id),
    [servers, form?.id]
  );

  const loadCredentials = useCallback(async () => {
    if (!auth.hasPermission("host.manage")) return;
    try {
      setCredentials(await api.get<CredentialMeta[]>("/api/credentials"));
    } catch {
      setCredentials([]);
    }
  }, [auth.identity?.user_id]);

  const createCredential = async () => {
    if (!newCred?.label || !newCred?.secret) return;
    setCredError(undefined);
    try {
      const meta = await api.post<CredentialMeta>("/api/credentials", {
        kind: "password",
        label: newCred.label,
        username: newCred.username,
        secret: newCred.secret,
      });
      setNewCred(null);
      await loadCredentials();
      setForm((f) => (f ? { ...f, credential_id: meta.id } : f));
    } catch (err) {
      setCredError(err instanceof Error ? err.message : "create credential failed");
    }
  };

  const openForm = (s?: ServerAsset) => {
    void loadCredentials();
    setCredError(undefined);
    setLocalKeyFile(null);
    setJumpSuPassword("");
    setForm(
      s
        ? {
            id: s.id,
            name: s.name,
            host: s.host,
            port: s.port,
            username: s.username,
            auth_kind: s.auth_kind,
            identity_file: s.identity_file ?? "",
            credential_id: s.credential_id ?? "",
            jump_host_id: s.jump_host_id ?? "",
            jump_mode: (s.jump_mode || "direct_tcpip") as NonNullable<ServerAsset["jump_mode"]>,
            jump_su_username: s.jump_su_username ?? "",
            jump_su_credential_id: s.jump_su_credential_id ?? "",
            group_name: s.group_name ?? "",
            tags: (s.tags ?? []).join(", "),
            enabled: s.enabled,
          }
        : { ...EMPTY_FORM }
    );
  };

  const saveForm = async () => {
    if (!form) return;
    let authKind = form.auth_kind;
    let identityFile = form.identity_file || undefined;
    let credentialId = form.credential_id || undefined;
    let jumpSuCredentialId =
      form.jump_host_id && form.jump_mode === "su_ssh"
        ? form.jump_su_credential_id || undefined
        : undefined;
    try {
      if (authKind === "identity_file" && localKeyFile) {
        const meta = await api.post<CredentialMeta>("/api/credentials", {
          kind: "private_key",
          label: `${form.name || form.host || "host"} (${localKeyFile.name})`,
          username: form.username,
          secret: localKeyFile.content,
        });
        authKind = "keyring_password";
        credentialId = meta.id;
        identityFile = undefined;
      }
      if (jumpSuCredentialId === "__custom__") {
        if (jumpSuPassword) {
          const meta = await api.post<CredentialMeta>("/api/credentials", {
            kind: "password",
            label: `${form.name || form.host || "host"} su (${form.jump_su_username.trim() || "root"})`,
            username: form.jump_su_username.trim() || undefined,
            secret: jumpSuPassword,
          });
          jumpSuCredentialId = meta.id;
        } else {
          jumpSuCredentialId = undefined;
        }
      }
      const payload: ServerAsset = {
        id: form.id || "",
        name: form.name,
        host: form.host,
        port: Number(form.port) || 22,
        username: form.username,
        auth_kind: authKind,
        identity_file: identityFile,
        credential_id: credentialId,
        jump_host_id: form.jump_host_id || undefined,
        jump_mode: form.jump_host_id ? form.jump_mode : undefined,
        jump_su_username:
          form.jump_host_id && form.jump_mode === "su_ssh"
            ? form.jump_su_username.trim() || undefined
            : undefined,
        jump_su_credential_id: jumpSuCredentialId,
        group_name: form.group_name || undefined,
        tags: form.tags.split(",").map((tag) => tag.trim()).filter(Boolean),
        description: "",
        enabled: form.enabled,
      };
      await api.post("/api/servers", payload);
      setLocalKeyFile(null);
      setJumpSuPassword("");
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "save failed");
    }
  };

  const remove = async (id: string) => {
    try {
      await api.del(`/api/servers/${id}`);
      setDeleteHostId(null);
      await load();
    } catch (err) {
      setDeleteHostId(null);
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const test = async (id: string) => {
    setTestResult((r) => ({ ...r, [id]: { text: t("hosts.testing") } }));
    try {
      const res = await api.post<{ ok: boolean; message: string }>(`/api/servers/${id}/test`);
      setTestResult((r) => ({ ...r, [id]: { ok: res.ok, text: res.message } }));
    } catch (err) {
      setTestResult((r) => ({
        ...r,
        [id]: { ok: false, text: err instanceof Error ? err.message : "test failed" },
      }));
    }
  };

  const toggleSelected = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const setGroupSelected = (list: ServerAsset[], checked: boolean) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      for (const s of list) {
        if (checked) next.add(s.id);
        else next.delete(s.id);
      }
      return next;
    });
  };

  const runBulk = async (op: (s: ServerAsset) => Promise<void>) => {
    setBulkBusy(true);
    try {
      for (const id of selectedIds) {
        const target = servers.find((s) => s.id === id);
        if (target) await op(target);
      }
      setSelectedIds(new Set());
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "bulk operation failed");
    } finally {
      setBulkBusy(false);
    }
  };

  const bulkSetEnabled = (enabled: boolean) =>
    void runBulk(async (s) => {
      await api.post("/api/servers", {
        ...s,
        enabled,
        tags: s.tags ?? [],
        description: s.description ?? "",
      });
    });

  const bulkDelete = async () => {
    setBulkBusy(true);
    try {
      for (const id of selectedIds) {
        await api.del(`/api/servers/${id}`);
      }
      setBulkDeleteOpen(false);
      setSelectedIds(new Set());
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
      setBulkDeleteOpen(false);
    } finally {
      setBulkBusy(false);
    }
  };

  const pickFile = async (file: File | undefined) => {
    if (!file) return;
    try {
      setImportText(await file.text());
      setError(undefined);
    } catch {
      setError(t("hosts.fileReadFailed"));
    }
  };

  const pickCsvFile = async (file: File | undefined) => {
    if (!file) return;
    try {
      setCsvText(await file.text());
      setError(undefined);
    } catch {
      setError(t("hosts.fileReadFailed"));
    }
  };

  // Client-side CSV parsing: name,host,port,username,group per line;
  // `#` comments and blank lines are skipped, port defaults to 22.
  const csvRows = useMemo(
    () =>
      csvText
        .split(/\r?\n/)
        .map((line) => line.trim())
        .filter((line) => line && !line.startsWith("#"))
        .map((line) => {
          const parts = line.split(",").map((p) => p.trim());
          return {
            name: parts[0] ?? "",
            host: parts[1] ?? "",
            port: Number(parts[2]) > 0 ? Number(parts[2]) : 22,
            username: parts[3] || "root",
            group: parts[4] ?? "",
          };
        })
        .filter((row) => row.name && row.host),
    [csvText]
  );

  const doCsvImport = async () => {
    if (csvRows.length === 0) return;
    setCsvBusy(true);
    try {
      for (const row of csvRows) {
        await api.post("/api/servers", {
          id: "",
          name: row.name,
          host: row.host,
          port: row.port,
          username: row.username,
          auth_kind: "agent_or_default_key" as ServerAsset["auth_kind"],
          group_name: row.group || undefined,
          tags: [],
          description: "",
          enabled: true,
        });
      }
      setImportOpen(false);
      setCsvText("");
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "import failed");
    } finally {
      setCsvBusy(false);
    }
  };

  const doImport = async () => {
    try {
      await api.post("/api/servers/import-ssh-config", { content: importText });
      setImportOpen(false);
      setImportText("");
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "import failed");
    }
  };

  const authLabel = (kind: ServerAsset["auth_kind"]) => {
    if (kind === "identity_file") return t("hosts.authIdentity");
    if (kind === "keyring_password") return t("hosts.authCred");
    return t("hosts.authAgent");
  };

  const activeCount = servers.filter((s) => s.enabled).length;

  return (
    <Page
      header={
        <PageHeader
          title={t("hosts.title")}
          actions={
            canManage && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    setImportTab("ssh");
                    setImportOpen(true);
                  }}
                >
                  <Upload data-icon="inline-start" />
                  {t("hosts.import")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    setImportTab("csv");
                    setImportOpen(true);
                  }}
                >
                  <FileUp data-icon="inline-start" />
                  {t("hosts.csvImport")}
                </Button>
                <Button size="sm" onClick={() => openForm()}>
                  <Plus data-icon="inline-start" />
                  {t("hosts.add")}
                </Button>
              </>
            )
          }
        />
      }
    >
      <ErrorText>{error}</ErrorText>

      {servers.length > 0 && (
        <div className="mb-6 flex flex-col gap-4">
          {/* Metric Strip */}
          <div className="grid grid-cols-2 divide-x divide-border overflow-hidden rounded-xl border border-border bg-card">
            <div className="px-4 py-3">
              <div className="font-mono text-[11px] uppercase tracking-wider text-muted-foreground">
                {t("hosts.statTotal")}
              </div>
              <div className="mt-0.5 text-xl font-semibold tabular-nums">{servers.length}</div>
            </div>
            <div className="px-4 py-3">
              <div className="font-mono text-[11px] uppercase tracking-wider text-muted-foreground">
                {t("hosts.statEnabled")}
              </div>
              <div className="mt-0.5 text-xl font-semibold tabular-nums">{activeCount}</div>
            </div>
          </div>

          {/* Search Bar */}
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="relative w-full max-w-xs">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("hosts.searchPlaceholder")}
                className="pl-8"
              />
            </div>
          </div>
        </div>
      )}

      {selectedIds.size > 0 && (
        <div className="sticky top-2 z-20 mb-4 flex flex-wrap items-center gap-3 rounded-lg border border-border bg-card px-3 py-2 shadow-sm">
          <span className="text-sm font-medium">
            {t("sessions.selected", { count: selectedIds.size })}
          </span>
          <Button variant="ghost" size="xs" disabled={bulkBusy} onClick={() => setSelectedIds(new Set())}>
            {t("common.cancel")}
          </Button>
          <div className="ml-auto">
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button size="xs" variant="outline" disabled={bulkBusy}>
                    {t("common.actions")}
                    <ChevronDown data-icon="inline-end" />
                  </Button>
                }
              />
              <DropdownMenuContent align="end">
                <DropdownMenuItem disabled={bulkBusy} onClick={() => bulkSetEnabled(true)}>
                  {t("hosts.bulk.enable")}
                </DropdownMenuItem>
                <DropdownMenuItem disabled={bulkBusy} onClick={() => bulkSetEnabled(false)}>
                  {t("hosts.bulk.disable")}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  variant="destructive"
                  disabled={bulkBusy}
                  onClick={() => setBulkDeleteOpen(true)}
                >
                  {t("hosts.bulk.delete")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      )}

      {filtered.length === 0 ? (
        <Empty className="mt-6 border border-border bg-card py-14">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Server />
            </EmptyMedia>
            <EmptyTitle>{t("hosts.title")}</EmptyTitle>
            <EmptyDescription>
              {canManage ? t("hosts.emptyAdmin") : t("hosts.empty")}
            </EmptyDescription>
          </EmptyHeader>
          {canManage && (
            <EmptyContent>
              <Button size="sm" onClick={() => openForm()}>
                <Plus data-icon="inline-start" />
                {t("hosts.add")}
              </Button>
            </EmptyContent>
          )}
        </Empty>
      ) : (
        <Card className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-10 pl-4 pr-0">
                  <input
                    type="checkbox"
                    aria-label={t("common.all")}
                    checked={filtered.length > 0 && filtered.every((s) => selectedIds.has(s.id))}
                    onChange={(e) => setGroupSelected(filtered, e.target.checked)}
                  />
                </TableHead>
                <TableHead className="px-4 py-2 font-mono text-[11px] uppercase text-muted-foreground">
                  {t("hosts.tabHosts")}
                </TableHead>
                <TableHead className="px-4 py-2" />
                <TableHead className="px-4 py-2" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {filtered.map((s) => {
                      const tr = testResult[s.id];
                      return (
                        <TableRow key={s.id}>
                          <TableCell className="w-10 pl-4 pr-0 align-middle">
                            <input
                              type="checkbox"
                              aria-label={s.name}
                              checked={selectedIds.has(s.id)}
                              onChange={() => toggleSelected(s.id)}
                            />
                          </TableCell>
                          <TableCell className="px-4 py-3">
                            <div className="flex items-center gap-3">
                              <div className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border bg-secondary/60 text-muted-foreground">
                                <Server className="size-3.5" />
                              </div>
                              <div className="min-w-0">
                                <div className="flex flex-wrap items-center gap-2">
                                  <span className="font-medium text-foreground">{s.name}</span>
                                  {!s.enabled && (
                                    <Badge variant="secondary">{t("hosts.disabled")}</Badge>
                                  )}
                                  {(s.tags ?? []).map((tag) => (
                                    <Badge key={tag} variant="outline" className="font-mono text-[10px]">
                                      {tag}
                                    </Badge>
                                  ))}
                                </div>
                                <div className="mt-0.5 flex flex-wrap items-center gap-2 font-mono text-xs text-muted-foreground">
                                  <span>
                                    {s.username}@{s.host}:{s.port}
                                  </span>
                                  <span className="text-border">·</span>
                                  <span>{authLabel(s.auth_kind)}</span>
                                </div>
                              </div>
                            </div>
                          </TableCell>

                          <TableCell className="px-4 py-3">
                            {tr && (
                              <StatusPill
                                label={tr.text}
                                tone={tr.ok === true ? "success" : tr.ok === false ? "danger" : "neutral"}
                                pulse={tr.ok === undefined}
                              />
                            )}
                          </TableCell>

                          <TableCell className="px-4 py-3 text-right">
                            <div className="flex justify-end gap-1.5">
                              {canConnect && (
                                <Button
                                  size="xs"
                                  onClick={() => openTerminal(s)}
                                  disabled={!s.enabled}
                                  title={!s.enabled ? t("hosts.disabledHint") : undefined}
                                >
                                  <Plug data-icon="inline-start" />
                                  {t("hosts.connect")}
                                </Button>
                              )}
                              {canTransfer && (
                                <Tooltip>
                                  <TooltipTrigger
                                    render={<span className="inline-flex" />}
                                  >
                                    <Button
                                      variant="outline"
                                      size="xs"
                                      disabled={!s.enabled || s.jump_mode === "su_ssh"}
                                      onClick={() => openFiles({ id: s.id, name: s.name })}
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
                              {canManage && (
                                <>
                                  <Button variant="outline" size="xs" onClick={() => void test(s.id)}>
                                    {t("hosts.test")}
                                  </Button>
                                  <Button variant="ghost" size="icon-xs" onClick={() => openForm(s)}>
                                    <Pencil />
                                  </Button>
                                  <Button variant="destructive" size="icon-xs" onClick={() => setDeleteHostId(s.id)}>
                                    <Trash2 />
                                  </Button>
                                </>
                              )}
                            </div>
                          </TableCell>
                        </TableRow>
                      );
                    })}
            </TableBody>
          </Table>
        </Card>
      )}

      {/* Host form */}
      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{form?.id ? t("hosts.editHost") : t("hosts.add")}</DialogTitle>
          </DialogHeader>
          {form && (
            <div className="grid grid-cols-2 gap-3">
              <div className="col-span-2 flex flex-col gap-1.5">
                <Label>{t("common.name")}</Label>
                <Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>{t("hosts.f.host")}</Label>
                <Input value={form.host} onChange={(e) => setForm({ ...form, host: e.target.value })} />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>{t("hosts.f.port")}</Label>
                <Input
                  type="number"
                  value={form.port}
                  onChange={(e) => setForm({ ...form, port: Number(e.target.value) })}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>{t("hosts.f.sshUser")}</Label>
                <Input value={form.username} onChange={(e) => setForm({ ...form, username: e.target.value })} />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>{t("hosts.f.auth")}</Label>
                <Select
                  value={form.auth_kind}
                  items={{
                    agent_or_default_key: t("hosts.authAgent"),
                    identity_file: t("hosts.authIdentity"),
                    keyring_password: t("hosts.authCred"),
                  }}
                  onValueChange={(v) =>
                    v && setForm({ ...form, auth_kind: v as ServerAsset["auth_kind"] })
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="agent_or_default_key">{t("hosts.authAgent")}</SelectItem>
                      <SelectItem value="identity_file">{t("hosts.authIdentity")}</SelectItem>
                      <SelectItem value="keyring_password">{t("hosts.authCred")}</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
              {form.auth_kind === "identity_file" && (
                <div className="col-span-2 flex flex-col gap-1.5">
                  <Label>{t("hosts.f.identityPath")}</Label>
                  <div className="flex items-center gap-2">
                    <Input
                      className="flex-1"
                      value={localKeyFile ? localKeyFile.name : form.identity_file}
                      placeholder="~/.ssh/id_ed25519"
                      disabled={Boolean(localKeyFile)}
                      onChange={(e) => setForm({ ...form, identity_file: e.target.value })}
                    />
                    <input
                      ref={keyFileInputRef}
                      type="file"
                      className="hidden"
                      onChange={async (e) => {
                        const file = e.target.files?.[0];
                        e.target.value = "";
                        if (!file) return;
                        try {
                          const content = await file.text();
                          if (!content.includes("PRIVATE KEY")) {
                            setCredError(t("hosts.invalidKeyFile"));
                            return;
                          }
                          setCredError(undefined);
                          setLocalKeyFile({ name: file.name, content });
                        } catch {
                          setCredError(t("hosts.fileReadFailed"));
                        }
                      }}
                    />
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => keyFileInputRef.current?.click()}
                    >
                      <FolderOpen className="size-4" />
                      {t("hosts.chooseLocalFile")}
                    </Button>
                  </div>
                  {localKeyFile && (
                    <div className="flex items-center justify-between text-xs text-muted-foreground">
                      <span>{t("hosts.localKeySelected", { name: localKeyFile.name })}</span>
                      <button
                        type="button"
                        className="text-foreground underline hover:opacity-80"
                        onClick={() => setLocalKeyFile(null)}
                      >
                        {t("hosts.localKeyClear")}
                      </button>
                    </div>
                  )}
                  {credError && <p className="text-xs text-destructive">{credError}</p>}
                </div>
              )}
              {form.auth_kind === "keyring_password" && (
                <div className="col-span-2 flex flex-col gap-2">
                  <Label>{t("hosts.f.cred")}</Label>
                  <Select
                    value={form.credential_id || null}
                    items={Object.fromEntries(
                      credentials.map((c) => [
                        c.id,
                        c.username ? `${c.label} (${c.username})` : c.label,
                      ])
                    )}
                    onValueChange={(v) => setForm({ ...form, credential_id: v ?? "" })}
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue placeholder={t("hosts.selectCred")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        {credentials.map((c) => (
                          <SelectItem key={c.id} value={c.id}>
                            {c.label}
                            {c.username ? ` (${c.username})` : ""}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                  {!newCred ? (
                    <Button
                      variant="outline"
                      size="xs"
                      className="w-fit"
                      onClick={() => setNewCred({ label: "", username: "root", secret: "" })}
                    >
                      <Plus data-icon="inline-start" />
                      {t("hosts.newCred")}
                    </Button>
                  ) : (
                    <div className="flex flex-col gap-2 rounded-lg border border-border bg-secondary/40 p-3">
                      <Input
                        placeholder={t("hosts.phLabel")}
                        value={newCred.label}
                        onChange={(e) => setNewCred({ ...newCred, label: e.target.value })}
                      />
                      <Input
                        placeholder={t("hosts.phCredUser")}
                        value={newCred.username}
                        onChange={(e) => setNewCred({ ...newCred, username: e.target.value })}
                      />
                      <Input
                        type="password"
                        placeholder={t("hosts.phPassword")}
                        value={newCred.secret}
                        onChange={(e) => setNewCred({ ...newCred, secret: e.target.value })}
                      />
                      {credError && <p className="text-xs text-destructive">{credError}</p>}
                      <div className="flex gap-2">
                        <Button
                          size="xs"
                          onClick={() => void createCredential()}
                          disabled={!newCred.label || !newCred.secret}
                        >
                          {t("hosts.createSelect")}
                        </Button>
                        <Button size="xs" variant="outline" onClick={() => setNewCred(null)}>
                          {t("common.cancel")}
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              )}
              <div className="flex flex-col gap-1.5">
                <Label>{t("hosts.jumpHost")}</Label>
                <Select
                  value={form.jump_host_id || "__none__"}
                  items={{
                    __none__: t("hosts.jumpNone"),
                    ...Object.fromEntries(jumpOptions.map((s) => [s.id, s.name])),
                  }}
                  onValueChange={(v) =>
                    setForm({ ...form, jump_host_id: !v || v === "__none__" ? "" : v })
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="__none__">{t("hosts.jumpNone")}</SelectItem>
                      {jumpOptions.map((s) => (
                        <SelectItem key={s.id} value={s.id}>
                          {s.name}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
              {form.jump_host_id && (
                <div className="flex flex-col gap-1.5">
                  <Label>{t("hosts.jumpMode")}</Label>
                  <Select
                    value={form.jump_mode}
                    items={{
                      direct_tcpip: t("hosts.jumpModeDirect"),
                      su_ssh: t("hosts.jumpModeSuSSH"),
                    }}
                    onValueChange={(v) =>
                      setForm({
                        ...form,
                        jump_mode: (v || "direct_tcpip") as NonNullable<ServerAsset["jump_mode"]>,
                      })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        <SelectItem value="direct_tcpip">{t("hosts.jumpModeDirect")}</SelectItem>
                        <SelectItem value="su_ssh">{t("hosts.jumpModeSuSSH")}</SelectItem>
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </div>
              )}
              {form.jump_host_id && form.jump_mode === "su_ssh" && (
                <div className="col-span-2 grid grid-cols-2 gap-3 rounded-md border p-3 bg-muted/20">
                  <div className="flex flex-col gap-1.5">
                    <Label>{t("hosts.jumpSuUser")}</Label>
                    <Input
                      value={form.jump_su_username}
                      onChange={(e) => setForm({ ...form, jump_su_username: e.target.value })}
                      placeholder={t("hosts.jumpSuUserPlaceholder")}
                    />
                  </div>
                  <div className="flex flex-col gap-1.5">
                    <Label>{t("hosts.jumpSuCredential")}</Label>
                    <Select
                      value={form.jump_su_credential_id || "__none__"}
                      items={{
                        __none__: t("hosts.jumpSuCredNone"),
                        __custom__: t("hosts.jumpSuCredInput"),
                        ...Object.fromEntries(credentials.map((c) => [c.id, c.label])),
                      }}
                      onValueChange={(v) => {
                        const next = !v || v === "__none__" ? "" : v;
                        if (next !== "__custom__") {
                          setJumpSuPassword("");
                        }
                        setForm({
                          ...form,
                          jump_su_credential_id: next,
                        });
                      }}
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          <SelectItem value="__none__">{t("hosts.jumpSuCredNone")}</SelectItem>
                          <SelectItem value="__custom__">{t("hosts.jumpSuCredInput")}</SelectItem>
                          {credentials.map((c) => (
                            <SelectItem key={c.id} value={c.id}>
                              {c.label}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  </div>
                  {form.jump_su_credential_id === "__custom__" && (
                    <div className="col-span-2 flex flex-col gap-1.5">
                      <Input
                        type="password"
                        value={jumpSuPassword}
                        onChange={(e) => setJumpSuPassword(e.target.value)}
                        placeholder={t("hosts.jumpSuPasswordPlaceholder")}
                      />
                    </div>
                  )}
                </div>
              )}
              <div className={cn("flex flex-col gap-1.5", form.jump_host_id && "col-span-2")}>
                <Label>{t("hosts.f.tags")}</Label>
                <Input
                  value={form.tags}
                  onChange={(e) => setForm({ ...form, tags: e.target.value })}
                />
              </div>
              <div className="col-span-2 flex items-center justify-between rounded-lg border border-border px-3 py-2">
                <Label htmlFor="host-enabled">{t("common.enabled")}</Label>
                <Switch
                  id="host-enabled"
                  checked={form.enabled}
                  onCheckedChange={(v) => setForm({ ...form, enabled: v })}
                />
              </div>
              <div className="col-span-2">
                <ErrorText>{error}</ErrorText>
              </div>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void saveForm()}>{t("common.save")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Import ssh config / CSV */}
      <Dialog open={importOpen} onOpenChange={setImportOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{importTab === "csv" ? t("hosts.csvImport") : t("hosts.importTitle")}</DialogTitle>
          </DialogHeader>
          <Tabs value={importTab} onValueChange={(v) => setImportTab(v === "csv" ? "csv" : "ssh")}>
            <TabsList>
              <TabsTrigger value="ssh">{t("hosts.import")}</TabsTrigger>
              <TabsTrigger value="csv">{t("hosts.csvImport")}</TabsTrigger>
            </TabsList>
            <TabsContent value="ssh">
              <div className="flex flex-col gap-2">
                <Label>{t("hosts.importPaste")}</Label>
                <Textarea
                  className="h-48 font-mono text-xs"
                  value={importText}
                  onChange={(e) => setImportText(e.target.value)}
                  placeholder={"Host bastion\n  HostName 10.0.0.9\n  User ops\n\nHost web-*\n  HostName 10.0.0.%h"}
                />
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  {t("hosts.importHint")}
                  <label className="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-border px-2.5 py-1 font-medium text-foreground hover:bg-muted">
                    {t("hosts.importPick")}
                    <input
                      type="file"
                      className="hidden"
                      accept=".conf,.cfg,.txt,text/plain"
                      onChange={(e) => void pickFile(e.target.files?.[0])}
                    />
                  </label>
                </div>
              </div>
            </TabsContent>
            <TabsContent value="csv">
              <div className="flex flex-col gap-2">
                <Label>{t("hosts.csvFormat")}</Label>
                <Textarea
                  className="h-48 font-mono text-xs"
                  value={csvText}
                  onChange={(e) => setCsvText(e.target.value)}
                  placeholder={t("hosts.csvPlaceholder")}
                />
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  {csvText.trim() ? t("hosts.csvParsed", { count: csvRows.length }) : t("hosts.csvFormat")}
                  <label className="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-border px-2.5 py-1 font-medium text-foreground hover:bg-muted">
                    {t("hosts.importPick")}
                    <input
                      type="file"
                      className="hidden"
                      accept=".csv,text/csv,text/plain"
                      onChange={(e) => void pickCsvFile(e.target.files?.[0])}
                    />
                  </label>
                </div>
              </div>
            </TabsContent>
          </Tabs>
          <DialogFooter>
            <Button variant="outline" onClick={() => setImportOpen(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={importTab === "csv" ? () => void doCsvImport() : () => void doImport()}
              disabled={importTab === "csv" ? csvBusy || csvRows.length === 0 : !importText.trim()}
            >
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Single host delete confirmation */}
      <ConfirmDialog
        open={deleteHostId !== null}
        title={t("common.delete")}
        description={t("hosts.deleteConfirm")}
        destructive
        onConfirm={() => deleteHostId && void remove(deleteHostId)}
        onOpenChange={(o) => !o && setDeleteHostId(null)}
      />

      {/* Bulk delete confirmation */}
      <ConfirmDialog
        open={bulkDeleteOpen}
        title={t("hosts.bulk.delete")}
        description={t("hosts.confirmBulkDelete", { count: selectedIds.size })}
        destructive
        loading={bulkBusy}
        onConfirm={() => void bulkDelete()}
        onOpenChange={setBulkDeleteOpen}
      />
    </Page>
  );
}
