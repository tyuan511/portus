import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { Permission, Role } from "@/types";
import { ALL_PERMISSIONS } from "@/lib/permissions";

export default function AdminRoles() {
  const auth = useAuth();
  const [roles, setRoles] = useState<Role[]>([]);
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<{ name: string; description: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setRoles(await api.get<Role[]>("/api/admin/roles"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const create = async () => {
    if (!form) return;
    try {
      await api.post("/api/admin/roles", { name: form.name, description: form.description, permissions: ["host.read"] });
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "create failed");
    }
  };

  const toggle = async (role: Role, perm: Permission) => {
    if (role.name === "admin") return;
    const permissions = role.permissions.includes(perm)
      ? role.permissions.filter((p) => p !== perm)
      : [...role.permissions, perm];
    // Optimistic update; revert on failure.
    setRoles((rs) => rs.map((r) => (r.id === role.id ? { ...r, permissions } : r)));
    try {
      await api.patch(`/api/admin/roles/${role.id}`, { name: role.name, description: role.description ?? "", permissions });
    } catch (err) {
      setError(err instanceof Error ? err.message : "update failed");
      await load();
    }
  };

  const remove = async (role: Role) => {
    if (!confirm(`Delete role "${role.name}"?`)) return;
    try { await api.del(`/api/admin/roles/${role.id}`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  return (
    <Page
      header={
        <PageHeader
          title="Roles & Permissions"
          subtitle="The admin role is immutable. Toggle permissions per role — changes take effect after the user signs in again."
          actions={<Button size="sm" onClick={() => setForm({ name: "", description: "" })}><Plus className="size-3.5" /> New role</Button>}
        />
      }
    >
      <ErrorText>{error}</ErrorText>
      <div className="overflow-x-auto rounded-xl border">
        <table className="w-full min-w-4xl text-sm">
          <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
            <tr>
              <th className="sticky left-0 bg-muted/50 px-3 py-2">Role</th>
              {ALL_PERMISSIONS.map((p) => <th key={p} className="px-2 py-2 text-center font-mono text-[10px]">{p}</th>)}
              <th className="px-2 py-2" />
            </tr>
          </thead>
          <tbody>
            {roles.map((role) => (
              <tr key={role.id} className="border-b last:border-0">
                <td className="sticky left-0 bg-background px-3 py-1.5">
                  <div className="font-medium">{role.name}</div>
                  {role.is_builtin && <Badge variant="secondary">builtin</Badge>}
                </td>
                {ALL_PERMISSIONS.map((p) => (
                  <td key={p} className="px-2 py-1.5 text-center">
                    <input
                      type="checkbox"
                      className="size-3.5 accent-[var(--primary)]"
                      checked={role.permissions.includes(p)}
                      disabled={role.name === "admin"}
                      onChange={() => void toggle(role, p)}
                    />
                  </td>
                ))}
                <td className="px-2 py-1.5 text-right">
                  {!role.is_builtin && (
                    <Button variant="destructive" size="icon-xs" onClick={() => void remove(role)}><Trash2 className="size-3" /></Button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>New role</DialogTitle></DialogHeader>
          <div className="space-y-3">
            <Input placeholder="Role name" value={form?.name ?? ""} onChange={(e) => setForm({ ...form!, name: e.target.value })} />
            <Input placeholder="Description" value={form?.description ?? ""} onChange={(e) => setForm({ ...form!, description: e.target.value })} />
            <ErrorText>{error}</ErrorText>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>Cancel</Button>
            <Button onClick={() => void create()}>Create</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <div className="mt-2 text-xs text-muted-foreground">Signed in as {auth.identity?.username}</div>
    </Page>
  );
}
