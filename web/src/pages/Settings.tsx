import { useCallback, useEffect, useState } from "react";
import QRCode from "qrcode";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import { cn } from "cn";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n, type Lang } from "@/lib/i18n";
import { SSHIELD_THEMES, applyTheme, getSavedThemeId } from "@/lib/themes";
import type { ApiToken, UserSession } from "@/types";

const THEME_KEY = "/api/settings/ui.theme";

export default function SettingsView() {
  const auth = useAuth();
  const { t, lang, setLang } = useI18n();
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

  useEffect(() => {
    void loadTokens();
    void loadSessions();
  }, [loadTokens, loadSessions]);

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
    if (!confirm(t("settings.revokeConfirm"))) return;
    try {
      await api.del(`/api/tokens/${id}`);
      await loadTokens();
    } catch (err) {
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
    const password = prompt(t("settings.disablePwdPrompt"));
    if (!password) return;
    const code = prompt(t("settings.codePrompt"));
    if (!code) return;
    try {
      await api.post("/api/auth/totp/disable", { password, code });
      await auth.refresh();
    } catch (err) {
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
                            onClick={() => void revokeToken(tk.id)}
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
                    <Button variant="destructive" size="sm" className="w-fit" onClick={() => void disableTotp()}>
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
    </Page>
  );
}
