import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { ErrorText } from "@/components/ui/shared";
import type { CommandSnippet } from "@/types";

export default function Snippets() {
  const auth = useAuth();
  const canManage = auth.hasPermission("snippets.manage");
  const [snippets, setSnippets] = useState<CommandSnippet[]>([]);
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<{ name: string; command: string; description: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setSnippets(await api.get<CommandSnippet[]>("/api/snippets"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const save = async () => {
    if (!form) return;
    try {
      await api.post("/api/snippets", { ...form, id: "", tags: [] });
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "save failed");
    }
  };

  const remove = async (id: string) => {
    if (!confirm("Delete snippet?")) return;
    try {
      await api.del(`/api/snippets/${id}`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  return (
    <div className="space-y-3">
      {canManage && (
        <Button size="sm" onClick={() => setForm({ name: "", command: "", description: "" })}>
          <Plus className="size-3.5" /> New snippet
        </Button>
      )}
      <ErrorText>{error}</ErrorText>
      <div className="space-y-2">
        {snippets.map((s) => (
          <div key={s.id} className="flex items-center justify-between rounded-xl border px-3 py-2">
            <div className="min-w-0">
              <div className="text-sm font-medium">{s.name}</div>
              <code className="block truncate text-xs text-muted-foreground" title={s.command}>{s.command}</code>
            </div>
            <div className="flex items-center gap-2">
              <Button variant="outline" size="xs" onClick={() => void navigator.clipboard.writeText(s.command)}>Copy</Button>
              {canManage && <Button variant="destructive" size="icon-xs" onClick={() => void remove(s.id)}><Trash2 className="size-3" /></Button>}
            </div>
          </div>
        ))}
        {snippets.length === 0 && <p className="mt-8 text-center text-sm text-muted-foreground">No snippets.</p>}
      </div>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>New snippet</DialogTitle></DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1"><Label>Name</Label><Input value={form?.name ?? ""} onChange={(e) => setForm({ ...form!, name: e.target.value })} /></div>
            <div className="space-y-1"><Label>Command</Label><Input value={form?.command ?? ""} onChange={(e) => setForm({ ...form!, command: e.target.value })} /></div>
            <ErrorText>{error}</ErrorText>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>Cancel</Button>
            <Button onClick={() => void save()}>Save</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
