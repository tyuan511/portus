import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronLeft, ChevronRight, Pause, Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { AuditStats, AuditEvent, MCPSession } from "@/types";

const decisionVariant = (d: string) => (d === "allow" ? "default" : d === "deny" ? "destructive" : "secondary") as "default" | "destructive" | "secondary";

export default function Audit() {
  const auth = useAuth();
  const { t } = useI18n();
  const canViewAll = auth.hasPermission("audit.view_all");
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [stats, setStats] = useState<AuditStats | null>(null);
  const [q, setQ] = useState("");
  const [error, setError] = useState<string>();
  const [chain, setChain] = useState<{ ok: boolean; text: string }>();

  const load = useCallback(async () => {
    try {
      const query = q ? `?q=${encodeURIComponent(q)}` : "";
      setEvents(await api.get<AuditEvent[]>(`/api/audit-logs${query}`));
      setStats(await api.get<AuditStats>(`/api/audit-logs/stats${query}`));
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, [q]);

  useEffect(() => {
    void load();
  }, [load]);

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

  return (
    <Page
      header={
        <PageHeader
          title={t("audit.title")}
          actions={
            <div className="flex items-center gap-2">
              <Input placeholder={t("common.search")} value={q} onChange={(e) => setQ(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && void load()} className="h-8 w-56" />
              <Button size="sm" variant="outline" onClick={() => void load()}>{t("common.refresh")}</Button>
              {canViewAll && <Button size="sm" variant="outline" onClick={() => void verify()}>{t("audit.verify")}</Button>}
            </div>
          }
        />
      }
    >
      <Tabs defaultValue="events">
        <TabsList>
          <TabsTrigger value="events">{t("audit.tabEvents")}</TabsTrigger>
          <TabsTrigger value="sessions">{t("audit.tabSessions")}</TabsTrigger>
        </TabsList>

        <TabsContent value="events" className="space-y-4">
          <ErrorText>{error}</ErrorText>
          {chain && <p className={`text-sm ${chain.ok ? "text-primary" : "text-destructive"}`}>{chain.text}</p>}

          {stats && (
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-6">
              {[
                [t("audit.statTotal"), stats.total_commands],
                [t("audit.statAllowed"), stats.allowed_count],
                [t("audit.statApproved"), stats.approval_count],
                [t("audit.statDenied"), stats.denied_count],
                [t("audit.statFailed"), stats.failed_count],
                [t("audit.statAvgMs"), Math.round(stats.avg_duration_ms ?? 0)],
              ].map(([label, value]) => (
                <div key={String(label)} className="rounded-lg border p-2.5">
                  <div className="text-xs text-muted-foreground">{label}</div>
                  <div className="text-lg font-semibold">{String(value)}</div>
                </div>
              ))}
            </div>
          )}

          <div className="overflow-hidden rounded-xl border">
            <table className="w-full text-sm">
              <thead className="bg-muted/50 text-left text-xs uppercase tracking-wide text-muted-foreground">
                <tr>
                  <th className="px-3 py-2">{t("audit.colTime")}</th>
                  <th className="px-3 py-2">{t("audit.colUser")}</th>
                  <th className="px-3 py-2">{t("audit.colActor")}</th>
                  <th className="px-3 py-2">{t("audit.colServer")}</th>
                  <th className="px-3 py-2">{t("audit.colCommand")}</th>
                  <th className="px-3 py-2">{t("audit.colDecision")}</th>
                  <th className="px-3 py-2">{t("audit.colExit")}</th>
                </tr>
              </thead>
              <tbody>
                {events.map((ev) => (
                  <tr key={ev.id} className="border-b last:border-0 hover:bg-muted/30">
                    <td className="whitespace-nowrap px-3 py-1.5 text-xs text-muted-foreground">
                      {new Date(ev.timestamp).toLocaleString()}
                    </td>
                    <td className="px-3 py-1.5">{ev.username || "—"}</td>
                    <td className="px-3 py-1.5 text-xs">{ev.actor}</td>
                    <td className="px-3 py-1.5 text-xs">{ev.server_name || "—"}</td>
                    <td className="max-w-96 truncate px-3 py-1.5 font-mono text-xs" title={ev.command}>{ev.command}</td>
                    <td className="px-3 py-1.5"><Badge variant={decisionVariant(ev.decision)}>{ev.decision}</Badge></td>
                    <td className="px-3 py-1.5 text-xs">{ev.exit_code ?? "—"}</td>
                  </tr>
                ))}
                {events.length === 0 && (
                  <tr><td colSpan={7} className="px-3 py-8 text-center text-sm text-muted-foreground">{t("audit.empty")}</td></tr>
                )}
              </tbody>
            </table>
          </div>
        </TabsContent>

        <TabsContent value="sessions">
          <SessionReplay />
        </TabsContent>
      </Tabs>
    </Page>
  );
}

// --- MCP session listing + replay ---

function SessionReplay() {
  const { t } = useI18n();
  const [sessions, setSessions] = useState<MCPSession[] | null>(null);
  const [selected, setSelected] = useState<MCPSession | null>(null);
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [error, setError] = useState<string>();
  const [step, setStep] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [speedMs, setSpeedMs] = useState(1500);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  const loadSessions = useCallback(async () => {
    try {
      setSessions(await api.get<MCPSession[]>("/api/mcp-sessions?limit=200"));
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    if (sessions === null) void loadSessions();
  }, [sessions, loadSessions]);

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

  if (selected === null) {
    return (
      <div className="space-y-3">
        <ErrorText>{error}</ErrorText>
        <div className="overflow-hidden rounded-xl border">
          <table className="w-full text-sm">
            <thead className="bg-muted/50 text-left text-xs uppercase tracking-wide text-muted-foreground">
              <tr>
                <th className="px-3 py-2">{t("audit.session.colSession")}</th>
                <th className="px-3 py-2">{t("audit.session.colUser")}</th>
                <th className="px-3 py-2">{t("audit.session.colServers")}</th>
                <th className="px-3 py-2">{t("audit.session.colRange")}</th>
                <th className="px-3 py-2 text-right">{t("audit.session.colCount")}</th>
              </tr>
            </thead>
            <tbody>
              {(sessions ?? []).map((s) => (
                <tr key={s.session_id} className="cursor-pointer border-b last:border-0 hover:bg-muted/30"
                  onClick={() => void open(s)}>
                  <td className="px-3 py-1.5 font-mono text-xs">{s.session_id}</td>
                  <td className="px-3 py-1.5">{s.username || "—"}</td>
                  <td className="max-w-64 truncate px-3 py-1.5 text-xs text-muted-foreground" title={s.servers}>{s.servers || "—"}</td>
                  <td className="whitespace-nowrap px-3 py-1.5 text-xs text-muted-foreground">
                    {new Date(s.first_at).toLocaleString()} → {new Date(s.last_at).toLocaleTimeString()}
                  </td>
                  <td className="px-3 py-1.5 text-right font-medium">{s.event_count}</td>
                </tr>
              ))}
              {sessions !== null && sessions.length === 0 && (
                <tr><td colSpan={5} className="px-3 py-8 text-center text-sm text-muted-foreground">{t("audit.sessions.empty")}</td></tr>
              )}
            </tbody>
          </table>
        </div>
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
