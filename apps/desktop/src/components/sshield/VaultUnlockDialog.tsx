import { useState } from "react";
import { Lock } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { VaultLockState } from "@/types";

interface VaultUnlockDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  state: VaultLockState;
  onSubmit: (passphrase: string) => Promise<void>;
}

export function VaultUnlockDialog({
  open,
  onOpenChange,
  state,
  onSubmit,
}: VaultUnlockDialogProps) {
  const [passphrase, setPassphrase] = useState("");
  const [error, setError] = useState<string | null>(null);

  const isInit = state === "uninitialized";

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await onSubmit(passphrase);
      setPassphrase("");
      onOpenChange(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="border-border bg-card text-foreground sm:max-w-sm">
        <DialogHeader>
          <div className="flex items-center gap-2.5">
            <div className="flex size-9 items-center justify-center rounded-lg bg-indigo-500/20 text-indigo-400">
              <Lock className="size-4" />
            </div>
            <div>
              <DialogTitle>{isInit ? "初始化加密密库" : "解锁 Sshield 密库"}</DialogTitle>
              <DialogDescription>
                {isInit
                  ? "设置主密码（至少 8 位），用于 Argon2id 派生 AES-256-GCM 密钥。"
                  : "输入主密码解锁密库，15 分钟无操作自动锁定归零。"}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-3">
          <div className="space-y-1.5">
            <Label>主密码 (Master Passphrase)</Label>
            <Input
              required
              minLength={8}
              type="password"
              placeholder="••••••••••••"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
            />
          </div>
          {error && <div className="text-xs text-rose-400">{error}</div>}
          <DialogFooter className="pt-2">
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              取消
            </Button>
            <Button type="submit" className="bg-indigo-600 text-foreground hover:bg-indigo-500">
              {isInit ? "创建并解锁" : "解锁密库"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
