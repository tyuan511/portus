import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { AuditStats, AuditEvent } from "@/types";

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
      <ErrorText>{error}</ErrorText>
      {chain && (
        <p className={`mb-3 text-sm ${chain.ok ? "text-primary" : "text-destructive"}`}>{chain.text}</p>
      )}

      {stats && (
        <div className="mb-4 grid grid-cols-2 gap-2 sm:grid-cols-6">
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
    </Page>
  );
}
