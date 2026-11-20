import { useCallback, useEffect, useState } from "react";
import { Lock, LockOpen, Plus, Trash2, Unlock } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/api";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { VaultCredentialMeta, VaultStatus } from "@/types";

export default function VaultView() {
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [credentials, setCredentials] = useState<VaultCredentialMeta[]>([]);
  const [passphrase, setPassphrase] = useState("");
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<{ label: string; kind: string; username: string; secret: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setStatus(await api.get<VaultStatus>("/api/vault/status"));
      setCredentials(await api.get<VaultCredentialMeta[]>("/api/vault/credentials"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const initOrUnlock = async () => {
    try {
      const path = status?.state === "uninitialized" ? "/api/vault/initialize" : "/api/vault/unlock";
      setStatus(await api.post<VaultStatus>(path, { passphrase }));
      setPassphrase("");
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "operation failed");
    }
  };

  const lock = async () => {
    try { setStatus(await api.post<VaultStatus>("/api/vault/lock")); } catch (err) {
      setError(err instanceof Error ? err.message : "lock failed");
    }
  };

  const store = async () => {
    if (!form) return;
    try {
      await api.post("/api/vault/credentials", {
        label: form.label, kind: form.kind || "password", username: form.username, secret: form.secret,
      });
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "store failed");
    }
  };

  const removeCred = async (id: string) => {
    if (!confirm("Delete credential?")) return;
    try { await api.del(`/api/vault/credentials/${id}`); await load(); } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const stateBadge = () => {
    if (!status) return null;
    const map = { uninitialized: "secondary", locked: "destructive", unlocked: "default" } as const;
    return <Badge variant={map[status.state]}>{status.state}</Badge>;
  };

  return (
    <Page
      header={
        <PageHeader
          title="Credential Vault"
          subtitle={`${status?.credential_count ?? 0} credentials · auto-lock ${status?.auto_lock_secs ?? 0}s`}
          actions={
            <div className="flex items-center gap-2">
              {stateBadge()}
              {status?.state === "unlocked" && <Button size="sm" variant="outline" onClick={() => void lock()}><Lock className="size-3.5" /> Lock</Button>}
              {status && status.state !== "uninitialized" && (
                <Button size="sm" onClick={() => setForm({ label: "", kind: "password", username: "", secret: "" })} disabled={status.state !== "unlocked"}>
                  <Plus className="size-3.5" /> Credential
                </Button>
              )}
            </div>
          }
        />
      }
    >
      <ErrorText>{error}</ErrorText>

      {status?.state !== "unlocked" && (
        <div className="max-w-md space-y-2 rounded-xl border p-4">
          <p className="text-sm text-muted-foreground">
            {status?.state === "uninitialized"
              ? "Initialize a new vault with a master passphrase (min 8 chars)."
              : "Enter the master passphrase to unlock stored credentials."}
          </p>
          <div className="flex gap-2">
            <Input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && void initOrUnlock()} placeholder="Master passphrase" />
            <Button onClick={() => void initOrUnlock()}>
              {status?.state === "uninitialized" ? <><LockOpen className="size-3.5" /> Initialize</> : <><Unlock className="size-3.5" /> Unlock</>}
            </Button>
          </div>
        </div>
      )}

      {status?.state === "unlocked" && (
        <div className="overflow-hidden rounded-xl border">
          <table className="w-full text-sm">
            <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
              <tr><th className="px-3 py-2">Label</th><th className="px-3 py-2">Kind</th><th className="px-3 py-2">Username</th><th className="px-3 py-2">Preview</th><th className="px-3 py-2" /></tr>
            </thead>
            <tbody>
              {credentials.map((c) => (
                <tr key={c.id} className="border-b last:border-0">
                  <td className="px-3 py-1.5 font-medium">{c.label}</td>
                  <td className="px-3 py-1.5 text-xs">{c.kind}</td>
                  <td className="px-3 py-1.5 text-xs">{c.username || "—"}</td>
                  <td className="px-3 py-1.5 font-mono text-xs text-muted-foreground">{c.preview}</td>
                  <td className="px-3 py-1.5 text-right">
                    <Button variant="destructive" size="icon-xs" onClick={() => void removeCred(c.id)}><Trash2 className="size-3" /></Button>
                  </td>
                </tr>
              ))}
              {credentials.length === 0 && <tr><td colSpan={5} className="px-3 py-6 text-center text-sm text-muted-foreground">No credentials stored.</td></tr>}
            </tbody>
          </table>
        </div>
      )}

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>New credential</DialogTitle></DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1"><Label>Label</Label><Input value={form?.label ?? ""} onChange={(e) => setForm({ ...form!, label: e.target.value })} /></div>
            <div className="space-y-1"><Label>Kind</Label>
              <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.kind} onChange={(e) => setForm({ ...form!, kind: e.target.value })}>
                <option value="password">Password</option>
                <option value="private_key">Private key</option>
                <option value="api_key">API key</option>
              </select>
            </div>
            <div className="space-y-1"><Label>Username (optional)</Label><Input value={form?.username ?? ""} onChange={(e) => setForm({ ...form!, username: e.target.value })} /></div>
            <div className="space-y-1"><Label>Secret</Label><Input type="password" value={form?.secret ?? ""} onChange={(e) => setForm({ ...form!, secret: e.target.value })} /></div>
            <ErrorText>{error}</ErrorText>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>Cancel</Button>
            <Button onClick={() => void store()}>Store</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
