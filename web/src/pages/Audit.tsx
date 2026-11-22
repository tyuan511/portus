import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  FileJson,
  FileSpreadsheet,
  Loader2,
  Pause,
  Play,
  X,
} from "lucide-react";
import { cn } from "cn";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Pagination } from "@/components/sshield/Pagination";
import { StatusPill, decisionTone } from "@/components/sshield/StatusDot";
import { TerminalReplay, fmtBytes, fmtDuration } from "@/components/sshield/TerminalReplay";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type {
  AuditEvent,
  AuditStats,
  MCPSession,
  Paged,
  ServerAsset,
  TerminalSession,
} from "@/types";

const PAGE_SIZE = 50;
const EXPORT_PAGE = 1000; // page size while pulling everything for an export
const EXPORT_MAX_ROWS = 20000; // hard cap so a huge log can't lock the tab

const ACTORS = ["mcp", "terminal", "manual", "sftp", "console"] as const;
const DECISIONS = ["allow", "ask_approval", "deny"] as const;

interface AuditFilters {
  actor: string;
  decision: string;
  serverId: string;
  username: string;
  from: string; // datetime-local draft value
  to: string;
  onlyFailed: boolean;
}

const EMPTY_FILTERS: AuditFilters = {
  actor: "",
  decision: "",
  serverId: "",
  username: "",
  from: "",
  to: "",
  onlyFailed: false,
};

const decisionVariant = (d: string) =>
  (d === "allow" ? "default" : d === "deny" ? "destructive" : "secondary") as
    | "default"
    | "destructive"
    | "secondary";

// datetime-local draft value → ISO 8601 UTC for the API.
const toIso = (value: string): string => {
  if (!value) return "";
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? "" : d.toISOString();
};

function buildAuditParams(q: string, f: AuditFilters, limit?: number, offset?: number): URLSearchParams {
  const p = new URLSearchParams();
  if (q.trim()) p.set("q", q.trim());
  if (f.actor) p.set("actor", f.actor);
  if (f.decision) p.set("decision", f.decision);
  if (f.serverId) p.set("server_id", f.serverId);
  if (f.username.trim()) p.set("username", f.username.trim());
  const from = toIso(f.from);
  const to = toIso(f.to);
  if (from) p.set("from", from);
  if (to) p.set("to", to);
  if (f.onlyFailed) p.set("only_failed", "true");
  if (limit !== undefined) p.set("limit", String(limit));
  if (offset !== undefined) p.set("offset", String(offset));
  return p;
}

function csvEscape(value: unknown): string {
  const s = value === null || value === undefined ? "" : String(value);
  return /[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

function toCsv(rows: AuditEvent[]): string {
  const header = [
    "id",
    "timestamp",
    "actor",
    "username",
    "ssh_user",
    "server_name",
    "command",
    "decision",
    "exit_code",
    "duration_ms",
    "error",
  ];
  const lines = [header.join(",")];
  for (const ev of rows) {
    lines.push(
      [
        ev.id,
        ev.timestamp,
        ev.actor,
        ev.username ?? "",
        ev.ssh_user ?? "",
        ev.server_name,
        ev.command,
        ev.decision,
        ev.exit_code ?? "",
        ev.duration_ms ?? "",
        ev.error ?? "",
      ]
        .map(csvEscape)
        .join(","),
    );
  }
  // BOM keeps Excel happy with UTF-8.
  return `\uFEFF${lines.join("\r\n")}`;
}

function downloadFile(name: string, mime: string, content: string) {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

function exportStamp(): string {
  const d = new Date();
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}${mm}${dd}`;
}

interface StatRow {
  label: string;
  count: number;
  failed?: number;
}

// One horizontal bar list; the failed share (if any) renders in destructive red.
function StatBars({ title, rows }: { title: string; rows: StatRow[] }) {
  const shown = rows.slice(0, 6);
  const max = Math.max(1, ...shown.map((r) => r.count));
  return (
    <div className="rounded-lg border p-3">
      <div className="mb-2 text-xs font-medium text-muted-foreground">{title}</div>
      {shown.length === 0 ? (
        <p className="text-xs text-muted-foreground">—</p>
      ) : (
        <div className="space-y-1.5">
          {shown.map((r) => {
            const failed = Math.max(0, Math.min(r.failed ?? 0, r.count));
            return (
              <div key={r.label} className="flex items-center gap-2">
                <span className="w-28 shrink-0 truncate text-xs" title={r.label}>
                  {r.label || "—"}
                </span>
                <div className="flex h-2.5 min-w-0 flex-1 overflow-hidden rounded-sm bg-muted/60">
                  <div
                    className="h-full bg-foreground/20"
                    style={{ width: `${Math.min(100, ((r.count - failed) / max) * 100)}%` }}
                  />
                  {failed > 0 && (
                    <div className="h-full bg-destructive/40" style={{ width: `${Math.min(100, (failed / max) * 100)}%` }} />
                  )}
                </div>
                <span className="w-12 shrink-0 text-right font-mono text-xs text-muted-foreground">{r.count}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function TopCommands({ title, rows }: { title: string; rows: { key: string; count: number }[] }) {
  const shown = rows.slice(0, 8);
  return (
    <div className="rounded-lg border p-3">
      <div className="mb-2 text-xs font-medium text-muted-foreground">{title}</div>
      {shown.length === 0 ? (
        <p className="text-xs text-muted-foreground">—</p>
      ) : (
        <ol className="space-y-1">
          {shown.map((c) => (
            <li key={c.key} className="flex items-baseline justify-between gap-3 text-xs">
              <code className="truncate font-mono text-foreground/80" title={c.key}>
                {c.key}
              </code>
              <span className="shrink-0 font-mono text-muted-foreground">{c.count}</span>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

// Mini bar chart of the last 14 days; failed+denied stack on top in red.
function TrendChart({ title, trend }: { title: string; trend: AuditStats["daily_trend"] }) {
  const days = trend.slice(-14);
  const max = Math.max(1, ...days.map((d) => d.total));
  return (
    <div className="rounded-lg border p-3">
      <div className="mb-2 text-xs font-medium text-muted-foreground">{title}</div>
      {days.length === 0 ? (
        <p className="text-xs text-muted-foreground">—</p>
      ) : (
        <div className="flex h-20 items-end gap-1.5">
          {days.map((d) => {
            const totalPct = Math.min(100, (d.total / max) * 100);
            const badPct = d.total > 0 ? Math.min(totalPct, ((d.denied + d.failed) / d.total) * totalPct) : 0;
            return (
              <div
                key={d.date}
                className="flex min-w-0 flex-1 flex-col items-center gap-1"
                title={`${d.date} · ${d.total}`}
              >
                <div className="flex h-14 w-full max-w-7 flex-col justify-end overflow-hidden rounded-sm bg-muted/60">
                  <div className="w-full bg-destructive/40" style={{ height: `${badPct}%` }} />
                  <div className="w-full bg-foreground/20" style={{ height: `${totalPct - badPct}%` }} />
                </div>
                <span className="truncate font-mono text-[10px] text-muted-foreground">{d.date.slice(5)}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function FilterField({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex min-w-0 flex-col gap-1">
      <span className="text-[11px] font-medium text-muted-foreground">{label}</span>
      {children}
    </label>
  );
}

export default function Audit() {
  const auth = useAuth();
  const { t } = useI18n();
  const canViewAll = auth.hasPermission("audit.view_all");

  const [q, setQ] = useState("");
  const [filters, setFilters] = useState<AuditFilters>(EMPTY_FILTERS);
  const [appliedQ, setAppliedQ] = useState("");
  const [applied, setApplied] = useState<AuditFilters>(EMPTY_FILTERS);
  const [page, setPage] = useState(1);
  const [total, setTotal] = useState(0);
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [loading, setLoading] = useState(false);
  const [stats, setStats] = useState<AuditStats | null>(null);
  const [showStats, setShowStats] = useState(true);
  const [servers, setServers] = useState<ServerAsset[]>([]);
  const [exporting, setExporting] = useState<"csv" | "json" | null>(null);
  const [error, setError] = useState<string>();
  const [chain, setChain] = useState<{ ok: boolean; text: string }>();
  const [reloadSeq, setReloadSeq] = useState(0);
  const [detailEvent, setDetailEvent] = useState<AuditEvent | null>(null);
  const applyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Debounce drafts (search box, text inputs) before hitting the API.
  useEffect(() => {
    if (applyTimer.current) clearTimeout(applyTimer.current);
    applyTimer.current = setTimeout(() => {
      setAppliedQ(q);
      setApplied(filters);
      setPage(1);
    }, 300);
    return () => {
      if (applyTimer.current) clearTimeout(applyTimer.current);
    };
  }, [q, filters]);

  // Immediate apply (Enter in a text field); cancels the pending debounce.
  const applyNow = useCallback(() => {
    if (applyTimer.current) {
      clearTimeout(applyTimer.current);
      applyTimer.current = null;
    }
    setAppliedQ(q);
    setApplied(filters);
    setPage(1);
  }, [q, filters]);

  // Audit events page — the API returns a {items, total} envelope.
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    void (async () => {
      try {
        const params = buildAuditParams(appliedQ, applied, PAGE_SIZE, (page - 1) * PAGE_SIZE);
        const r = await api.get<Paged<AuditEvent> | AuditEvent[]>(`/api/audit-logs?${params.toString()}`);
        if (cancelled) return;
        const items = Array.isArray(r) ? r : (r?.items ?? []);
        const totalCount = Array.isArray(r) ? r.length : (r?.total ?? items.length);
        setEvents(items);
        setTotal(totalCount);
        setError(undefined);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : "load failed");
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [appliedQ, applied, page, reloadSeq]);

  // Aggregate stats follow the same filters (without paging).
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const params = buildAuditParams(appliedQ, applied);
        const s = await api.get<AuditStats>(`/api/audit-logs/stats?${params.toString()}`);
        if (!cancelled) setStats(s);
      } catch {
        /* stats are auxiliary; keep showing the last snapshot */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [appliedQ, applied, reloadSeq]);

  // Host dropdown options.
  useEffect(() => {
    api
      .get<ServerAsset[]>("/api/servers")
      .then(setServers)
      .catch(() => {
        /* the dropdown just stays empty */
      });
  }, []);

  const verify = async () => {
    try {
      const r = await api.get<{ valid: boolean; total_events: number; broken_at_id?: string }>("/api/audit-logs/verify");
      setChain(
        r.valid
          ? { ok: true, text: t("audit.chainValid", { count: r.total_events }) }
          : { ok: false, text: t("audit.chainTampered", { id: r.broken_at_id ?? "?" }) },
      );
    } catch (err) {
      setError(err instanceof Error ? err.message : "verify failed");
    }
  };

  // Pulls every row matching the current filters (paged, capped) and downloads it.
  const runExport = async (fmt: "csv" | "json") => {
    if (exporting !== null) return; // re-entry guard
    setExporting(fmt);
    try {
      const rows: AuditEvent[] = [];
      let offset = 0;
      for (;;) {
        const params = buildAuditParams(appliedQ, applied, EXPORT_PAGE, offset);
        const r = await api.get<Paged<AuditEvent> | AuditEvent[]>(`/api/audit-logs?${params.toString()}`);
        const items = Array.isArray(r) ? r : (r?.items ?? []);
        const totalCount = Array.isArray(r) ? r.length : (r?.total ?? items.length);
        rows.push(...items);
        if (items.length < EXPORT_PAGE || rows.length >= totalCount || rows.length >= EXPORT_MAX_ROWS) break;
        offset += items.length;
      }
      const name = `audit-${exportStamp()}.${fmt}`;
      if (fmt === "csv") downloadFile(name, "text/csv;charset=utf-8", toCsv(rows));
      else downloadFile(name, "application/json", JSON.stringify(rows, null, 2));
    } catch (err) {
      setError(`${t("audit.exportFailed")}: ${err instanceof Error ? err.message : "export failed"}`);
    } finally {
      setExporting(null);
    }
  };

  const clearFilters = () => {
    setQ("");
    setFilters(EMPTY_FILTERS);
    setAppliedQ("");
    setApplied(EMPTY_FILTERS);
    setPage(1);
  };

  return (
    <Page
      header={
        <PageHeader
          title={t("audit.title")}
          actions={
            <div className="flex items-center gap-2">
              <Input placeholder={t("common.search")} value={q} onChange={(e) => setQ(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && applyNow()} className="h-8 w-56" />
              <Button size="sm" variant="outline" onClick={() => setReloadSeq((s) => s + 1)}>{t("common.refresh")}</Button>
              {canViewAll && <Button size="sm" variant="outline" onClick={() => void verify()}>{t("audit.verify")}</Button>}
            </div>
          }
        />
      }
    >
      <Tabs defaultValue="events" className="gap-5">
        <TabsList variant="line" className="w-full justify-start border-b border-border pb-0">
          <TabsTrigger value="events">{t("audit.tabEvents")}</TabsTrigger>
          <TabsTrigger value="sessions">{t("audit.tabSessions")}</TabsTrigger>
          <TabsTrigger value="recordings">{t("audit.sessions.title")}</TabsTrigger>
        </TabsList>

        <TabsContent value="events" className="space-y-4">
          <ErrorText>{error}</ErrorText>
          {chain && (
            <div>
              <StatusPill label={chain.text} tone={chain.ok ? "success" : "danger"} />
            </div>
          )}

          {/* filters */}
          <Card className="p-3">
            <div className="flex flex-wrap items-end gap-2">
              <FilterField label={t("audit.filter.actor")}>
                <select className="h-8 w-32 rounded-lg border border-border bg-background px-2 text-xs" value={filters.actor}
                  onChange={(e) => setFilters((f) => ({ ...f, actor: e.target.value }))}>
                  <option value="">{t("common.all")}</option>
                  {ACTORS.map((a) => (
                    <option key={a} value={a}>{a}</option>
                  ))}
                </select>
              </FilterField>
              <FilterField label={t("audit.filter.decision")}>
                <select className="h-8 w-36 rounded-lg border border-border bg-background px-2 text-xs" value={filters.decision}
                  onChange={(e) => setFilters((f) => ({ ...f, decision: e.target.value }))}>
                  <option value="">{t("common.all")}</option>
                  {DECISIONS.map((d) => (
                    <option key={d} value={d}>{d}</option>
                  ))}
                </select>
              </FilterField>
              <FilterField label={t("audit.filter.server")}>
                <select className="h-8 w-44 rounded-lg border border-border bg-background px-2 text-xs" value={filters.serverId}
                  onChange={(e) => setFilters((f) => ({ ...f, serverId: e.target.value }))}>
                  <option value="">{t("common.all")}</option>
                  {servers.map((s) => (
                    <option key={s.id} value={s.id}>{s.name}</option>
                  ))}
                </select>
              </FilterField>
              <FilterField label={t("audit.filter.user")}>
                <Input className="h-8 w-36 text-xs" placeholder={t("common.search")} value={filters.username}
                  onChange={(e) => setFilters((f) => ({ ...f, username: e.target.value }))}
                  onKeyDown={(e) => e.key === "Enter" && applyNow()} />
              </FilterField>
              <FilterField label={t("audit.filter.from")}>
                <Input type="datetime-local" className="h-8 w-52 text-xs" value={filters.from}
                  onChange={(e) => setFilters((f) => ({ ...f, from: e.target.value }))} />
              </FilterField>
              <FilterField label={t("audit.filter.to")}>
                <Input type="datetime-local" className="h-8 w-52 text-xs" value={filters.to}
                  onChange={(e) => setFilters((f) => ({ ...f, to: e.target.value }))} />
              </FilterField>
            </div>
            <div className="mt-2 flex flex-wrap items-center justify-between gap-2 border-t border-border/70 pt-2">
              <div className="flex items-center gap-3">
                <label className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground">
                  <Switch size="sm" checked={filters.onlyFailed}
                    onCheckedChange={(v) => setFilters((f) => ({ ...f, onlyFailed: v }))} />
                  {t("audit.filter.failedOnly")}
                </label>
                <Button variant="ghost" size="xs" onClick={clearFilters}>
                  <X className="size-3" /> {t("common.clear")}
                </Button>
              </div>
              <div className="flex items-center gap-2">
                <Button variant="outline" size="xs" disabled={exporting !== null} onClick={() => void runExport("csv")}>
                  {exporting === "csv" ? <Loader2 className="size-3.5 animate-spin" /> : <FileSpreadsheet className="size-3.5" />}
                  {t("common.exportCsv")}
                </Button>
                <Button variant="outline" size="xs" disabled={exporting !== null} onClick={() => void runExport("json")}>
                  {exporting === "json" ? <Loader2 className="size-3.5 animate-spin" /> : <FileJson className="size-3.5" />}
                  {t("common.exportJson")}
                </Button>
              </div>
            </div>
          </Card>

          {/* stats */}
          {stats && (
            <Card className="overflow-hidden p-0">
              <button type="button" className="flex w-full items-center justify-between gap-2 px-4 py-3 text-left transition-colors hover:bg-muted/30"
                onClick={() => setShowStats((v) => !v)} aria-expanded={showStats}>
                <span className="text-sm font-semibold tracking-tight">{t("audit.stats.overview")}</span>
                <ChevronDown className={cn("size-4 text-muted-foreground transition-transform", showStats && "rotate-180")} />
              </button>
              {showStats && (
                <div className="space-y-3 border-t border-border p-3">
                  <div className="grid grid-cols-2 divide-x divide-y divide-border overflow-hidden rounded-lg border border-border bg-background sm:grid-cols-6 sm:divide-y-0">
                    {[
                      [t("audit.statTotal"), stats.total_commands],
                      [t("audit.statAllowed"), stats.allowed_count],
                      [t("audit.statApproved"), stats.approval_count],
                      [t("audit.statDenied"), stats.denied_count],
                      [t("audit.statFailed"), stats.failed_count],
                      [t("audit.statAvgMs"), Math.round(stats.avg_duration_ms ?? 0)],
                    ].map(([label, value]) => (
                      <div key={String(label)} className="px-3 py-2.5">
                        <div className="font-mono text-[10px] uppercase tracking-wider text-muted-foreground">{label}</div>
                        <div className="mt-0.5 text-lg font-semibold tabular-nums">{String(value)}</div>
                      </div>
                    ))}
                  </div>
                  <div className="grid gap-2 md:grid-cols-2">
                    <StatBars title={t("audit.stats.byActor")}
                      rows={(stats.by_actor ?? []).map((a) => ({ label: a.key, count: a.count }))} />
                    <StatBars title={t("audit.stats.byUser")}
                      rows={(stats.by_user ?? []).map((u) => ({ label: u.key, count: u.count }))} />
                    <StatBars title={t("audit.stats.byServer")}
                      rows={(stats.by_server ?? []).map((s) => ({ label: s.server_name || s.server_id, count: s.count, failed: s.failed_count }))} />
                    <TopCommands title={t("audit.stats.topCommands")} rows={stats.top_commands ?? []} />
                  </div>
                  <TrendChart title={t("audit.stats.trend")} trend={stats.daily_trend ?? []} />
                </div>
              )}
            </Card>
          )}

          <Card className={cn("p-0 transition-opacity", loading && "opacity-60")}>
            <Table>
              <TableHeader className="bg-secondary/50">
                <TableRow>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.colTime")}</TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.colUser")}</TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.colActor")}</TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.colServer")}</TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.colCommand")}</TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.colDecision")}</TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.colExit")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {events.map((ev) => (
                  <TableRow
                    key={ev.id}
                    className="cursor-pointer"
                    onClick={() => setDetailEvent(ev)}
                  >
                    <TableCell className="whitespace-nowrap px-4 font-mono text-xs text-muted-foreground tabular-nums">
                      {new Date(ev.timestamp).toLocaleString()}
                    </TableCell>
                    <TableCell className="px-4 font-medium">{ev.username || "—"}</TableCell>
                    <TableCell className="px-4">
                      <Badge variant="outline" className="font-mono text-[10px]">
                        {ev.actor}
                      </Badge>
                    </TableCell>
                    <TableCell className="px-4 text-xs">{ev.server_name || "—"}</TableCell>
                    <TableCell className="max-w-96 truncate px-4 font-mono text-xs" title={ev.command}>{ev.command}</TableCell>
                    <TableCell className="px-4">
                      <StatusPill label={ev.decision} tone={decisionTone(ev.decision)} />
                    </TableCell>
                    <TableCell className="px-4 font-mono text-xs tabular-nums">
                      {ev.exit_code != null ? (
                        <span className={ev.exit_code !== 0 ? "text-destructive" : "text-muted-foreground"}>
                          {ev.exit_code}
                        </span>
                      ) : (
                        "—"
                      )}
                    </TableCell>
                  </TableRow>
                ))}
                {events.length === 0 && !loading && (
                  <TableRow>
                    <TableCell colSpan={7} className="px-4 py-10 text-center text-sm text-muted-foreground">
                      {t("audit.empty")}
                    </TableCell>
                  </TableRow>
                )}
                {events.length === 0 && loading && (
                  <TableRow>
                    <TableCell colSpan={7} className="px-4 py-10 text-center text-sm text-muted-foreground">
                      …
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </Card>
          <Pagination page={page} pageSize={PAGE_SIZE} total={total} onPageChange={setPage} />
        </TabsContent>

        <TabsContent value="sessions">
          <SessionReplay />
        </TabsContent>

        <TabsContent value="recordings">
          <TerminalRecordings />
        </TabsContent>
      </Tabs>

      {/* Event detail inspector */}
      <Dialog open={detailEvent !== null} onOpenChange={(o) => !o && setDetailEvent(null)}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("audit.detailTitle")}</DialogTitle>
          </DialogHeader>
          {detailEvent && (
            <div className="flex flex-col gap-3 text-xs">
              <div className="flex flex-wrap items-center gap-2">
                <StatusPill label={detailEvent.decision} tone={decisionTone(detailEvent.decision)} />
                <Badge variant="outline" className="font-mono text-[10px]">
                  {detailEvent.actor}
                </Badge>
                {detailEvent.dry_run && <Badge variant="secondary">{t("audit.replay.detail.dryRun")}</Badge>}
                <span className="font-mono text-muted-foreground">
                  {new Date(detailEvent.timestamp).toLocaleString()}
                </span>
                {detailEvent.username && <span>· {detailEvent.username}</span>}
                {detailEvent.server_name && <span>· {detailEvent.server_name}</span>}
                {detailEvent.exit_code != null && <span>· exit {detailEvent.exit_code}</span>}
                {detailEvent.duration_ms != null && <span>· {detailEvent.duration_ms}ms</span>}
              </div>

              <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded-lg border border-border bg-[#09090b] p-3 font-mono text-xs text-[#ededed]">
                {detailEvent.command}
              </pre>

              {detailEvent.intent && (
                <p className="text-muted-foreground">
                  <span className="font-medium text-foreground">{t("audit.replay.detail.intent")}:</span>{" "}
                  {detailEvent.intent}
                </p>
              )}

              {detailEvent.l1_verdict?.matched_rules?.length ? (
                <p className="text-muted-foreground">
                  <span className="font-medium text-foreground">{t("audit.replay.detail.rules")}:</span>{" "}
                  {detailEvent.l1_verdict.matched_rules.join(", ")}
                </p>
              ) : null}

              {detailEvent.l2_verdict && (
                <p className="text-muted-foreground">
                  <span className="font-medium text-foreground">{t("audit.replay.detail.l2")}:</span>{" "}
                  {detailEvent.l2_verdict.reasoning} ({detailEvent.l2_verdict.risk_score}/100)
                </p>
              )}

              {detailEvent.stdout_preview && (
                <div>
                  <div className="mb-1 font-medium text-muted-foreground">{t("audit.replay.detail.output")}</div>
                  <pre className="max-h-52 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-border bg-[#09090b] p-3 font-mono text-xs text-[#ededed]">
                    {detailEvent.stdout_preview}
                  </pre>
                </div>
              )}

              {detailEvent.stderr_preview && (
                <div>
                  <div className="mb-1 font-medium text-muted-foreground">{t("audit.replay.detail.stderr")}</div>
                  <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-border bg-[#09090b] p-3 font-mono text-xs text-[#fca5a5]">
                    {detailEvent.stderr_preview}
                  </pre>
                </div>
              )}

              {detailEvent.error && <p className="text-destructive">{detailEvent.error}</p>}

              <div className="border-t border-border/70 pt-2 font-mono text-[10px] text-muted-foreground">
                sha256: {detailEvent.event_hash}
              </div>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </Page>
  );
}

// --- MCP session listing + replay ---

function SessionReplay() {
  const { t } = useI18n();
  const [data, setData] = useState<Paged<MCPSession> | null>(null);
  const [page, setPage] = useState(1);
  const [selected, setSelected] = useState<MCPSession | null>(null);
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [error, setError] = useState<string>();
  const [step, setStep] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [speedMs, setSpeedMs] = useState(1500);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  const loadSessions = useCallback(async (p: number) => {
    try {
      const r = await api.get<Paged<MCPSession>>(`/api/mcp-sessions?limit=${PAGE_SIZE}&offset=${(p - 1) * PAGE_SIZE}`);
      setData(r);
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void loadSessions(page);
  }, [loadSessions, page]);

  const open = async (s: MCPSession) => {
    try {
      const evs = await api.get<AuditEvent[]>(`/api/mcp-sessions/${encodeURIComponent(s.session_id)}/events`);
      setSelected(s);
      setEvents(evs);
      setStep(0);
      setPlaying(false);
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  };

  // Auto-play advances until the last step, then stops.
  useEffect(() => {
    if (timer.current) clearInterval(timer.current);
    if (playing && events.length > 0) {
      timer.current = setInterval(() => {
        setStep((s) => {
          if (s >= events.length - 1) {
            setPlaying(false);
            return s;
          }
          return s + 1;
        });
      }, speedMs);
    }
    return () => {
      if (timer.current) clearInterval(timer.current);
    };
  }, [playing, speedMs, events.length]);

  const sessions = data?.items ?? [];

  if (selected === null) {
    return (
      <div className="space-y-3">
        <ErrorText>{error}</ErrorText>
        <Card className="p-0">
          <Table>
            <TableHeader className="bg-secondary/50">
              <TableRow>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.session.colSession")}</TableHead>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.session.colUser")}</TableHead>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.session.colServers")}</TableHead>
                <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.session.colRange")}</TableHead>
                <TableHead className="px-4 text-right font-mono text-[11px] uppercase text-muted-foreground">{t("audit.session.colCount")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {sessions.map((s) => (
                <TableRow
                  key={s.session_id}
                  className="cursor-pointer"
                  onClick={() => void open(s)}
                >
                  <TableCell className="px-4 font-mono text-xs">{s.session_id}</TableCell>
                  <TableCell className="px-4 font-medium">{s.username || "—"}</TableCell>
                  <TableCell className="max-w-64 truncate px-4 text-xs text-muted-foreground" title={s.servers}>{s.servers || "—"}</TableCell>
                  <TableCell className="whitespace-nowrap px-4 font-mono text-xs text-muted-foreground tabular-nums">
                    {new Date(s.first_at).toLocaleString()} → {new Date(s.last_at).toLocaleTimeString()}
                  </TableCell>
                  <TableCell className="px-4 text-right font-mono text-xs font-medium tabular-nums">{s.event_count}</TableCell>
                </TableRow>
              ))}
              {data !== null && sessions.length === 0 && (
                <TableRow>
                  <TableCell colSpan={5} className="px-4 py-10 text-center text-sm text-muted-foreground">
                    {t("audit.sessions.empty")}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Card>
        <Pagination page={page} pageSize={PAGE_SIZE} total={data?.total ?? 0} onPageChange={setPage} />
      </div>
    );
  }

  const ev = events[step];
  const isLast = step >= events.length - 1;

  return (
    <div className="space-y-4">
      {/* summary + controls */}
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border p-4">
        <div>
          <Button variant="ghost" size="sm" className="mb-1 -ml-2" onClick={() => { setSelected(null); setPlaying(false); }}>
            <ChevronLeft className="size-4" /> {t("audit.replay.back")}
          </Button>
          <div className="font-mono text-xs text-muted-foreground">{selected.session_id}</div>
          <div className="text-sm">
            {selected.username || "—"} · {selected.servers || "—"} ·{" "}
            {t("audit.replay.summary.ops", { count: selected.event_count })}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="icon-sm" disabled={step === 0}
            onClick={() => { setPlaying(false); setStep((s) => Math.max(0, s - 1)); }}>
            <ChevronLeft className="size-4" />
          </Button>
          <span className="w-20 text-center text-sm text-muted-foreground">
            {t("audit.replay.step", { i: step + 1, n: events.length })}
          </span>
          <Button variant="outline" size="icon-sm" disabled={isLast}
            onClick={() => { setPlaying(false); setStep((s) => Math.min(events.length - 1, s + 1)); }}>
            <ChevronRight className="size-4" />
          </Button>
          <Button size="sm" disabled={events.length < 2} onClick={() => setPlaying((p) => !p)}>
            {playing ? <Pause className="size-4" /> : <Play className="size-4" />}
            {playing ? t("audit.replay.pause") : t("audit.replay.autoplay")}
          </Button>
          <select className="h-8 rounded-lg border bg-background px-2 text-xs" value={speedMs}
            onChange={(e) => setSpeedMs(Number(e.target.value))} title={t("audit.replay.speed")}>
            <option value={3000}>0.5×</option>
            <option value={1500}>1×</option>
            <option value={700}>2×</option>
            <option value={350}>4×</option>
          </select>
        </div>
      </div>

      {/* progress rail */}
      <div className="flex h-1.5 gap-0.5 overflow-hidden rounded-full">
        {events.map((e, i) => (
          <button key={e.id} className={`h-full flex-1 rounded-sm transition-colors ${
            i === step ? "bg-primary" : i < step ? "bg-primary/40" : "bg-muted"
          }`} onClick={() => { setPlaying(false); setStep(i); }} title={e.command} />
        ))}
      </div>

      {/* current event card */}
      {ev && (
        <div className="rounded-xl border p-4">
          <div className="mb-2 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <Badge variant={decisionVariant(ev.decision)}>{ev.decision}</Badge>
            {ev.dry_run && <Badge variant="secondary">{t("audit.replay.detail.dryRun")}</Badge>}
            <span>{new Date(ev.timestamp).toLocaleString()}</span>
            <span>· {ev.server_name || "—"}</span>
            {ev.exit_code != null && <span>· exit {ev.exit_code}</span>}
            {ev.duration_ms != null && <span>· {t("audit.replay.detail.duration")} {ev.duration_ms}ms</span>}
          </div>
          <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded-lg bg-muted/40 p-3 font-mono text-xs">{ev.command}</pre>
          {ev.intent && ev.command !== "mcp_session.open" && ev.command !== "mcp_session.close" && (
            <p className="mt-2 text-xs text-muted-foreground">{t("audit.replay.detail.intent")}: {ev.intent}</p>
          )}
          {ev.l1_verdict?.matched_rules?.length ? (
            <p className="mt-1 text-xs text-muted-foreground">
              {t("audit.replay.detail.rules")}: {ev.l1_verdict.matched_rules.join(", ")}
            </p>
          ) : null}
          {ev.l2_verdict && (
            <p className="mt-1 text-xs text-muted-foreground">
              {t("audit.replay.detail.l2")}: {ev.l2_verdict.reasoning}
            </p>
          )}
          {ev.stdout_preview && (
            <div className="mt-3">
              <div className="mb-1 text-xs font-medium text-muted-foreground">{t("audit.replay.detail.output")}</div>
              <pre className="max-h-56 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#1c1c1c] p-3 font-mono text-xs text-[#ededed]">{ev.stdout_preview}</pre>
            </div>
          )}
          {ev.stderr_preview && (
            <div className="mt-3">
              <div className="mb-1 text-xs font-medium text-muted-foreground">{t("audit.replay.detail.stderr")}</div>
              <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#1c1c1c] p-3 font-mono text-xs text-[#fca5a5]">{ev.stderr_preview}</pre>
            </div>
          )}
          {ev.error && <p className="mt-2 text-xs text-destructive">{ev.error}</p>}
        </div>
      )}
    </div>
  );
}

// --- Terminal recordings listing + xterm replay ---

function TerminalRecordings() {
  const { t } = useI18n();
  const [data, setData] = useState<Paged<TerminalSession> | null>(null);
  const [page, setPage] = useState(1);
  const [error, setError] = useState<string>();
  const [replay, setReplay] = useState<TerminalSession | null>(null);

  const load = useCallback(async (p: number) => {
    try {
      const r = await api.get<Paged<TerminalSession>>(
        `/api/terminal-sessions?limit=${PAGE_SIZE}&offset=${(p - 1) * PAGE_SIZE}`,
      );
      setData(r);
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void load(page);
  }, [load, page]);

  const sessions = data?.items ?? [];

  return (
    <div className="space-y-3">
      <ErrorText>{error}</ErrorText>
      <Card className="p-0">
        <Table>
          <TableHeader className="bg-secondary/50">
            <TableRow>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.sessions.colUser")}</TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.sessions.colHost")}</TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.sessions.colMode")}</TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.sessions.colOpened")}</TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.sessions.colDuration")}</TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">{t("audit.sessions.colSize")}</TableHead>
              <TableHead className="px-4" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {sessions.map((s) => (
              <TableRow key={s.id}>
                <TableCell className="px-4 font-medium">{s.username || "—"}</TableCell>
                <TableCell className="px-4 text-xs">
                  <span className="font-medium text-foreground">{s.server_name || "—"}</span>
                  {s.server_host && (
                    <span className="ml-1.5 font-mono text-[11px] text-muted-foreground">
                      {s.ssh_user ? `${s.ssh_user}@${s.server_host}` : s.server_host}
                    </span>
                  )}
                </TableCell>
                <TableCell className="px-4">
                  <Badge variant={s.policy_mode === "enforce" ? "default" : "secondary"}>{s.policy_mode}</Badge>
                </TableCell>
                <TableCell className="whitespace-nowrap px-4 font-mono text-xs text-muted-foreground tabular-nums">
                  {new Date(s.opened_at).toLocaleString()}
                </TableCell>
                <TableCell className="px-4 font-mono text-xs tabular-nums">
                  {s.closed_at ? fmtDuration(Date.parse(s.closed_at) - Date.parse(s.opened_at)) : "—"}
                </TableCell>
                <TableCell className="whitespace-nowrap px-4">
                  <span className="font-mono text-xs tabular-nums">{fmtBytes(s.byte_size)}</span>
                  {s.truncated && <Badge variant="destructive" className="ml-2">{t("audit.sessions.truncated")}</Badge>}
                </TableCell>
                <TableCell className="px-4 text-right">
                  <Button variant="outline" size="xs" onClick={() => setReplay(s)} title={t("audit.sessions.replay")}>
                    <Play data-icon="inline-start" />
                    {t("audit.sessions.replay")}
                  </Button>
                </TableCell>
              </TableRow>
            ))}
            {data !== null && sessions.length === 0 && (
              <TableRow>
                <TableCell colSpan={7} className="px-4 py-10 text-center text-sm text-muted-foreground">
                  {t("audit.sessions.terminalEmpty")}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </Card>
      <Pagination page={page} pageSize={PAGE_SIZE} total={data?.total ?? 0} onPageChange={setPage} />

      <TerminalReplay session={replay} open={replay !== null} onOpenChange={(o) => !o && setReplay(null)} />
    </div>
  );
}
