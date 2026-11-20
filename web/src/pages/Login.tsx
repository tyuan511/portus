import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { ArrowRight, Eye, EyeOff, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { BrandLogo } from "@/components/BrandLogo";
import { ErrorText } from "@/components/ui/shared";
import { GridCrosshair } from "@/components/sshield/GridCrosshair";
import { LangToggle } from "@/components/sshield/LangToggle";
import { ThemeToggle } from "@/components/sshield/ThemeToggle";
import ChangePasswordDialog from "@/components/ChangePasswordDialog";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { ApiError, TOTP_REQUIRED } from "@/lib/api";

export default function Login() {
  const auth = useAuth();
  const { t } = useI18n();
  const navigate = useNavigate();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPwd, setShowPwd] = useState(false);
  const [totpCode, setTotpCode] = useState("");
  const [needTotp, setNeedTotp] = useState(false);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      await auth.login(username, password, needTotp ? totpCode : undefined);
    } catch (err) {
      if (err instanceof ApiError && err.code === TOTP_REQUIRED) {
        setNeedTotp(true);
        setError(t("login.totpPrompt"));
      } else {
        setError(err instanceof Error ? err.message : "login failed");
      }
    } finally {
      setBusy(false);
    }
  };

  if (auth.identity && auth.mustChangePassword) {
    return <ChangePasswordDialog open onDone={() => navigate("/")} />;
  }
  if (auth.identity) {
    navigate("/");
  }

  return (
    <div className="relative flex min-h-screen flex-col bg-background text-foreground">
      <div
        aria-hidden="true"
        className="vercel-grid-bg pointer-events-none absolute inset-0 [mask-image:radial-gradient(ellipse_70%_60%_at_50%_35%,#000_25%,transparent_100%)]"
      />
      <div aria-hidden="true" className="vercel-hero-glow pointer-events-none absolute inset-0" />

      <header className="relative z-10 border-b border-border/80 bg-background/80 backdrop-blur-md">
        <div className="mx-auto flex h-14 max-w-6xl items-center justify-between px-4 sm:px-6">
          <Link to="/" className="flex items-center gap-2.5">
            <BrandLogo size={24} />
            <span className="text-sm font-semibold tracking-tight">SSHield</span>
          </Link>
          <div className="flex items-center gap-1.5">
            <LangToggle compact />
            <ThemeToggle compact />
            <Button variant="ghost" size="sm" onClick={() => navigate("/")}>
              {t("login.back")}
            </Button>
          </div>
        </div>
      </header>

      <main className="relative z-10 flex flex-1 items-center justify-center px-4 py-12">
        <div className="relative w-full max-w-[400px]">
          <GridCrosshair position="top-left" />
          <GridCrosshair position="top-right" />
          <GridCrosshair position="bottom-left" />
          <GridCrosshair position="bottom-right" />

          <Card className="vercel-surface bg-card/95 backdrop-blur-sm">
            <CardHeader className="border-b border-border/70 pb-4">
              <div className="mb-2 flex items-center gap-2">
                <div className="flex size-8 items-center justify-center rounded-lg border border-border bg-secondary">
                  <ShieldCheck className="size-4 text-foreground" />
                </div>
                <span className="font-mono text-[11px] uppercase tracking-wider text-muted-foreground">
                  SSHield Auth
                </span>
              </div>
              <CardTitle className="text-2xl font-semibold tracking-tight">
                {t("login.title")}
              </CardTitle>
              <CardDescription>{t("login.subtitle")}</CardDescription>
            </CardHeader>

            <CardContent className="pt-5">
              <form onSubmit={submit} className="flex flex-col gap-4">
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="username">{t("login.username")}</Label>
                  <Input
                    id="username"
                    value={username}
                    autoFocus
                    required
                    autoComplete="username"
                    className="h-9"
                    onChange={(e) => setUsername(e.target.value)}
                  />
                </div>

                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="password">{t("login.password")}</Label>
                  <div className="relative">
                    <Input
                      id="password"
                      type={showPwd ? "text" : "password"}
                      value={password}
                      required
                      autoComplete="current-password"
                      className="h-9 pr-9"
                      onChange={(e) => setPassword(e.target.value)}
                    />
                    <button
                      type="button"
                      tabIndex={-1}
                      onClick={() => setShowPwd((v) => !v)}
                      aria-label={showPwd ? "Hide password" : "Show password"}
                      className="absolute inset-y-0 right-2 flex items-center text-muted-foreground transition-colors hover:text-foreground"
                    >
                      {showPwd ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                    </button>
                  </div>
                </div>

                {needTotp && (
                  <div className="flex flex-col gap-1.5">
                    <Label htmlFor="totp">{t("login.totp")}</Label>
                    <Input
                      id="totp"
                      inputMode="numeric"
                      pattern="[0-9]*"
                      maxLength={6}
                      placeholder="000000"
                      className="h-9 text-center font-mono tracking-[0.3em]"
                      value={totpCode}
                      onChange={(e) => setTotpCode(e.target.value.replace(/\D/g, ""))}
                    />
                  </div>
                )}

                <ErrorText>{error}</ErrorText>

                <Button type="submit" className="mt-1 h-9 w-full font-medium" disabled={busy}>
                  {busy ? t("login.signingin") : t("login.submit")}
                  {!busy && <ArrowRight data-icon="inline-end" />}
                </Button>
              </form>
            </CardContent>

            <CardFooter className="justify-between text-xs text-muted-foreground">
              <span>{t("login.provisioned")}</span>
              <Link to="/" className="font-medium text-foreground underline underline-offset-4">
                {t("login.overview")}
              </Link>
            </CardFooter>
          </Card>
        </div>
      </main>

      <footer className="relative z-10 border-t border-border/80 bg-card/50 py-5">
        <div className="mx-auto max-w-6xl px-4 text-center font-mono text-xs text-muted-foreground sm:px-6 sm:text-left">
          {t("login.footer")}
        </div>
      </footer>
    </div>
  );
}
