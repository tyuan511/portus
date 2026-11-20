import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { UserSession } from "@/types";

export default function AdminSessions() {
  const { t } = useI18n();
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
    <Page header={<PageHeader title={t("admin.sessions.title")} />}>
      <ErrorText>{error}</ErrorText>
      <div className="overflow-hidden rounded-xl border">
        <table className="w-full text-sm">
          <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
            <tr><th className="px-3 py-2">{t("sessions.session")}</th><th className="px-3 py-2">{t("sessions.ip")}</th><th className="px-3 py-2">{t("sessions.userAgent")}</th><th className="px-3 py-2">{t("sessions.lastSeen")}</th><th className="px-3 py-2" /></tr>
          </thead>
          <tbody>
            {sessions.map((s) => (
              <tr key={s.id} className="border-b last:border-0">
                <td className="px-3 py-1.5 font-mono text-xs">{s.user_id}</td>
                <td className="px-3 py-1.5 text-xs">{s.ip}</td>
                <td className="max-w-80 truncate px-3 py-1.5 text-xs text-muted-foreground" title={s.user_agent}>{s.user_agent}</td>
                <td className="px-3 py-1.5 text-xs">{new Date(s.last_seen_at).toLocaleString()}</td>
                <td className="px-3 py-1.5 text-right">
                  <Button variant="destructive" size="xs" onClick={() => void kick(s.id)}>{t("sessions.forceSignout")}</Button>
                </td>
              </tr>
            ))}
            {sessions.length === 0 && <tr><td colSpan={5} className="px-3 py-6 text-center text-sm text-muted-foreground">{t("sessions.empty")}</td></tr>}
          </tbody>
        </table>
      </div>
    </Page>
  );
}
