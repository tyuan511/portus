import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useAuth } from "@/lib/auth";
import { api, ApiError, TOTP_REQUIRED } from "@/lib/api";
import { ErrorText } from "@/components/ui/shared";
import ChangePasswordDialog from "@/components/ChangePasswordDialog";

// Supabaze-inspired sign-in (DESIGN.md): white canvas, hairline inputs at 6px
// radius, and a single emerald CTA carrying near-black ink — no gradients,
// no decorative chrome.
export default function Login() {
  const auth = useAuth();
  const navigate = useNavigate();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
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
        setError("Enter your 6-digit authenticator code");
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
    <div className="flex min-h-screen flex-col bg-background text-foreground">
      <header className="border-b border-border">
        <div className="mx-auto flex h-14 max-w-6xl items-center justify-between px-6">
          <Link to="/" className="flex items-center gap-2">
            <div className="flex size-6 items-center justify-center rounded-md bg-primary font-bold text-primary-foreground">S</div>
            <span className="text-sm font-medium tracking-tight">SSHield</span>
          </Link>
          <Link to="/" className="text-sm text-muted-foreground hover:text-foreground">
            Back to home
          </Link>
        </div>
      </header>

      <main className="flex flex-1 items-center justify-center px-4 py-16">
        <div className="w-full max-w-sm">
          <h1 className="text-3xl font-medium tracking-tight">Sign in</h1>
          <p className="mt-1.5 text-sm text-muted-foreground">
            to your SSHield operations console
          </p>

          <form onSubmit={submit} className="mt-8 space-y-5">
            <div className="space-y-1.5">
              <Label htmlFor="username" className="text-foreground">Username</Label>
              <Input id="username" value={username} autoFocus required autoComplete="username"
                onChange={(e) => setUsername(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="password" className="text-foreground">Password</Label>
              <Input id="password" type="password" value={password} required autoComplete="current-password"
                onChange={(e) => setPassword(e.target.value)} />
            </div>
            {needTotp && (
              <div className="space-y-1.5">
                <Label htmlFor="totp" className="text-foreground">Authenticator code</Label>
                <Input id="totp" inputMode="numeric" pattern="[0-9]*" maxLength={6} value={totpCode}
                  onChange={(e) => setTotpCode(e.target.value.replace(/\D/g, ""))} />
              </div>
            )}
            <ErrorText>{error}</ErrorText>
            <Button type="submit" className="w-full font-medium" disabled={busy}>
              {busy ? "Signing in…" : "Sign in"}
            </Button>
          </form>

          <p className="mt-6 text-sm text-muted-foreground">
            Accounts are provisioned by your administrator.{" "}
            <Link to="/" className="underline underline-offset-2 hover:text-foreground">
              Back to overview
            </Link>
          </p>
        </div>
      </main>

      <footer className="border-t border-border py-6">
        <div className="mx-auto max-w-6xl px-6 text-xs text-muted-foreground">
          SSHield Enterprise · RBAC-scoped access with a tamper-evident audit trail
        </div>
      </footer>
    </div>
  );
}
