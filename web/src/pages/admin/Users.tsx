import { useCallback, useEffect, useState } from "react";
import { KeyRound, Plus, ShieldX, Trash2, UserCheck } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ErrorText } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { Role, User } from "@/types";

const ROLES: Role[] = ["admin", "member"];

export default function AdminUsers() {
  const { t } = useI18n();
  const [users, setUsers] = useState<User[]>([]);
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<{
    username: string;
    display_name: string;
    password: string;
    role: Role;
  } | null>(null);
  const [tempPassword, setTempPassword] = useState<{ username: string; password: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setUsers(await api.get<User[]>("/api/admin/users"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

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
    try {
      await api.del(`/api/admin/users/${u.id}`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const resetPassword = async (u: User, newPassword = "") => {
    try {
      const res = await api.post<{ temporary_password: string }>(
        `/api/admin/users/${u.id}/reset-password`,
        { new_password: newPassword }
      );
      setTempPassword({ username: u.username, password: res.temporary_password });
    } catch (err) {
      setError(err instanceof Error ? err.message : "reset failed");
    }
  };

  const resetTotp = async (u: User) => {
    if (!confirm(t("admin.users.resetTotpConfirm", { name: u.username }))) return;
    try {
      await api.post(`/api/admin/users/${u.id}/reset-totp`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "reset failed");
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold tracking-tight">{t("admin.users.title")}</h2>
        <Button
          size="sm"
          onClick={() => setForm({ username: "", display_name: "", password: "", role: "member" })}
        >
          <Plus data-icon="inline-start" />
          {t("admin.users.add")}
        </Button>
      </div>

      <ErrorText>{error}</ErrorText>

      <Card className="p-0">
        <Table>
          <TableHeader className="bg-secondary/50">
            <TableRow>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.colUser")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.colRole")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.col2fa")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.colActive")}
              </TableHead>
              <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                {t("admin.users.colLastLogin")}
              </TableHead>
              <TableHead className="px-4" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {users.map((u) => (
              <TableRow key={u.id}>
                <TableCell className="px-4">
                  <div className="font-medium">{u.username}</div>
                  {u.display_name && (
                    <div className="text-xs text-muted-foreground">{u.display_name}</div>
                  )}
                </TableCell>
                <TableCell className="px-4">
                  <Select
                    value={u.role}
                    onValueChange={(v) => v && void patch(u.id, { role: v })}
                  >
                    <SelectTrigger size="sm" className="w-32">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        {ROLES.map((r) => (
                          <SelectItem key={r} value={r}>
                            {t(r === "admin" ? "role.admin" : "role.member")}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </TableCell>
                <TableCell className="px-4">
                  {u.totp_enabled ? (
                    <Badge>{t("common.on")}</Badge>
                  ) : (
                    <Badge variant="secondary">{t("common.off")}</Badge>
                  )}
                </TableCell>
                <TableCell className="px-4">
                  <Switch
                    checked={u.is_active}
                    onCheckedChange={(v) => void patch(u.id, { is_active: v })}
                  />
                </TableCell>
                <TableCell className="px-4 font-mono text-xs text-muted-foreground tabular-nums">
                  {u.last_login_at ? new Date(u.last_login_at).toLocaleString() : t("common.never")}
                </TableCell>
                <TableCell className="px-4 text-right">
                  <div className="flex justify-end gap-1.5">
                    <Button variant="outline" size="xs" onClick={() => void resetPassword(u)}>
                      <KeyRound data-icon="inline-start" />
                      {t("admin.users.resetPwd")}
                    </Button>
                    {u.totp_enabled && (
                      <Button variant="outline" size="xs" onClick={() => void resetTotp(u)}>
                        <ShieldX data-icon="inline-start" />
                        {t("admin.users.resetTotp")}
                      </Button>
                    )}
                    <Button variant="destructive" size="icon-xs" onClick={() => void remove(u)}>
                      <Trash2 />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </Card>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("admin.users.add")}</DialogTitle>
          </DialogHeader>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>{t("admin.users.fUsername")}</Label>
              <Input
                value={form?.username ?? ""}
                onChange={(e) => setForm({ ...form!, username: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("admin.users.fDisplay")}</Label>
              <Input
                value={form?.display_name ?? ""}
                onChange={(e) => setForm({ ...form!, display_name: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("admin.users.fInitialPwd")}</Label>
              <Input
                type="password"
                value={form?.password ?? ""}
                onChange={(e) => setForm({ ...form!, password: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("admin.users.fRole")}</Label>
              <Select
                value={form?.role ?? "member"}
                onValueChange={(v) => v && setForm({ ...form!, role: v as Role })}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {ROLES.map((r) => (
                      <SelectItem key={r} value={r}>
                        {t(r === "admin" ? "role.admin" : "role.member")}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            <div className="col-span-2">
              <ErrorText>{error}</ErrorText>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void create()}>
              <UserCheck data-icon="inline-start" />
              {t("common.create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={tempPassword !== null} onOpenChange={(o) => !o && setTempPassword(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("admin.users.tempPwd")}</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            {t("admin.users.tempPwdHint", { name: tempPassword?.username ?? "" })}
          </p>
          <code className="block select-all rounded-lg border border-border bg-secondary p-3 text-center font-mono text-lg">
            {tempPassword?.password}
          </code>
          <DialogFooter>
            <Button onClick={() => setTempPassword(null)}>{t("common.done")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
