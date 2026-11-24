import { useCallback, useEffect, useState } from "react";
import { ConfirmDialog } from "@/components/bulwark/ConfirmDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ErrorText } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { UserSession } from "@/types";

export default function AdminSessions() {
  const { t } = useI18n();
  const [sessions, setSessions] = useState<UserSession[]>([]);
  const [error, setError] = useState<string>();
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmKick, setConfirmKick] = useState(false);

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

  const q = filter.trim().toLowerCase();
  const filtered = sessions.filter(
    (s) =>
      !q ||
      (s.username ?? s.user_id).toLowerCase().includes(q) ||
      s.ip.toLowerCase().includes(q)
  );

  const kick = async (id: string) => {
    try {
      await api.del(`/api/admin/sessions/${id}`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "kick failed");
    }
  };

  const toggleSelected = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const allChecked = filtered.length > 0 && filtered.every((s) => selected.has(s.id));

  const kickSelected = async () => {
    for (const id of selected) {
      try {
        await api.del(`/api/admin/sessions/${id}`);
      } catch (err) {
        setError(err instanceof Error ? err.message : "kick failed");
      }
    }
    setSelected(new Set());
    await load();
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold tracking-tight">{t("admin.sessions.title")}</h2>
        <div className="flex items-center gap-2">
          {selected.size > 0 && (
            <Badge variant="secondary">{t("sessions.selected", { count: selected.size })}</Badge>
          )}
          <Input
            placeholder={t("sessions.filterUser")}
            className="w-56"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <Button
            variant="destructive"
            size="sm"
            disabled={selected.size === 0}
            onClick={() => setConfirmKick(true)}
          >
            {t("sessions.kickAll")}
          </Button>
        </div>
      </div>
      <ErrorText>{error}</ErrorText>

      <Card className="p-0">
        <Table>
          <TableHeader className="bg-secondary/50">
            <TableRow>
              <TableHead className="w-10 px-4">
                <input
                  type="checkbox"
                  aria-label={t("sessions.kickAll")}
                  checked={allChecked}
                  onChange={(e) =>
                    setSelected(new Set(e.target.checked ? filtered.map((s) => s.id) : []))
                  }
                />
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("sessions.colUsername")}
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
            {filtered.map((s) => (
              <TableRow key={s.id}>
                <TableCell className="px-4">
                  <input
                    type="checkbox"
                    checked={selected.has(s.id)}
                    onChange={() => toggleSelected(s.id)}
                  />
                </TableCell>
                <TableCell className="px-4">
                  <div className="font-medium">{s.username ?? s.user_id}</div>
                  {s.username && (
                    <div className="font-mono text-xs text-muted-foreground">{s.user_id}</div>
                  )}
                </TableCell>
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
            {filtered.length === 0 && (
              <TableRow>
                <TableCell colSpan={6} className="px-4 py-10 text-center text-sm text-muted-foreground">
                  {t("sessions.empty")}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </Card>

      <ConfirmDialog
        open={confirmKick}
        title={t("sessions.kickAll")}
        description={t("sessions.selected", { count: selected.size })}
        confirmLabel={t("sessions.kickAll")}
        onConfirm={() => void kickSelected()}
        onOpenChange={setConfirmKick}
      />
    </div>
  );
}
