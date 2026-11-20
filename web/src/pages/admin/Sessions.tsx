import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { UserSession } from "@/types";

export default function AdminSessions() {
  const [sessions, setSessions] = useState<UserSession[]>([]);
  const [error, setError] = useState<string>();

  const load = useCallback(async () => {
    try {
      setSessions(await api.get<UserSession[]>("/api/admin/sessions"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const kick = async (id: string) => {
    try { await api.del(`/api/admin/sessions/${id}`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "kick failed");
    }
  };

  return (
    <Page header={<PageHeader title="Active Sessions" subtitle="Force sign-out for any user" />}>
      <ErrorText>{error}</ErrorText>
      <div className="overflow-hidden rounded-xl border">
        <table className="w-full text-sm">
          <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
            <tr><th className="px-3 py-2">Session</th><th className="px-3 py-2">IP</th><th className="px-3 py-2">User agent</th><th className="px-3 py-2">Last seen</th><th className="px-3 py-2" /></tr>
          </thead>
          <tbody>
            {sessions.map((s) => (
              <tr key={s.id} className="border-b last:border-0">
                <td className="px-3 py-1.5 font-mono text-xs">{s.user_id}</td>
                <td className="px-3 py-1.5 text-xs">{s.ip}</td>
                <td className="max-w-80 truncate px-3 py-1.5 text-xs text-muted-foreground" title={s.user_agent}>{s.user_agent}</td>
                <td className="px-3 py-1.5 text-xs">{new Date(s.last_seen_at).toLocaleString()}</td>
                <td className="px-3 py-1.5 text-right">
                  <Button variant="destructive" size="xs" onClick={() => void kick(s.id)}>Force sign-out</Button>
                </td>
              </tr>
            ))}
            {sessions.length === 0 && <tr><td colSpan={5} className="px-3 py-6 text-center text-sm text-muted-foreground">No active sessions.</td></tr>}
          </tbody>
        </table>
      </div>
    </Page>
  );
}
