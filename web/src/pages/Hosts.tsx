import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Pencil, Plug, Plus, Trash2, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { ServerAsset, CredentialMeta } from "@/types";

const EMPTY_FORM = {
  id: "",
  name: "",
  host: "",
  port: 22,
  username: "root",
  auth_kind: "agent_or_default_key" as ServerAsset["auth_kind"],
  identity_file: "",
  credential_id: "",
  jump_host_id: "",
  group_name: "",
  tags: "",
  enabled: true,
};

export default function Hosts() {
  const auth = useAuth();
  const { t } = useI18n();
  const navigate = useNavigate();
  const [servers, setServers] = useState<ServerAsset[]>([]);
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<typeof EMPTY_FORM | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [testResult, setTestResult] = useState<Record<string, string>>({});
  const [credentials, setCredentials] = useState<CredentialMeta[]>([]);
  const [newCred, setNewCred] = useState<{ label: string; username: string; secret: string } | null>(null);
  const [credError, setCredError] = useState<string>();

  const canManage = auth.hasPermission("host.manage");
  const canConnect = auth.hasPermission("host.connect");

  const load = useCallback(async () => {
    try {
      setServers(await api.get<ServerAsset[]>("/api/servers"));
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    if (auth.hasPermission("host.read")) void load();
  }, [load, auth.identity?.user_id]);

  const groups = useMemo(() => {
    const map = new Map<string, ServerAsset[]>();
    for (const s of servers) {
      const g = s.group_name || "";
      if (!map.has(g)) map.set(g, []);
      map.get(g)!.push(s);
    }
    return [...map.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [servers]);

  const loadCredentials = useCallback(async () => {
    if (!auth.hasPermission("host.manage")) return;
    try {
      setCredentials(await api.get<CredentialMeta[]>("/api/credentials"));
    } catch {
      setCredentials([]);
    }
  }, [auth.identity?.user_id]);

  const createCredential = async () => {
    if (!newCred?.label || !newCred?.secret) return;
    setCredError(undefined);
    try {
      const meta = await api.post<CredentialMeta>("/api/credentials", {
        kind: "password", label: newCred.label, username: newCred.username, secret: newCred.secret,
      });
      setNewCred(null);
      await loadCredentials();
      setForm((f) => (f ? { ...f, credential_id: meta.id } : f));
    } catch (err) {
      setCredError(err instanceof Error ? err.message : "create credential failed");
    }
  };

  const openForm = (s?: ServerAsset) => {
    void loadCredentials();
    setCredError(undefined);
    setForm(
      s
        ? {
            id: s.id, name: s.name, host: s.host, port: s.port, username: s.username,
            auth_kind: s.auth_kind, identity_file: s.identity_file ?? "",
            credential_id: s.credential_id ?? "", jump_host_id: s.jump_host_id ?? "",
            group_name: s.group_name ?? "", tags: (s.tags ?? []).join(", "), enabled: s.enabled,
          }
        : { ...EMPTY_FORM }
    );
  };

  const saveForm = async () => {
    if (!form) return;
    const payload: ServerAsset = {
      id: form.id || "",
      name: form.name,
      host: form.host,
      port: Number(form.port) || 22,
      username: form.username,
      auth_kind: form.auth_kind,
      identity_file: form.identity_file || undefined,
      credential_id: form.credential_id || undefined,
      jump_host_id: form.jump_host_id || undefined,
      group_name: form.group_name || undefined,
      tags: form.tags.split(",").map((t) => t.trim()).filter(Boolean),
      description: "",
      enabled: form.enabled,
    };
    try {
      await api.post("/api/servers", payload);
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "save failed");
    }
  };

  const remove = async (id: string) => {
    if (!confirm(t("hosts.deleteConfirm"))) return;
    try {
      await api.del(`/api/servers/${id}`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const test = async (id: string) => {
    setTestResult((r) => ({ ...r, [id]: "testing…" }));
    try {
      const res = await api.post<{ ok: boolean; message: string }>(`/api/servers/${id}/test`);
      setTestResult((r) => ({ ...r, [id]: res.message }));
    } catch (err) {
      setTestResult((r) => ({ ...r, [id]: err instanceof Error ? err.message : "test failed" }));
    }
  };

  const pickFile = async (file: File | undefined) => {
    if (!file) return;
    try {
      const text = await file.text();
      setImportText(text);
      setError(undefined);
    } catch {
      setError(t("hosts.fileReadFailed"));
    }
  };

  const doImport = async () => {
    try {
      await api.post("/api/servers/import-ssh-config", { content: importText });
      setImportOpen(false);
      setImportText("");
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "import failed");
    }
  };

  return (
    <Page
      header={
        <PageHeader
          title={t("hosts.title")}
          actions={
            canManage && (
              <>
                <Button variant="outline" size="sm" onClick={() => setImportOpen(true)}>
                  <Upload className="size-3.5" /> {t("hosts.import")}
                </Button>
                <Button size="sm" onClick={() => openForm()}>
                  <Plus className="size-3.5" /> {t("hosts.add")}
                </Button>
              </>
            )
          }
        />
      }
    >
      <ErrorText>{error}</ErrorText>
      {groups.length === 0 && (
        <p className="mt-8 text-center text-sm text-muted-foreground">
          {canManage ? t("hosts.emptyAdmin") : t("hosts.empty")}
        </p>
      )}
      <div className="space-y-5">
        {groups.map(([group, list]) => (
          <section key={group}>
            {group && <h2 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{group}</h2>}
            <div className="overflow-hidden rounded-xl border">
              <table className="w-full text-sm">
                <tbody>
                  {list.map((s) => (
                    <tr key={s.id} className="border-b last:border-0">
                      <td className="px-3 py-2">
                        <div className="font-medium">{s.name}</div>
                        <div className="text-xs text-muted-foreground">
                          {s.username}@{s.host}:{s.port}
                          {!s.enabled && <Badge variant="secondary" className="ml-2">{t("hosts.disabled")}</Badge>}
                        </div>
                      </td>
                      <td className="px-3 py-2 text-xs text-muted-foreground">
                        {testResult[s.id] ?? ""}
                      </td>
                      <td className="px-3 py-2">
                        <div className="flex justify-end gap-1">
                          {canConnect && (
                            <Button size="xs" onClick={() => navigate(`/terminal/${s.id}`)}>
                              <Plug className="size-3" /> {t("hosts.connect")}
                            </Button>
                          )}
                          {canManage && (
                            <>
                              <Button variant="outline" size="xs" onClick={() => void test(s.id)}>{t("hosts.test")}</Button>
                              <Button variant="ghost" size="icon-xs" onClick={() => openForm(s)}><Pencil className="size-3" /></Button>
                              <Button variant="destructive" size="icon-xs" onClick={() => void remove(s.id)}><Trash2 className="size-3" /></Button>
                            </>
                          )}
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        ))}
      </div>

      {/* Host form */}
      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{form?.id ? t("hosts.editHost") : t("hosts.add")}</DialogTitle>
          </DialogHeader>
          {form && (
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1"><Label>{t("common.name")}</Label><Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></div>
              <div className="space-y-1"><Label>{t("hosts.f.group")}</Label><Input value={form.group_name} placeholder={t("hosts.phGroup")} onChange={(e) => setForm({ ...form, group_name: e.target.value })} /></div>
              <div className="space-y-1"><Label>{t("hosts.f.host")}</Label><Input value={form.host} onChange={(e) => setForm({ ...form, host: e.target.value })} /></div>
              <div className="space-y-1"><Label>{t("hosts.f.port")}</Label><Input type="number" value={form.port} onChange={(e) => setForm({ ...form, port: Number(e.target.value) })} /></div>
              <div className="space-y-1"><Label>{t("hosts.f.sshUser")}</Label><Input value={form.username} onChange={(e) => setForm({ ...form, username: e.target.value })} /></div>
              <div className="space-y-1">
                <Label>{t("hosts.f.auth")}</Label>
                <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form.auth_kind}
                  onChange={(e) => setForm({ ...form, auth_kind: e.target.value as ServerAsset["auth_kind"] })}>
                  <option value="agent_or_default_key">{t("hosts.authAgent")}</option>
                  <option value="identity_file">{t("hosts.authIdentity")}</option>
                  <option value="keyring_password">{t("hosts.authCred")}</option>
                </select>
              </div>
              {form.auth_kind === "identity_file" && (
                <div className="col-span-2 space-y-1"><Label>{t("hosts.f.identityPath")}</Label><Input value={form.identity_file} onChange={(e) => setForm({ ...form, identity_file: e.target.value })} /></div>
              )}
              {form.auth_kind === "keyring_password" && (
                <div className="col-span-2 space-y-1.5">
                  <Label>{t("hosts.f.cred")}</Label>
                  <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm"
                    value={form.credential_id}
                    onChange={(e) => setForm({ ...form, credential_id: e.target.value })}>
                    <option value="">{t("hosts.selectCred")}</option>
                    {credentials.map((c) => (
                      <option key={c.id} value={c.id}>{c.label}{c.username ? ` (${c.username})` : ""}</option>
                    ))}
                  </select>
                  {!newCred ? (
                    <Button variant="outline" size="xs" onClick={() => setNewCred({ label: "", username: "root", secret: "" })}>
                      <Plus className="size-3" /> {t("hosts.newCred")}
                    </Button>
                  ) : (
                    <div className="space-y-2 rounded-lg border p-3">
                      <Input placeholder={t("hosts.phLabel")} value={newCred.label} onChange={(e) => setNewCred({ ...newCred, label: e.target.value })} />
                      <Input placeholder={t("hosts.phCredUser")} value={newCred.username} onChange={(e) => setNewCred({ ...newCred, username: e.target.value })} />
                      <Input type="password" placeholder={t("hosts.phPassword")} value={newCred.secret} onChange={(e) => setNewCred({ ...newCred, secret: e.target.value })} />
                      {credError && <p className="text-xs text-destructive">{credError}</p>}
                      <div className="flex gap-2">
                        <Button size="xs" onClick={() => void createCredential()} disabled={!newCred.label || !newCred.secret}>{t("hosts.createSelect")}</Button>
                        <Button size="xs" variant="outline" onClick={() => setNewCred(null)}>{t("common.cancel")}</Button>
                      </div>
                    </div>
                  )}
                </div>
              )}
              <div className="space-y-1"><Label>{t("hosts.f.jump")}</Label><Input value={form.jump_host_id} onChange={(e) => setForm({ ...form, jump_host_id: e.target.value })} /></div>
              <div className="space-y-1"><Label>{t("hosts.f.tags")}</Label><Input value={form.tags} onChange={(e) => setForm({ ...form, tags: e.target.value })} /></div>
              <label className="col-span-2 flex items-center gap-2 text-sm">
                <input type="checkbox" checked={form.enabled} onChange={(e) => setForm({ ...form, enabled: e.target.checked })} />
                {t("common.enabled")}
              </label>
              <div className="col-span-2"><ErrorText>{error}</ErrorText></div>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>{t("common.cancel")}</Button>
            <Button onClick={() => void saveForm()}>{t("common.save")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Import ssh config */}
      <Dialog open={importOpen} onOpenChange={setImportOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader><DialogTitle>{t("hosts.importTitle")}</DialogTitle></DialogHeader>
          <div className="space-y-2">
            <Label>{t("hosts.importPaste")}</Label>
            <textarea
              className="h-48 w-full rounded-lg border bg-background p-2 font-mono text-xs"
              value={importText}
              onChange={(e) => setImportText(e.target.value)}
              placeholder={"Host bastion\n  HostName 10.0.0.9\n  User ops\n\nHost web-*\n  HostName 10.0.0.%h"}
            />
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              {t("hosts.importHint")}
              <label className="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border px-2 py-1 font-medium text-foreground hover:bg-muted">
                {t("hosts.importPick")}
                <input
                  type="file"
                  className="hidden"
                  accept=".conf,.cfg,.txt,text/plain"
                  onChange={(e) => void pickFile(e.target.files?.[0])}
                />
              </label>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setImportOpen(false)}>{t("common.cancel")}</Button>
            <Button onClick={() => void doImport()} disabled={!importText.trim()}>{t("common.save")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
