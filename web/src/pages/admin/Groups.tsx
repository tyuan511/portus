import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2, Users } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/api";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { User, UserGroup } from "@/types";

export default function AdminGroups() {
  const [groups, setGroups] = useState<UserGroup[]>([]);
  const [members, setMembers] = useState<Record<string, User[]>>({});
  const [allUsers, setAllUsers] = useState<User[]>([]);
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<{ name: string; description: string } | null>(null);
  const [addUser, setAddUser] = useState<{ groupId: string; userId: string } | null>(null);

  const load = useCallback(async () => {
    try {
      const gs = await api.get<UserGroup[]>("/api/admin/groups");
      setGroups(gs);
      setAllUsers(await api.get<User[]>("/api/admin/users"));
      const map: Record<string, User[]> = {};
      for (const g of gs) map[g.id] = await api.get<User[]>(`/api/admin/groups/${g.id}/members`);
      setMembers(map);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const create = async () => {
    if (!form) return;
    try {
      await api.post("/api/admin/groups", form);
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "create failed");
    }
  };

  const remove = async (g: UserGroup) => {
    if (!confirm(`Delete group "${g.name}"? Its host grants are removed too.`)) return;
    try { await api.del(`/api/admin/groups/${g.id}`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const addMember = async () => {
    if (!addUser?.userId) return;
    try {
      await api.post(`/api/admin/groups/${addUser.groupId}/members`, { user_id: addUser.userId });
      setAddUser(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "add failed");
    }
  };

  const removeMember = async (groupId: string, userId: string) => {
    try { await api.del(`/api/admin/groups/${groupId}/members/${userId}`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "remove failed");
    }
  };

  return (
    <Page
      header={
        <PageHeader
          title="User Groups"
          subtitle="Groups are the target of host access grants"
          actions={<Button size="sm" onClick={() => setForm({ name: "", description: "" })}><Plus className="size-3.5" /> New group</Button>}
        />
      }
    >
      <ErrorText>{error}</ErrorText>
      <div className="space-y-3">
        {groups.map((g) => (
          <div key={g.id} className="rounded-xl border p-3">
            <div className="flex items-center justify-between">
              <div>
                <div className="font-medium">{g.name}</div>
                <div className="text-xs text-muted-foreground">{g.description || "no description"}</div>
              </div>
              <div className="flex items-center gap-1.5">
                <Button size="xs" variant="outline" onClick={() => setAddUser({ groupId: g.id, userId: "" })}><Users className="size-3" /> Add member</Button>
                <Button variant="destructive" size="icon-xs" onClick={() => void remove(g)}><Trash2 className="size-3" /></Button>
              </div>
            </div>
            <div className="mt-2 flex flex-wrap gap-1.5">
              {(members[g.id] ?? []).map((u) => (
                <Badge key={u.id} variant="secondary" className="gap-1">
                  {u.username}
                  <button className="text-muted-foreground hover:text-destructive" onClick={() => void removeMember(g.id, u.id)}>×</button>
                </Badge>
              ))}
              {(members[g.id] ?? []).length === 0 && <span className="text-xs text-muted-foreground">no members</span>}
            </div>
          </div>
        ))}
        {groups.length === 0 && <p className="mt-8 text-center text-sm text-muted-foreground">No groups yet.</p>}
      </div>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>New group</DialogTitle></DialogHeader>
          <div className="space-y-3">
            <Input placeholder="Group name (e.g. ops-team)" value={form?.name ?? ""} onChange={(e) => setForm({ ...form!, name: e.target.value })} />
            <Input placeholder="Description" value={form?.description ?? ""} onChange={(e) => setForm({ ...form!, description: e.target.value })} />
            <ErrorText>{error}</ErrorText>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>Cancel</Button>
            <Button onClick={() => void create()}>Create</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={addUser !== null} onOpenChange={(o) => !o && setAddUser(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>Add member</DialogTitle></DialogHeader>
          <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm"
            value={addUser?.userId ?? ""} onChange={(e) => setAddUser({ ...addUser!, userId: e.target.value })}>
            <option value="">Select a user…</option>
            {allUsers.map((u) => <option key={u.id} value={u.id}>{u.username} ({u.display_name})</option>)}
          </select>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddUser(null)}>Cancel</Button>
            <Button onClick={() => void addMember()} disabled={!addUser?.userId}>Add</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
