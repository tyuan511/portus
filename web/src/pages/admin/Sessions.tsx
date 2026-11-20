import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ErrorText } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
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

  useEffect(() => {
    void load();
  }, [load]);

  const kick = async (id: string) => {
    try {
      await api.del(`/api/admin/sessions/${id}`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "kick failed");
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <h2 className="text-sm font-semibold tracking-tight">{t("admin.sessions.title")}</h2>
      <ErrorText>{error}</ErrorText>

      <Card className="p-0">
        <Table>
          <TableHeader className="bg-secondary/50">
            <TableRow>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("sessions.session")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("sessions.ip")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("sessions.userAgent")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("sessions.lastSeen")}
              </TableHead>
              <TableHead className="px-4" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {sessions.map((s) => (
              <TableRow key={s.id}>
                <TableCell className="px-4 font-mono text-xs">{s.user_id}</TableCell>
                <TableCell className="px-4 font-mono text-xs">{s.ip}</TableCell>
                <TableCell
                  className="max-w-80 truncate px-4 text-xs text-muted-foreground"
                  title={s.user_agent}
                >
                  {s.user_agent}
                </TableCell>
                <TableCell className="px-4 font-mono text-xs tabular-nums">
                  {new Date(s.last_seen_at).toLocaleString()}
                </TableCell>
                <TableCell className="px-4 text-right">
                  <Button variant="destructive" size="xs" onClick={() => void kick(s.id)}>
                    {t("sessions.forceSignout")}
                  </Button>
                </TableCell>
              </TableRow>
            ))}
            {sessions.length === 0 && (
              <TableRow>
                <TableCell colSpan={5} className="px-4 py-10 text-center text-sm text-muted-foreground">
                  {t("sessions.empty")}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </Card>
    </div>
  );
}
