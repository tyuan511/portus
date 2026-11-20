import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { ErrorText } from "@/components/ui/shared";

export default function ChangePasswordDialog({ open, onDone }: { open: boolean; onDone: () => void }) {
  const auth = useAuth();
  const [oldPassword, setOld] = useState("");
  const [newPassword, setNew] = useState("");
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    setError(undefined);
    try {
      await api.post("/api/auth/change-password", { old_password: oldPassword, new_password: newPassword });
      auth.setMustChangePassword(false);
      await auth.logout();
      onDone();
    } catch (err) {
      setError(err instanceof Error ? err.message : "change failed");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Change your password</DialogTitle>
          <DialogDescription>
            An administrator issued you a temporary password. Set a new one to continue.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1">
            <Label htmlFor="cp-old">Temporary password</Label>
            <Input id="cp-old" type="password" value={oldPassword} onChange={(e) => setOld(e.target.value)} />
          </div>
          <div className="space-y-1">
            <Label htmlFor="cp-new">New password</Label>
            <Input id="cp-new" type="password" value={newPassword} onChange={(e) => setNew(e.target.value)} />
            <p className="text-xs text-muted-foreground">Min 10 chars; no whitespace; not containing your username.</p>
          </div>
          <ErrorText>{error}</ErrorText>
        </div>
        <DialogFooter>
          <Button onClick={submit} disabled={busy || !oldPassword || !newPassword}>
            {busy ? "Saving…" : "Set new password"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
