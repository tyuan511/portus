import { useCallback, useEffect, useState } from "react";
import QRCode from "qrcode";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { SSHIELD_THEMES, applyTheme, getSavedThemeId } from "@/lib/themes";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { ApiToken, UserSession } from "@/types";

const THEME_KEY = "/api/settings/ui.theme";

export default function SettingsView() {
  const auth = useAuth();
  const canUseMcp = auth.hasPermission("mcp.access");
  const [themeId, setThemeId] = useState(getSavedThemeId());
  const [tokens, setTokens] = useState<ApiToken[]>([]);
  const [newTokenName, setNewTokenName] = useState("");
  const [plainToken, setPlainToken] = useState<string>();
  const [error, setError] = useState<string>();
  const [sessions, setSessions] = useState<UserSession[]>([]);

  const [totpUrl, setTotpUrl] = useState<string>();
  const [totpQr, setTotpQr] = useState<string>();
  const [totpCode, setTotpCode] = useState("");
  const [recoveryCodes, setRecoveryCodes] = useState<string[]>();

  const loadTokens = useCallback(async () => {
    if (!canUseMcp) return;
    try { setTokens(await api.get<ApiToken[]>("/api/tokens")); } catch (err) {
      setError(err instanceof Error ? err.message : "load tokens failed");
    }
  }, [canUseMcp]);

  const loadSessions = useCallback(async () => {
    try { setSessions(await api.get<UserSession[]>("/api/auth/sessions")); } catch { /* fine */ }
  }, []);

  useEffect(() => {
    void loadTokens();
    void loadSessions();
  }, [loadTokens, loadSessions]);

  const pickTheme = async (id: string) => {
    setThemeId(id);
    applyTheme(id);
    try {
      await api.put(THEME_KEY, { value: id });
    } catch { /* prefer local-only */ }
  };

  const createToken = async () => {
    try {
      const res = await api.post<{ token: string }>("/api/tokens", { name: newTokenName || "mcp" });
      setPlainToken(res.token);
      setNewTokenName("");
      await loadTokens();
    } catch (err) {
      setError(err instanceof Error ? err.message : "create token failed");
    }
  };

  const revokeToken = async (id: string) => {
    if (!confirm("Revoke this token? MCP clients using it will stop working.")) return;
    try { await api.del(`/api/tokens/${id}`); await loadTokens(); } catch (err) {
      setError(err instanceof Error ? err.message : "revoke failed");
    }
  };

  const startTotp = async () => {
    try {
      const res = await api.post<{ secret: string; otpauth_url: string }>("/api/auth/totp/setup");
      setTotpUrl(res.otpauth_url);
      setTotpQr(await QRCode.toDataURL(res.otpauth_url));
    } catch (err) {
      setError(err instanceof Error ? err.message : "totp setup failed");
    }
  };

  const enableTotp = async () => {
    try {
      const res = await api.post<{ recovery_codes: string[] }>("/api/auth/totp/enable", { code: totpCode });
      setRecoveryCodes(res.recovery_codes);
      setTotpUrl(undefined);
      setTotpQr(undefined);
      await auth.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "totp enable failed");
    }
  };

  const disableTotp = async () => {
    const password = prompt("Confirm your password to disable 2FA:");
    if (!password) return;
    const code = prompt("Current authenticator code (or recovery code):");
    if (!code) return;
    try {
      await api.post("/api/auth/totp/disable", { password, code });
      await auth.refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "totp disable failed");
    }
  };

  const killSession = async (id: string) => {
    try { await api.del(`/api/auth/sessions/${id}`); await loadSessions(); } catch (err) {
      setError(err instanceof Error ? err.message : "kick failed");
    }
  };

  return (
    <Page header={<PageHeader title="Settings" subtitle="Appearance, API tokens, two-factor, active sessions" />}>
      <ErrorText>{error}</ErrorText>
      <Tabs defaultValue="appearance">
        <TabsList>
          <TabsTrigger value="appearance">Appearance</TabsTrigger>
          {canUseMcp && <TabsTrigger value="tokens">API Tokens</TabsTrigger>}
          <TabsTrigger value="totp">Two-Factor</TabsTrigger>
          <TabsTrigger value="sessions">Sessions</TabsTrigger>
        </TabsList>

        <TabsContent value="appearance">
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-6">
            {SSHIELD_THEMES.map((t) => (
              <button key={t.id}
                className={`rounded-xl border p-2 text-left transition ${themeId === t.id ? "border-primary ring-2 ring-primary/40" : "hover:border-foreground/30"}`}
                onClick={() => void pickTheme(t.id)}>
                <div className="flex gap-1">
                  {t.swatches.map((c, i) => <span key={i} className="size-4 rounded-full border" style={{ background: c }} />)}
                </div>
                <div className="mt-1.5 text-xs font-medium">{t.name}</div>
                <div className="text-[10px] text-muted-foreground">{t.mode}</div>
              </button>
            ))}
          </div>
        </TabsContent>

        {canUseMcp && (
          <TabsContent value="tokens">
            <div className="mb-3 flex max-w-md gap-2">
              <Input placeholder="Token name (e.g. claude-code)" value={newTokenName} onChange={(e) => setNewTokenName(e.target.value)} />
              <Button size="sm" onClick={() => void createToken()}><Plus className="size-3.5" /> Create</Button>
            </div>
            {plainToken && (
              <div className="mb-3 rounded-xl border border-primary/40 bg-primary/5 p-3">
                <p className="mb-1 text-xs font-medium">Copy this token now — it is shown only once:</p>
                <code className="block select-all break-all font-mono text-xs">{plainToken}</code>
                <p className="mt-2 text-xs text-muted-foreground">MCP endpoint: <code>POST {"{host}"}/mcp</code> with header <code>Authorization: Bearer &lt;token&gt;</code></p>
                <Button size="xs" variant="outline" className="mt-1" onClick={() => setPlainToken(undefined)}>Dismiss</Button>
              </div>
            )}
            <div className="overflow-hidden rounded-xl border">
              <table className="w-full text-sm">
                <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
                  <tr><th className="px-3 py-2">Name</th><th className="px-3 py-2">Prefix</th><th className="px-3 py-2">Created</th><th className="px-3 py-2">Last used</th><th className="px-3 py-2" /></tr>
                </thead>
                <tbody>
                  {tokens.filter((t) => !t.revoked_at).map((t) => (
                    <tr key={t.id} className="border-b last:border-0">
                      <td className="px-3 py-1.5 font-medium">{t.name}</td>
                      <td className="px-3 py-1.5 font-mono text-xs">{t.prefix}…</td>
                      <td className="px-3 py-1.5 text-xs">{new Date(t.created_at).toLocaleDateString()}</td>
                      <td className="px-3 py-1.5 text-xs">{t.last_used_at ? new Date(t.last_used_at).toLocaleString() : "never"}</td>
                      <td className="px-3 py-1.5 text-right">
                        <Button variant="destructive" size="icon-xs" onClick={() => void revokeToken(t.id)}><Trash2 className="size-3" /></Button>
                      </td>
                    </tr>
                  ))}
                  {tokens.filter((t) => !t.revoked_at).length === 0 && (
                    <tr><td colSpan={5} className="px-3 py-6 text-center text-sm text-muted-foreground">No active tokens. Create one to connect MCP clients.</td></tr>
                  )}
                </tbody>
              </table>
            </div>
          </TabsContent>
        )}

        <TabsContent value="totp">
          <div className="max-w-md space-y-3">
            {!auth.identity ? null : (
              <>
                <div className="flex items-center gap-2 text-sm">
                  Status:
                  {auth.identity && <Badge variant={auth.hasTotp ? "default" : "secondary"}>{auth.hasTotp ? "enabled" : "disabled"}</Badge>}
                </div>
                {auth.hasTotp ? (
                  <Button variant="destructive" size="sm" onClick={() => void disableTotp()}>Disable two-factor</Button>
                ) : totpUrl ? (
                  <div className="space-y-2">
                    {totpQr && <img src={totpQr} alt="TOTP QR code" className="size-48 rounded-lg border bg-white p-1" />}
                    <p className="break-all text-xs text-muted-foreground">{totpUrl}</p>
                    <div className="flex gap-2">
                      <Input placeholder="6-digit code" maxLength={6} value={totpCode} onChange={(e) => setTotpCode(e.target.value.replace(/\D/g, ""))} />
                      <Button size="sm" onClick={() => void enableTotp()} disabled={totpCode.length !== 6}>Enable</Button>
                    </div>
                  </div>
                ) : (
                  <Button size="sm" onClick={() => void startTotp()}>Set up authenticator</Button>
                )}
                {recoveryCodes && (
                  <div className="rounded-xl border border-primary/40 bg-primary/5 p-3">
                    <p className="mb-1 text-xs font-medium">Recovery codes (store safely — shown once):</p>
                    <div className="grid grid-cols-2 gap-1 font-mono text-xs">
                      {recoveryCodes.map((c) => <span key={c}>{c}</span>)}
                    </div>
                  </div>
                )}
              </>
            )}
          </div>
        </TabsContent>

        <TabsContent value="sessions">
          <div className="overflow-hidden rounded-xl border">
            <table className="w-full text-sm">
              <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
                <tr><th className="px-3 py-2">IP</th><th className="px-3 py-2">User agent</th><th className="px-3 py-2">Last seen</th><th className="px-3 py-2" /></tr>
              </thead>
              <tbody>
                {sessions.map((s) => (
                  <tr key={s.id} className="border-b last:border-0">
                    <td className="px-3 py-1.5 text-xs">{s.ip}</td>
                    <td className="max-w-72 truncate px-3 py-1.5 text-xs text-muted-foreground" title={s.user_agent}>{s.user_agent}</td>
                    <td className="px-3 py-1.5 text-xs">{new Date(s.last_seen_at).toLocaleString()}</td>
                    <td className="px-3 py-1.5 text-right">
                      <Button variant="destructive" size="xs" onClick={() => void killSession(s.id)}>Sign out</Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </TabsContent>
      </Tabs>
    </Page>
  );
}
