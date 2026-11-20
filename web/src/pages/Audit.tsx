import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { AuditStats, AuditEvent } from "@/types";

const decisionVariant = (d: string) => (d === "allow" ? "default" : d === "deny" ? "destructive" : "secondary") as "default" | "destructive" | "secondary";

export default function Audit() {
  const auth = useAuth();
  const canViewAll = auth.hasPermission("audit.view_all");
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [stats, setStats] = useState<AuditStats | null>(null);
  const [q, setQ] = useState("");
  const [error, setError] = useState<string>();
  const [chainValid, setChainValid] = useState<string>();

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
      setChainValid(r.valid ? `chain valid (${r.total_events} events)` : `TAMPERED at ${r.broken_at_id}`);
    } catch (err) {
      setChainValid(err instanceof Error ? err.message : "verify failed");
    }
  };

  return (
    <Page
      header={
        <PageHeader
          title="Audit Logs"
          subtitle={canViewAll ? "All users' operations" : "Your own and MCP activity"}
          actions={
            <div className="flex items-center gap-2">
              <Input placeholder="Search…" value={q} onChange={(e) => setQ(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && void load()} className="h-8 w-56" />
              <Button size="sm" variant="outline" onClick={() => void load()}>Refresh</Button>
              {canViewAll && <Button size="sm" variant="outline" onClick={() => void verify()}>Verify chain</Button>}
            </div>
          }
        />
      }
    >
      <ErrorText>{error}</ErrorText>
      {chainValid && (
        <p className={`mb-3 text-sm ${chainValid.startsWith("chain valid") ? "text-primary" : "text-destructive"}`}>{chainValid}</p>
      )}

      {stats && (
        <div className="mb-4 grid grid-cols-2 gap-2 sm:grid-cols-6">
          {[
            ["Total", stats.total_commands],
            ["Allowed", stats.allowed_count],
            ["Approved", stats.approval_count],
            ["Denied", stats.denied_count],
            ["Failed", stats.failed_count],
            ["Avg ms", Math.round(stats.avg_duration_ms ?? 0)],
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
              <th className="px-3 py-2">Time</th>
              <th className="px-3 py-2">User</th>
              <th className="px-3 py-2">Actor</th>
              <th className="px-3 py-2">Server</th>
              <th className="px-3 py-2">Command</th>
              <th className="px-3 py-2">Decision</th>
              <th className="px-3 py-2">Exit</th>
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
              <tr><td colSpan={7} className="px-3 py-8 text-center text-sm text-muted-foreground">No audit events.</td></tr>
            )}
          </tbody>
        </table>
      </div>
    </Page>
  );
}
