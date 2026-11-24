import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronRight,
  CornerLeftUp,
  Download,
  File,
  Folder,
  FolderOpen,
  FolderPlus,
  Pencil,
  RefreshCw,
  Trash2,
  Upload,
} from "lucide-react";
import { cn } from "cn";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ErrorText } from "@/components/ui/shared";
import { ConfirmDialog, PromptDialog } from "@/components/bulwark/ConfirmDialog";
import { api, ApiError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { SftpEntry, SftpListResult, SftpUploadResult } from "@/types";

function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  let value = bytes;
  let unit = -1;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 100 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

function joinPath(dir: string, name: string): string {
  const base = dir === "/" ? "" : dir.replace(/\/+$/, "");
  return `${base}/${name}`;
}

function parentOf(path: string): string | null {
  if (!path || path === "/") return null;
  const trimmed = path.replace(/\/+$/, "");
  const idx = trimmed.lastIndexOf("/");
  return idx <= 0 ? "/" : trimmed.slice(0, idx);
}

const ICON_ACTION =
  "inline-flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground";

export default function FilesManager({ serverId, serverName }: { serverId: string; serverName: string }) {
  const { t } = useI18n();
  const [cwd, setCwd] = useState("");
  const [entries, setEntries] = useState<SftpEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<string[] | null>(null);
  const [renameTarget, setRenameTarget] = useState<SftpEntry | null>(null);
  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const dragDepth = useRef(0);
  const uploadInputRef = useRef<HTMLInputElement>(null);

  const describeError = (err: unknown) => {
    if (err instanceof ApiError && err.status === 403) return t("files.forbidden");
    return err instanceof Error ? err.message : t("files.opFailed");
  };

  const load = useCallback(
    async (path: string) => {
      setLoading(true);
      setError(null);
      try {
        const res = await api.get<SftpListResult>(
          `/api/servers/${serverId}/files?path=${encodeURIComponent(path)}`
        );
        setCwd(res.path || path);
        setEntries(res.entries ?? []);
        setSelected(new Set());
      } catch (err) {
        setError(err);
        setEntries([]);
      } finally {
        setLoading(false);
      }
    },
    [serverId]
  );

  useEffect(() => {
    void load(".");
  }, [load]);

  const parentPath = useMemo(() => parentOf(cwd), [cwd]);

  const crumbs = useMemo(() => {
    if (!cwd) return [];
    const parts = cwd.split("/").filter(Boolean);
    return parts.map((part, i) => ({
      name: part,
      path: `/${parts.slice(0, i + 1).join("/")}`,
    }));
  }, [cwd]);

  const sorted = useMemo(
    () =>
      [...entries].sort((a, b) => {
        if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
        return a.name.localeCompare(b.name);
      }),
    [entries]
  );

  const toggleSelected = (path: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const allChecked = sorted.length > 0 && sorted.every((e) => selected.has(e.path));

  const toggleAll = (checked: boolean) => {
    setSelected(checked ? new Set(sorted.map((e) => e.path)) : new Set());
  };

  const uploadFiles = async (list: FileList | null | undefined) => {
    const files = list ? Array.from(list) : [];
    if (files.length === 0 || !cwd) return;
    setBusy(true);
    setError(null);
    try {
      const form = new FormData();
      form.append("path", cwd);
      for (const f of files) form.append("files[]", f);
      const resp = await fetch(`/api/servers/${serverId}/files/upload`, {
        method: "POST",
        credentials: "same-origin",
        body: form,
      });
      const envelope = (await resp.json().catch(() => ({}))) as {
        code?: number;
        message?: string;
        data?: SftpUploadResult;
      };
      if (!resp.ok || (envelope.code ?? 0) !== 0) {
        throw new ApiError(envelope.code ?? 50001, resp.status, envelope.message ?? t("files.uploadFailed"));
      }
      const failures = (envelope.data?.results ?? []).filter((r) => r.error);
      if (failures.length > 0 || envelope.data?.failed) {
        setError(`${t("files.uploadFailed")}: ${failures[0]?.error ?? failures.length}`);
      }
      await load(cwd);
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };

  const doDelete = async () => {
    const paths = pendingDelete;
    if (!paths || paths.length === 0) return;
    setBusy(true);
    try {
      const res = await api.post<{ results: { path: string; deleted?: boolean; error?: string }[]; failed: boolean }>(
        `/api/servers/${serverId}/files/delete`,
        { paths }
      );
      const failures = (res.results ?? []).filter((r) => r.error);
      if (failures.length > 0) {
        setError(`${t("files.opFailed")}: ${failures[0].error ?? failures.length}`);
      }
      setPendingDelete(null);
      await load(cwd || ".");
    } catch (err) {
      setError(err);
      setPendingDelete(null);
    } finally {
      setBusy(false);
    }
  };

  const doRename = async (values: Record<string, string>) => {
    const target = renameTarget;
    if (!target) return;
    const newName = (values.name ?? "").trim();
    if (!newName || newName === target.name) {
      setRenameTarget(null);
      return;
    }
    setBusy(true);
    try {
      await api.post(`/api/servers/${serverId}/files/rename`, {
        from: target.path,
        to: joinPath(parentOf(target.path) ?? "/", newName),
      });
      setRenameTarget(null);
      await load(cwd || ".");
    } catch (err) {
      setError(err);
      setRenameTarget(null);
    } finally {
      setBusy(false);
    }
  };

  const doMkdir = async (values: Record<string, string>) => {
    const name = (values.name ?? "").trim();
    if (!name || !cwd) return;
    setBusy(true);
    try {
      await api.post(`/api/servers/${serverId}/files/mkdir`, { path: joinPath(cwd, name) });
      setNewFolderOpen(false);
      await load(cwd);
    } catch (err) {
      setError(err);
      setNewFolderOpen(false);
    } finally {
      setBusy(false);
    }
  };

  const modTime = (e: SftpEntry) => {
    if (!e.mod_time) return "—";
    const d = new Date(e.mod_time);
    return Number.isNaN(d.getTime()) ? e.mod_time : d.toLocaleString();
  };

  return (
    <div
      className="relative flex min-h-0 flex-1 flex-col"
      onDragEnter={(e) => {
        e.preventDefault();
        dragDepth.current += 1;
        setDragging(true);
      }}
      onDragOver={(e) => e.preventDefault()}
      onDragLeave={(e) => {
        e.preventDefault();
        dragDepth.current = Math.max(0, dragDepth.current - 1);
        if (dragDepth.current === 0) setDragging(false);
      }}
      onDrop={(e) => {
        e.preventDefault();
        dragDepth.current = 0;
        setDragging(false);
        void uploadFiles(e.dataTransfer?.files);
      }}
    >
      {/* Toolbar */}
      <div className="flex shrink-0 items-center gap-1.5 border-b border-border bg-card/80 px-3 py-2">
        <Button
          variant="outline"
          size="icon-sm"
          title={t("files.parent")}
          disabled={!parentPath || loading}
          onClick={() => parentPath && void load(parentPath)}
        >
          <CornerLeftUp />
        </Button>

        <nav className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto" aria-label={serverName}>
          <button
            type="button"
            onClick={() => void load("/")}
            className="shrink-0 rounded-md bg-secondary/60 px-2 py-1 font-mono text-xs font-medium text-foreground hover:bg-secondary"
          >
            {serverName}
          </button>
          {crumbs.map((crumb, i) => {
            const isLast = i === crumbs.length - 1;
            return (
              <span key={crumb.path} className="flex shrink-0 items-center gap-0.5">
                <ChevronRight className="size-3 text-muted-foreground" />
                <button
                  type="button"
                  onClick={() => !isLast && void load(crumb.path)}
                  disabled={isLast}
                  className={cn(
                    "rounded-md px-1.5 py-1 font-mono text-xs",
                    isLast
                      ? "bg-secondary/60 font-medium text-foreground"
                      : "text-muted-foreground hover:bg-muted hover:text-foreground"
                  )}
                >
                  {crumb.name}
                </button>
              </span>
            );
          })}
        </nav>

        {selected.size > 0 && (
          <Button
            variant="destructive"
            size="icon-sm"
            title={t("files.delete")}
            disabled={busy}
            onClick={() => setPendingDelete([...selected])}
          >
            <Trash2 />
          </Button>
        )}
        <Button
          variant="outline"
          size="icon-sm"
          title={t("files.newFolder")}
          disabled={busy || !cwd}
          onClick={() => setNewFolderOpen(true)}
        >
          <FolderPlus />
        </Button>
        <Button
          variant="outline"
          size="icon-sm"
          title={t("files.upload")}
          disabled={busy || !cwd}
          onClick={() => uploadInputRef.current?.click()}
        >
          <Upload />
        </Button>
        <Button
          variant="outline"
          size="icon-sm"
          title={t("files.refresh")}
          disabled={busy || !cwd}
          onClick={() => void load(cwd)}
        >
          <RefreshCw />
        </Button>
        <input
          ref={uploadInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={(e) => {
            void uploadFiles(e.target.files);
            e.target.value = "";
          }}
        />
      </div>

      {/* Content */}
      <div className="min-h-0 flex-1 overflow-y-auto p-4 sm:p-6">
        {error ? <ErrorText>{describeError(error)}</ErrorText> : null}

        {loading ? (
          <div className="flex min-h-48 items-center justify-center text-sm text-muted-foreground">
            {t("common.loading")}
          </div>
        ) : sorted.length === 0 ? (
          <Empty className="border border-border bg-card py-14">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <FolderOpen />
              </EmptyMedia>
              <EmptyTitle>{t("files.title")}</EmptyTitle>
              <EmptyDescription>{t("files.empty")}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <Card className="p-0">
            <Table>
              <TableHeader className="bg-secondary/50">
                <TableRow>
                  <TableHead className="w-10 pl-4">
                    <input
                      type="checkbox"
                      aria-label={t("common.all")}
                      checked={allChecked}
                      onChange={(e) => toggleAll(e.target.checked)}
                    />
                  </TableHead>
                  <TableHead className="px-3 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("files.colName")}
                  </TableHead>
                  <TableHead className="w-28 px-3 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("files.colSize")}
                  </TableHead>
                  <TableHead className="hidden w-32 px-3 font-mono text-[11px] uppercase text-muted-foreground sm:table-cell">
                    {t("files.colMode")}
                  </TableHead>
                  <TableHead className="w-48 px-3 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("files.colModified")}
                  </TableHead>
                  <TableHead className="w-28 px-4" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {sorted.map((e) => (
                  <TableRow
                    key={e.path}
                    className={cn("group/file", e.is_dir && "cursor-pointer")}
                    onClick={e.is_dir ? () => void load(e.path) : undefined}
                  >
                    <TableCell className="w-10 pl-4" onClick={(ev) => ev.stopPropagation()}>
                      <input
                        type="checkbox"
                        aria-label={e.name}
                        checked={selected.has(e.path)}
                        onChange={() => toggleSelected(e.path)}
                      />
                    </TableCell>
                    <TableCell className="max-w-0 px-3 py-2.5">
                      <div className="flex min-w-0 items-center gap-2">
                        {e.is_dir ? (
                          <Folder className="size-4 shrink-0 text-geist-blue" />
                        ) : (
                          <File className="size-4 shrink-0 text-muted-foreground" />
                        )}
                        <span className="truncate font-medium text-foreground">{e.name}</span>
                        {e.target && (
                          <span className="hidden min-w-0 truncate font-mono text-[11px] text-muted-foreground lg:inline">
                            → {e.target}
                          </span>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="w-28 px-3 py-2.5 font-mono text-xs text-muted-foreground tabular-nums">
                      {e.is_dir ? "—" : formatSize(e.size)}
                    </TableCell>
                    <TableCell className="hidden w-32 px-3 py-2.5 font-mono text-xs text-muted-foreground sm:table-cell">
                      {e.mode || "—"}
                    </TableCell>
                    <TableCell className="w-48 px-3 py-2.5 font-mono text-xs text-muted-foreground">
                      {modTime(e)}
                    </TableCell>
                    <TableCell className="w-28 px-4 py-2.5 text-right" onClick={(ev) => ev.stopPropagation()}>
                      <div className="flex justify-end gap-0.5 opacity-0 transition-opacity group-focus-within/file:opacity-100 group-hover/file:opacity-100">
                        {!e.is_dir && (
                          <a
                            href={`/api/servers/${serverId}/files/download?path=${encodeURIComponent(e.path)}`}
                            download={e.name}
                            title={t("files.download")}
                            className={ICON_ACTION}
                          >
                            <Download className="size-3.5" />
                          </a>
                        )}
                        <button type="button" title={t("files.rename")} className={ICON_ACTION} onClick={() => setRenameTarget(e)}>
                          <Pencil className="size-3.5" />
                        </button>
                        <button
                          type="button"
                          title={t("files.delete")}
                          className={cn(ICON_ACTION, "hover:text-destructive")}
                          onClick={() => setPendingDelete([e.path])}
                        >
                          <Trash2 className="size-3.5" />
                        </button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </Card>
        )}
      </div>

      {/* Drag & drop overlay */}
      {dragging && (
        <div className="pointer-events-none absolute inset-0 z-30 flex items-center justify-center bg-background/80 p-6">
          <div className="flex flex-col items-center gap-2 rounded-xl border-2 border-dashed border-geist-blue bg-card px-10 py-12 text-sm text-muted-foreground">
            <Upload className="size-6 text-geist-blue" />
            {t("files.dropHint")}
          </div>
        </div>
      )}

      {/* Delete confirmation */}
      <ConfirmDialog
        open={pendingDelete !== null}
        title={t("files.delete")}
        description={t("files.confirmDelete", { count: pendingDelete?.length ?? 0 })}
        destructive
        loading={busy}
        onConfirm={() => void doDelete()}
        onOpenChange={(open) => !open && setPendingDelete(null)}
      />

      {/* Rename */}
      <PromptDialog
        open={renameTarget !== null}
        title={t("files.renameTitle", { name: renameTarget?.name ?? "" })}
        fields={[
          {
            key: "name",
            label: t("files.renameTo"),
            placeholder: renameTarget?.name ?? "",
            defaultValue: renameTarget?.name ?? "",
          },
        ]}
        loading={busy}
        onSubmit={(values) => void doRename(values)}
        onOpenChange={(open) => !open && setRenameTarget(null)}
      />

      {/* New folder */}
      <PromptDialog
        open={newFolderOpen}
        title={t("files.newFolder")}
        fields={[{ key: "name", label: t("files.newFolderName") }]}
        loading={busy}
        onSubmit={(values) => void doMkdir(values)}
        onOpenChange={setNewFolderOpen}
      />
    </div>
  );
}
