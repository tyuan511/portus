import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { Lock } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useAuth } from "@/lib/auth";
import { api, ApiError, TOTP_REQUIRED } from "@/lib/api";
import { ErrorText } from "@/components/ui/shared";
import ChangePasswordDialog from "@/components/ChangePasswordDialog";

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

  // 强制改密流程：登录成功后 must_change_password=true 时由 AuthProvider 驱动。
  if (auth.identity && auth.mustChangePassword) {
    return <ChangePasswordDialog open onDone={() => navigate("/")} />;
  }
  if (auth.identity) {
    navigate("/");
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <Card className="w-full max-w-sm">
        <CardHeader className="items-center text-center">
          <div className="mx-auto mb-2 flex size-12 items-center justify-center rounded-xl bg-primary text-primary-foreground">
            <Lock className="size-6" />
          </div>
          <CardTitle className="text-lg">SSHield Enterprise</CardTitle>
          <CardDescription>Sign in to your operations console</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submit} className="space-y-3">
            <div className="space-y-1">
              <Label htmlFor="username">Username</Label>
              <Input id="username" value={username} autoFocus required autoComplete="username"
                onChange={(e) => setUsername(e.target.value)} />
            </div>
            <div className="space-y-1">
              <Label htmlFor="password">Password</Label>
              <Input id="password" type="password" value={password} required autoComplete="current-password"
                onChange={(e) => setPassword(e.target.value)} />
            </div>
            {needTotp && (
              <div className="space-y-1">
                <Label htmlFor="totp">Authenticator code</Label>
                <Input id="totp" inputMode="numeric" pattern="[0-9]*" maxLength={6} value={totpCode}
                  onChange={(e) => setTotpCode(e.target.value.replace(/\D/g, ""))} />
              </div>
            )}
            <ErrorText>{error}</ErrorText>
            <Button type="submit" className="w-full" disabled={busy}>
              {busy ? "Signing in…" : "Sign in"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
