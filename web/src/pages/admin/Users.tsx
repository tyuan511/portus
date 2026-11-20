import { useCallback, useEffect, useState } from "react";
import { KeyRound, Plus, ShieldX, Trash2, UserCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { Role, User } from "@/types";

// Built-in roles only — admin manages hosts and review rules, members just
// use granted hosts and MCP.
const ROLES: Role[] = ["admin", "member"];

export default function AdminUsers() {
  const { t } = useI18n();
  const [users, setUsers] = useState<User[]>([]);
  const [error, setError] = useState<string>();
  const [info, setInfo] = useState<string>();
  const [form, setForm] = useState<{ username: string; display_name: string; password: string; role: Role } | null>(null);
  const [tempPassword, setTempPassword] = useState<{ username: string; password: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setUsers(await api.get<User[]>("/api/admin/users"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const create = async () => {
    if (!form) return;
    try {
      await api.post("/api/admin/users", form);
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "create failed");
    }
  };

  const patch = async (id: string, body: Record<string, unknown>) => {
    try {
      await api.patch(`/api/admin/users/${id}`, body);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "update failed");
    }
  };

  const remove = async (u: User) => {
    if (!confirm(t("admin.users.deleteConfirm", { name: u.username }))) return;
    try { await api.del(`/api/admin/users/${u.id}`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const resetPassword = async (u: User, newPassword = "") => {
    try {
      const res = await api.post<{ temporary_password: string }>(`/api/admin/users/${u.id}/reset-password`, { new_password: newPassword });
      setTempPassword({ username: u.username, password: res.temporary_password });
    } catch (err) {
      setError(err instanceof Error ? err.message : "reset failed");
    }
  };

  const resetTotp = async (u: User) => {
    if (!confirm(t("admin.users.resetTotpConfirm", { name: u.username }))) return;
    try { await api.post(`/api/admin/users/${u.id}/reset-totp`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "reset failed");
    }
  };

  return (
    <Page
      header={
        <PageHeader
          title={t("admin.users.title")}
          actions={<Button size="sm" onClick={() => setForm({ username: "", display_name: "", password: "", role: "member" })}><Plus className="size-3.5" /> {t("admin.users.add")}</Button>}
        />
      }
    >
      <ErrorText>{error}</ErrorText>
      {info && <p className="mb-2 text-sm text-primary">{info}</p>}
      <div className="overflow-hidden rounded-xl border">
        <table className="w-full text-sm">
          <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
            <tr><th className="px-3 py-2">{t("admin.users.colUser")}</th><th className="px-3 py-2">{t("admin.users.colRole")}</th><th className="px-3 py-2">{t("admin.users.col2fa")}</th><th className="px-3 py-2">{t("admin.users.colActive")}</th><th className="px-3 py-2">{t("admin.users.colLastLogin")}</th><th className="px-3 py-2" /></tr>
          </thead>
          <tbody>
            {users.map((u) => (
              <tr key={u.id} className="border-b last:border-0">
                <td className="px-3 py-1.5">
                  <div className="font-medium">{u.username}</div>
                  <div className="text-xs text-muted-foreground">{u.display_name}</div>
                </td>
                <td className="px-3 py-1.5">
                  <select className="h-7 rounded border bg-background px-1.5 text-xs" value={u.role}
                    onChange={(e) => void patch(u.id, { role: e.target.value })}>
                    {ROLES.map((r) => <option key={r} value={r}>{t(r === "admin" ? "role.admin" : "role.member")}</option>)}
                  </select>
                </td>
                <td className="px-3 py-1.5">{u.totp_enabled ? <Badge>{t("common.on")}</Badge> : <Badge variant="secondary">{t("common.off")}</Badge>}</td>
                <td className="px-3 py-1.5">
                  <Switch checked={u.is_active} onCheckedChange={(v) => void patch(u.id, { is_active: v })} />
                </td>
                <td className="px-3 py-1.5 text-xs text-muted-foreground">
                  {u.last_login_at ? new Date(u.last_login_at).toLocaleString() : t("common.never")}
                </td>
                <td className="px-3 py-1.5">
                  <div className="flex justify-end gap-1">
                    <Button variant="outline" size="xs" onClick={() => void resetPassword(u)}><KeyRound className="size-3" /> {t("admin.users.resetPwd")}</Button>
                    {u.totp_enabled && <Button variant="outline" size="xs" onClick={() => void resetTotp(u)}><ShieldX className="size-3" /> {t("admin.users.resetTotp")}</Button>}
                    <Button variant="destructive" size="icon-xs" onClick={() => void remove(u)}><Trash2 className="size-3" /></Button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("admin.users.add")}</DialogTitle></DialogHeader>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1"><Label>{t("admin.users.fUsername")}</Label><Input value={form?.username ?? ""} onChange={(e) => setForm({ ...form!, username: e.target.value })} /></div>
            <div className="space-y-1"><Label>{t("admin.users.fDisplay")}</Label><Input value={form?.display_name ?? ""} onChange={(e) => setForm({ ...form!, display_name: e.target.value })} /></div>
            <div className="space-y-1"><Label>{t("admin.users.fInitialPwd")}</Label><Input type="password" value={form?.password ?? ""} onChange={(e) => setForm({ ...form!, password: e.target.value })} /></div>
            <div className="space-y-1"><Label>{t("admin.users.fRole")}</Label>
              <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.role} onChange={(e) => setForm({ ...form!, role: e.target.value as Role })}>
                {ROLES.map((r) => <option key={r} value={r}>{t(r === "admin" ? "role.admin" : "role.member")}</option>)}
              </select>
            </div>
            <div className="col-span-2"><ErrorText>{error}</ErrorText></div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>{t("common.cancel")}</Button>
            <Button onClick={() => void create()}><UserCheck className="size-3.5" /> {t("common.create")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={tempPassword !== null} onOpenChange={(o) => !o && setTempPassword(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t("admin.users.tempPwd")}</DialogTitle></DialogHeader>
          <p className="text-sm text-muted-foreground">{t("admin.users.tempPwdHint", { name: tempPassword?.username ?? "" })}</p>
          <code className="block select-all rounded-lg bg-muted p-3 text-center font-mono text-lg">{tempPassword?.password}</code>
          <DialogFooter><Button onClick={() => setTempPassword(null)}>{t("common.done")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
