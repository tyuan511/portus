import { useCallback, useEffect, useState } from "react";
import { KeyRound, Plus, ShieldX, Trash2, UserCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/api";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { Role, User } from "@/types";

export default function AdminUsers() {
  const [users, setUsers] = useState<User[]>([]);
  const [roles, setRoles] = useState<Role[]>([]);
  const [error, setError] = useState<string>();
  const [info, setInfo] = useState<string>();
  const [form, setForm] = useState<{ username: string; display_name: string; password: string; role_id: string } | null>(null);
  const [tempPassword, setTempPassword] = useState<{ username: string; password: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setUsers(await api.get<User[]>("/api/admin/users"));
      setRoles(await api.get<Role[]>("/api/admin/roles"));
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
    if (!confirm(`Delete user "${u.username}"? Their sessions will be revoked.`)) return;
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
    if (!confirm(`Reset TOTP for "${u.username}"?`)) return;
    try { await api.post(`/api/admin/users/${u.id}/reset-totp`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "reset failed");
    }
  };

  return (
    <Page
      header={
        <PageHeader
          title="User Management"
          actions={<Button size="sm" onClick={() => setForm({ username: "", display_name: "", password: "", role_id: roles.find((r) => r.name === "member")?.id ?? "" })}><Plus className="size-3.5" /> Add user</Button>}
        />
      }
    >
      <ErrorText>{error}</ErrorText>
      {info && <p className="mb-2 text-sm text-primary">{info}</p>}
      <div className="overflow-hidden rounded-xl border">
        <table className="w-full text-sm">
          <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
            <tr><th className="px-3 py-2">User</th><th className="px-3 py-2">Role</th><th className="px-3 py-2">2FA</th><th className="px-3 py-2">Active</th><th className="px-3 py-2">Last login</th><th className="px-3 py-2" /></tr>
          </thead>
          <tbody>
            {users.map((u) => (
              <tr key={u.id} className="border-b last:border-0">
                <td className="px-3 py-1.5">
                  <div className="font-medium">{u.username}</div>
                  <div className="text-xs text-muted-foreground">{u.display_name}</div>
                </td>
                <td className="px-3 py-1.5">
                  <select className="h-7 rounded border bg-background px-1.5 text-xs" value={u.role_id}
                    onChange={(e) => void patch(u.id, { role_id: e.target.value })}>
                    {roles.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
                  </select>
                </td>
                <td className="px-3 py-1.5">{u.totp_enabled ? <Badge>on</Badge> : <Badge variant="secondary">off</Badge>}</td>
                <td className="px-3 py-1.5">
                  <Switch checked={u.is_active} onCheckedChange={(v) => void patch(u.id, { is_active: v })} />
                </td>
                <td className="px-3 py-1.5 text-xs text-muted-foreground">
                  {u.last_login_at ? new Date(u.last_login_at).toLocaleString() : "never"}
                </td>
                <td className="px-3 py-1.5">
                  <div className="flex justify-end gap-1">
                    <Button variant="outline" size="xs" onClick={() => void resetPassword(u)}><KeyRound className="size-3" /> Reset pwd</Button>
                    {u.totp_enabled && <Button variant="outline" size="xs" onClick={() => void resetTotp(u)}><ShieldX className="size-3" /> Reset 2FA</Button>}
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
          <DialogHeader><DialogTitle>Add user</DialogTitle></DialogHeader>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1"><Label>Username</Label><Input value={form?.username ?? ""} onChange={(e) => setForm({ ...form!, username: e.target.value })} /></div>
            <div className="space-y-1"><Label>Display name</Label><Input value={form?.display_name ?? ""} onChange={(e) => setForm({ ...form!, display_name: e.target.value })} /></div>
            <div className="space-y-1"><Label>Initial password</Label><Input type="password" value={form?.password ?? ""} onChange={(e) => setForm({ ...form!, password: e.target.value })} /></div>
            <div className="space-y-1"><Label>Role</Label>
              <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.role_id} onChange={(e) => setForm({ ...form!, role_id: e.target.value })}>
                {roles.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
              </select>
            </div>
            <div className="col-span-2"><ErrorText>{error}</ErrorText></div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>Cancel</Button>
            <Button onClick={() => void create()}><UserCheck className="size-3.5" /> Create</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={tempPassword !== null} onOpenChange={(o) => !o && setTempPassword(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>Temporary password</DialogTitle></DialogHeader>
          <p className="text-sm text-muted-foreground">Give this to <b>{tempPassword?.username}</b> now — shown only once. They must change it at next login.</p>
          <code className="block select-all rounded-lg bg-muted p-3 text-center font-mono text-lg">{tempPassword?.password}</code>
          <DialogFooter><Button onClick={() => setTempPassword(null)}>Done</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
