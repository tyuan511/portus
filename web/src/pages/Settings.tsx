import { useCallback, useEffect, useState } from "react";
import QRCode from "qrcode";
import { KeyRound, LockKeyhole, Pencil, Plus, Trash2 } from "lucide-react";
import { cn } from "cn";
import { ConfirmDialog, PromptDialog } from "@/components/sshield/ConfirmDialog";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n, type Lang } from "@/lib/i18n";
import { SSHIELD_THEMES, applyTheme, getSavedThemeId } from "@/lib/themes";
import type { ApiToken, CredentialKind, CredentialMeta, UserSession } from "@/types";

const THEME_KEY = "/api/settings/ui.theme";

const CRED_KINDS: CredentialKind[] = ["password", "private_key", "api_key"];

const CRED_KIND_ITEMS: Record<CredentialKind, string> = {
  password: "password",
  private_key: "private_key",
  api_key: "api_key",
};

export default function SettingsView() {
  const auth = useAuth();
  const { t, lang, setLang } = useI18n();
  const canUseMcp = auth.hasPermission("mcp.access");
  const canManageCreds = auth.hasPermission("host.manage");
  const [themeId, setThemeId] = useState(getSavedThemeId());
  const [tokens, setTokens] = useState<ApiToken[]>([]);
  const [newTokenName, setNewTokenName] = useState("");
  const [plainToken, setPlainToken] = useState<string>();
  const [error, setError] = useState<string>();
  const [sessions, setSessions] = useState<UserSession[]>([]);

  const [pwd, setPwd] = useState({ current: "", next: "", confirm: "" });
  const [pwdError, setPwdError] = useState<string>();
  const [pwdChanged, setPwdChanged] = useState(false);

  const [creds, setCreds] = useState<CredentialMeta[]>([]);
  const [credForm, setCredForm] = useState<{
    label: string;
    kind: CredentialKind;
    username: string;
    secret: string;
  } | null>(null);
  const [credError, setCredError] = useState<string>();
  const [credNotice, setCredNotice] = useState(false);
  const [renameCred, setRenameCred] = useState<CredentialMeta | null>(null);
  const [deleteCred, setDeleteCred] = useState<CredentialMeta | null>(null);
  const [revokeTokenId, setRevokeTokenId] = useState<string | null>(null);

  const [disableTotpOpen, setDisableTotpOpen] = useState(false);

  const [totpUrl, setTotpUrl] = useState<string>();
  const [totpQr, setTotpQr] = useState<string>();
  const [totpCode, setTotpCode] = useState("");
  const [recoveryCodes, setRecoveryCodes] = useState<string[]>();

  const loadTokens = useCallback(async () => {
    if (!canUseMcp) return;
    try {
      setTokens(await api.get<ApiToken[]>("/api/tokens"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load tokens failed");
    }
  }, [canUseMcp]);

  const loadSessions = useCallback(async () => {
    try {
      setSessions(await api.get<UserSession[]>("/api/auth/sessions"));
    } catch {
      /* fine */
    }
  }, []);

  const loadCreds = useCallback(async () => {
    if (!canManageCreds) return;
    try {
      setCreds(await api.get<CredentialMeta[]>("/api/credentials"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load credentials failed");
    }
  }, [canManageCreds]);

  useEffect(() => {
    void loadTokens();
    void loadSessions();
    void loadCreds();
  }, [loadTokens, loadSessions, loadCreds]);

  const pickTheme = async (id: string) => {
    setThemeId(id);
    applyTheme(id);
    window.dispatchEvent(new CustomEvent("sshield:theme"));
    try {
      await api.put(THEME_KEY, { value: id });
    } catch {
      /* prefer local-only */
    }
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
    try {
      await api.del(`/api/tokens/${id}`);
      setRevokeTokenId(null);
      await loadTokens();
    } catch (err) {
      setRevokeTokenId(null);
      setError(err instanceof Error ? err.message : "revoke failed");
    }
  };

  const changePassword = async () => {
    setPwdError(undefined);
    if (pwd.next !== pwd.confirm) {
      setPwdError(t("settings.passwordMismatch"));
      return;
    }
    try {
      await api.post("/api/auth/change-password", { old_password: pwd.current, new_password: pwd.next });
      setPwd({ current: "", next: "", confirm: "" });
      setPwdChanged(true);
      // The server revoked every session (including this one) — head back to login.
      window.setTimeout(() => void auth.refresh(), 1500);
    } catch (err) {
      setPwdError(err instanceof Error ? err.message : "change password failed");
    }
  };

  const createCredential = async () => {
    if (!credForm?.label || !credForm?.secret) return;
    setCredError(undefined);
    try {
      await api.post<CredentialMeta>("/api/credentials", {
        kind: credForm.kind,
        label: credForm.label,
        username: credForm.username || undefined,
        secret: credForm.secret,
      });
      setCredForm(null);
      setCredNotice(true);
      await loadCreds();
    } catch (err) {
      setCredError(err instanceof Error ? err.message : "create credential failed");
    }
  };

  const renameCredential = async (cred: CredentialMeta, label: string) => {
    try {
      await api.patch(`/api/credentials/${cred.id}`, { label });
      setRenameCred(null);
      await loadCreds();
    } catch (err) {
      setRenameCred(null);
      setError(err instanceof Error ? err.message : "rename failed");
    }
  };

  const deleteCredential = async (cred: CredentialMeta) => {
    try {
      await api.del(`/api/credentials/${cred.id}`);
      setDeleteCred(null);
      await loadCreds();
    } catch (err) {
      setDeleteCred(null);
      setError(err instanceof Error ? err.message : "delete failed");
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

  const disableTotp = async (password: string, code: string) => {
    try {
      await api.post("/api/auth/totp/disable", { password, code });
      setDisableTotpOpen(false);
      await auth.refresh();
    } catch (err) {
      setDisableTotpOpen(false);
      setError(err instanceof Error ? err.message : "totp disable failed");
    }
  };

  const killSession = async (id: string) => {
    try {
      await api.del(`/api/auth/sessions/${id}`);
      await loadSessions();
    } catch (err) {
      setError(err instanceof Error ? err.message : "kick failed");
    }
  };

  const activeTokens = tokens.filter((tk) => !tk.revoked_at);

  return (
    <Page header={<PageHeader title={t("settings.title")} />}>
      <ErrorText>{error}</ErrorText>

      <Tabs defaultValue="appearance" className="gap-5">
        <TabsList variant="line" className="w-full justify-start border-b border-border pb-0">
          <TabsTrigger value="appearance">{t("settings.appearance")}</TabsTrigger>
          <TabsTrigger value="security">{t("settings.security")}</TabsTrigger>
          {canUseMcp && <TabsTrigger value="tokens">{t("settings.tokens")}</TabsTrigger>}
          <TabsTrigger value="totp">{t("settings.totp")}</TabsTrigger>
          <TabsTrigger value="sessions">{t("settings.sessions")}</TabsTrigger>
        </TabsList>

        <TabsContent value="appearance" className="flex max-w-2xl flex-col gap-6">
          <Card>
            <CardHeader>
              <CardTitle>{t("settings.language")}</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="inline-flex rounded-lg border border-border bg-secondary/50 p-0.5">
                {(
                  [
                    { id: "en", label: "English" },
                    { id: "zh-CN", label: "简体中文" },
                  ] as { id: Lang; label: string }[]
                ).map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => setLang(item.id)}
                    className={cn(
                      "rounded-md px-3 py-1.5 text-xs font-medium transition-all",
                      lang === item.id
                        ? "bg-card text-foreground shadow-2xs"
                        : "text-muted-foreground hover:text-foreground"
                    )}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>{t("settings.appearance")}</CardTitle>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              {SSHIELD_THEMES.map((theme) => {
                const isSelected = themeId === theme.id;
                const isDark = theme.mode === "dark";
                return (
                  <button
                    key={theme.id}
                    type="button"
                    onClick={() => void pickTheme(theme.id)}
                    className={cn(
                      "flex flex-col gap-3 rounded-xl border p-3 text-left transition-all",
                      isSelected
                        ? "border-primary ring-2 ring-primary/20"
                        : "border-border hover:border-foreground/30"
                    )}
                  >
                    {/* Mini UI wireframe preview */}
                    <div
                      className={cn(
                        "flex h-20 overflow-hidden rounded-lg border",
                        isDark ? "border-zinc-800 bg-[#000000]" : "border-zinc-200 bg-[#fafafa]"
                      )}
                    >
                      <div
                        className={cn(
                          "w-10 border-r p-1.5",
                          isDark ? "border-zinc-800 bg-[#050505]" : "border-zinc-200 bg-white"
                        )}
                      >
                        <div className="mb-1.5 size-2.5 rounded-sm bg-emerald-500" />
                        <div className={cn("mb-1 h-1 w-full rounded", isDark ? "bg-zinc-800" : "bg-zinc-200")} />
                        <div className={cn("h-1 w-4/5 rounded", isDark ? "bg-zinc-800" : "bg-zinc-200")} />
                      </div>
                      <div className="flex flex-1 flex-col gap-1.5 p-2">
                        <div className="flex items-center justify-between">
                          <div className={cn("h-1.5 w-12 rounded", isDark ? "bg-zinc-700" : "bg-zinc-300")} />
                          <div className={cn("h-2.5 w-7 rounded-sm", isDark ? "bg-zinc-100" : "bg-zinc-900")} />
                        </div>
                        <div
                          className={cn(
                            "flex-1 rounded border p-1.5",
                            isDark ? "border-zinc-800 bg-[#0a0a0a]" : "border-zinc-200 bg-white"
                          )}
                        >
                          <div className={cn("h-1.5 w-2/3 rounded", isDark ? "bg-zinc-800" : "bg-zinc-200")} />
                        </div>
                      </div>
                    </div>

                    <div className="flex items-center justify-between">
                      <div>
                        <div className="text-sm font-medium">{theme.name}</div>
                        <div className="text-xs text-muted-foreground">{theme.description}</div>
                      </div>
                      <Badge variant={isSelected ? "default" : "outline"} className="font-mono text-[10px]">
                        {theme.mode}
                      </Badge>
                    </div>
                  </button>
                );
              })}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="security" className="flex max-w-2xl flex-col gap-6">
          <Card>
            <CardHeader>
              <CardTitle>{t("settings.password")}</CardTitle>
              <CardDescription>{t("settings.passwordTitle")}</CardDescription>
            </CardHeader>
            <CardContent>
              <form
                className="flex max-w-sm flex-col gap-3"
                onSubmit={(e) => {
                  e.preventDefault();
                  void changePassword();
                }}
              >
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="pwd-current">{t("settings.currentPassword")}</Label>
                  <Input
                    id="pwd-current"
                    type="password"
                    autoComplete="current-password"
                    value={pwd.current}
                    onChange={(e) => setPwd({ ...pwd, current: e.target.value })}
                    required
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="pwd-new">{t("settings.newPassword")}</Label>
                  <Input
                    id="pwd-new"
                    type="password"
                    autoComplete="new-password"
                    value={pwd.next}
                    onChange={(e) => setPwd({ ...pwd, next: e.target.value })}
                    required
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="pwd-confirm">{t("settings.confirmPassword")}</Label>
                  <Input
                    id="pwd-confirm"
                    type="password"
                    autoComplete="new-password"
                    value={pwd.confirm}
                    onChange={(e) => setPwd({ ...pwd, confirm: e.target.value })}
                    required
                  />
                </div>
                {pwdError && <ErrorText>{pwdError}</ErrorText>}
                {pwdChanged && (
                  <Alert>
                    <KeyRound />
                    <AlertTitle>{t("settings.passwordChanged")}</AlertTitle>
                  </Alert>
                )}
                <Button size="sm" type="submit" className="w-fit">
                  {t("common.save")}
                </Button>
              </form>
            </CardContent>
          </Card>

          {canManageCreds && (
            <section className="flex flex-col gap-4">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-semibold tracking-tight">{t("settings.credentials")}</h3>
                <Button
                  size="sm"
                  onClick={() => {
                    setCredError(undefined);
                    setCredForm({ label: "", kind: "password", username: "", secret: "" });
                  }}
                >
                  <Plus data-icon="inline-start" />
                  {t("common.create")}
                </Button>
              </div>

              {credNotice && (
                <Alert>
                  <KeyRound />
                  <AlertTitle>{t("settings.credCreated")}</AlertTitle>
                  <AlertDescription className="mt-2">
                    <Button size="xs" variant="outline" className="w-fit" onClick={() => setCredNotice(false)}>
                      {t("common.dismiss")}
                    </Button>
                  </AlertDescription>
                </Alert>
              )}

              {creds.length === 0 ? (
                <Empty className="border border-border bg-card py-12">
                  <EmptyHeader>
                    <EmptyMedia variant="icon">
                      <LockKeyhole />
                    </EmptyMedia>
                    <EmptyTitle>{t("settings.credentials")}</EmptyTitle>
                    <EmptyDescription>{t("settings.credEmpty")}</EmptyDescription>
                  </EmptyHeader>
                </Empty>
              ) : (
                <Card className="p-0">
                  <Table>
                    <TableHeader className="bg-secondary/50">
                      <TableRow>
                        <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                          {t("settings.credLabel")}
                        </TableHead>
                        <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                          {t("settings.credKind")}
                        </TableHead>
                        <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                          {t("settings.credColUser")}
                        </TableHead>
                        <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                          {t("settings.colCreated")}
                        </TableHead>
                        <TableHead className="px-4" />
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {creds.map((c) => (
                        <TableRow key={c.id}>
                          <TableCell className="px-4 font-medium">{c.label}</TableCell>
                          <TableCell className="px-4">
                            <Badge variant="outline" className="font-mono text-[10px]">
                              {c.kind}
                            </Badge>
                          </TableCell>
                          <TableCell className="px-4 font-mono text-xs text-muted-foreground">
                            {c.username || "-"}
                          </TableCell>
                          <TableCell className="px-4 font-mono text-xs tabular-nums">
                            {new Date(c.created_at).toLocaleDateString()}
                          </TableCell>
                          <TableCell className="px-4 text-right">
                            <div className="flex justify-end gap-1">
                              <Button variant="ghost" size="icon-xs" onClick={() => setRenameCred(c)}>
                                <Pencil />
                              </Button>
                              <Button variant="destructive" size="icon-xs" onClick={() => setDeleteCred(c)}>
                                <Trash2 />
                              </Button>
                            </div>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </Card>
              )}
            </section>
          )}
        </TabsContent>

        {canUseMcp && (
          <TabsContent value="tokens" className="flex flex-col gap-4">
            <div className="flex max-w-md gap-2">
              <Input
                placeholder={t("settings.tokenName")}
                value={newTokenName}
                onChange={(e) => setNewTokenName(e.target.value)}
              />
              <Button size="sm" onClick={() => void createToken()}>
                <Plus data-icon="inline-start" />
                {t("common.create")}
              </Button>
            </div>

            {plainToken && (
              <Alert>
                <KeyRound />
                <AlertTitle>{t("settings.tokenOnce")}</AlertTitle>
                <AlertDescription className="mt-2 flex flex-col gap-2">
                  <code className="block select-all break-all rounded-md border border-border bg-secondary px-2.5 py-2 font-mono text-xs text-foreground">
                    {plainToken}
                  </code>
                  <p className="text-xs text-muted-foreground">
                    {t("settings.mcpEndpoint")} <code>POST {"{host}"}/mcp</code> {t("settings.withHeader")}{" "}
                    <code>Authorization: Bearer &lt;token&gt;</code>
                  </p>
                  <Button
                    size="xs"
                    variant="outline"
                    className="w-fit"
                    onClick={() => setPlainToken(undefined)}
                  >
                    {t("common.dismiss")}
                  </Button>
                </AlertDescription>
              </Alert>
            )}

            {activeTokens.length === 0 ? (
              <Empty className="border border-border bg-card py-12">
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <KeyRound />
                  </EmptyMedia>
                  <EmptyTitle>{t("settings.tokens")}</EmptyTitle>
                  <EmptyDescription>{t("settings.noTokens")}</EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              <Card className="p-0">
                <Table>
                  <TableHeader className="bg-secondary/50">
                    <TableRow>
                      <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                        {t("settings.colName")}
                      </TableHead>
                      <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                        {t("settings.colPrefix")}
                      </TableHead>
                      <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                        {t("settings.colCreated")}
                      </TableHead>
                      <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                        {t("settings.colLastUsed")}
                      </TableHead>
                      <TableHead className="px-4" />
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {activeTokens.map((tk) => (
                      <TableRow key={tk.id}>
                        <TableCell className="px-4 font-medium">{tk.name}</TableCell>
                        <TableCell className="px-4 font-mono text-xs text-muted-foreground">
                          {tk.prefix}…
                        </TableCell>
                        <TableCell className="px-4 font-mono text-xs tabular-nums">
                          {new Date(tk.created_at).toLocaleDateString()}
                        </TableCell>
                        <TableCell className="px-4 font-mono text-xs text-muted-foreground tabular-nums">
                          {tk.last_used_at ? new Date(tk.last_used_at).toLocaleString() : t("common.never")}
                        </TableCell>
                        <TableCell className="px-4 text-right">
                          <Button
                            variant="destructive"
                            size="icon-xs"
                            onClick={() => setRevokeTokenId(tk.id)}
                          >
                            <Trash2 />
                          </Button>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </Card>
            )}
          </TabsContent>
        )}

        <TabsContent value="totp">
          <Card className="max-w-md">
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                {t("settings.totp")}
                {auth.identity && (
                  <Badge variant={auth.hasTotp ? "default" : "secondary"}>
                    {auth.hasTotp ? t("settings.totpOn") : t("settings.totpOff")}
                  </Badge>
                )}
              </CardTitle>
              <CardDescription>{t("common.status")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              {auth.identity && (
                <>
                  {auth.hasTotp ? (
                    <Button
                      variant="destructive"
                      size="sm"
                      className="w-fit"
                      onClick={() => setDisableTotpOpen(true)}
                    >
                      {t("settings.disable2fa")}
                    </Button>
                  ) : totpUrl ? (
                    <div className="flex flex-col gap-3">
                      {totpQr && (
                        <img
                          src={totpQr}
                          alt={t("settings.qrAlt")}
                          className="size-48 rounded-lg border border-border bg-white p-1"
                        />
                      )}
                      <p className="break-all font-mono text-xs text-muted-foreground">{totpUrl}</p>
                      <div className="flex gap-2">
                        <Input
                          placeholder={t("settings.codePlaceholder")}
                          maxLength={6}
                          className="font-mono"
                          value={totpCode}
                          onChange={(e) => setTotpCode(e.target.value.replace(/\D/g, ""))}
                        />
                        <Button size="sm" onClick={() => void enableTotp()} disabled={totpCode.length !== 6}>
                          {t("settings.enable")}
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <Button size="sm" className="w-fit" onClick={() => void startTotp()}>
                      {t("settings.setupTotp")}
                    </Button>
                  )}

                  {recoveryCodes && (
                    <Alert>
                      <AlertTitle>{t("settings.recoveryCodes")}</AlertTitle>
                      <AlertDescription className="mt-2 grid grid-cols-2 gap-1 font-mono text-xs text-foreground">
                        {recoveryCodes.map((c) => (
                          <span key={c}>{c}</span>
                        ))}
                      </AlertDescription>
                    </Alert>
                  )}
                </>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="sessions">
          <Card className="p-0">
            <Table>
              <TableHeader className="bg-secondary/50">
                <TableRow>
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
                {sessions.map((s) => (
                  <TableRow key={s.id}>
                    <TableCell className="px-4 font-mono text-xs">{s.ip}</TableCell>
                    <TableCell
                      className="max-w-72 truncate px-4 text-xs text-muted-foreground"
                      title={s.user_agent}
                    >
                      {s.user_agent}
                    </TableCell>
                    <TableCell className="px-4 font-mono text-xs tabular-nums">
                      {new Date(s.last_seen_at).toLocaleString()}
                    </TableCell>
                    <TableCell className="px-4 text-right">
                      <Button variant="destructive" size="xs" onClick={() => void killSession(s.id)}>
                        {t("sessions.signOut")}
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </Card>
        </TabsContent>
      </Tabs>

      <Dialog open={credForm !== null} onOpenChange={(o) => !o && setCredForm(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("settings.credentials")}</DialogTitle>
          </DialogHeader>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>{t("settings.credLabel")}</Label>
              <Input
                value={credForm?.label ?? ""}
                onChange={(e) => setCredForm({ ...credForm!, label: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("settings.credKind")}</Label>
              <Select
                value={credForm?.kind ?? "password"}
                items={CRED_KIND_ITEMS}
                onValueChange={(v) => v && setCredForm({ ...credForm!, kind: v as CredentialKind })}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {CRED_KINDS.map((k) => (
                      <SelectItem key={k} value={k}>
                        {k}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("settings.credUsername")}</Label>
              <Input
                value={credForm?.username ?? ""}
                onChange={(e) => setCredForm({ ...credForm!, username: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("settings.credSecret")}</Label>
              <Input
                type="password"
                value={credForm?.secret ?? ""}
                onChange={(e) => setCredForm({ ...credForm!, secret: e.target.value })}
              />
            </div>
            <div className="col-span-2">
              <ErrorText>{credError}</ErrorText>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCredForm(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void createCredential()} disabled={!credForm?.label || !credForm?.secret}>
              {t("common.create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <PromptDialog
        open={renameCred !== null}
        title={t("settings.credRename")}
        fields={[
          {
            key: "label",
            label: t("settings.credLabel"),
            placeholder: renameCred?.label,
            defaultValue: renameCred?.label ?? "",
          },
        ]}
        confirmLabel={t("common.save")}
        onSubmit={(v) => renameCred && void renameCredential(renameCred, v.label)}
        onOpenChange={(o) => !o && setRenameCred(null)}
      />

      <ConfirmDialog
        open={revokeTokenId !== null}
        title={t("common.delete")}
        description={t("settings.revokeConfirm")}
        confirmLabel={t("common.delete")}
        onConfirm={() => revokeTokenId && void revokeToken(revokeTokenId)}
        onOpenChange={(o) => !o && setRevokeTokenId(null)}
      />

      <ConfirmDialog
        open={deleteCred !== null}
        title={t("common.delete")}
        description={t("settings.credDeleteConfirm", { name: deleteCred?.label ?? "" })}
        confirmLabel={t("common.delete")}
        onConfirm={() => deleteCred && void deleteCredential(deleteCred)}
        onOpenChange={(o) => !o && setDeleteCred(null)}
      />

      <PromptDialog
        open={disableTotpOpen}
        title={t("settings.disable2fa")}
        fields={[
          {
            key: "password",
            label: t("settings.disablePwdPrompt"),
            type: "password",
            autoComplete: "current-password",
          },
          { key: "code", label: t("settings.codePrompt"), autoComplete: "one-time-code" },
        ]}
        onSubmit={(v) => void disableTotp(v.password, v.code)}
        onOpenChange={setDisableTotpOpen}
      />
    </Page>
  );
}
