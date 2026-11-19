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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ServerAsset, VaultCredentialMeta } from "@/types";

export type HostFormState = Partial<ServerAsset>;

interface HostFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  form: HostFormState;
  setForm: (form: HostFormState) => void;
  servers: ServerAsset[];
  credentials: VaultCredentialMeta[];
  onSubmit: (e: React.FormEvent) => void;
}

export function HostFormDialog({
  open,
  onOpenChange,
  form,
  setForm,
  servers,
  credentials,
  onSubmit,
}: HostFormDialogProps) {
  const sshCreds = credentials.filter((c) => c.kind !== "api_key");

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="border-white/10 bg-[#141824] text-slate-100 sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{form.id ? "Edit Host" : "New Host"}</DialogTitle>
          <DialogDescription>
            主机凭据通过 Keychain 密库引用，SQLite 仅保存主机地址与拓扑元数据。
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={onSubmit} className="space-y-3">
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label>Label / 名称</Label>
              <Input
                required
                placeholder="prod-api"
                value={form.name ?? ""}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </div>
            <div className="space-y-1.5">
              <Label>Group / 分组</Label>
              <Input
                placeholder="Production"
                value={form.group_name ?? ""}
                onChange={(e) => setForm({ ...form, group_name: e.target.value })}
              />
            </div>
            <div className="space-y-1.5">
              <Label>Address (Host / IP)</Label>
              <Input
                required
                placeholder="10.0.1.25"
                value={form.host ?? ""}
                onChange={(e) => setForm({ ...form, host: e.target.value })}
              />
            </div>
            <div className="grid grid-cols-2 gap-2">
              <div className="space-y-1.5">
                <Label>Username</Label>
                <Input
                  required
                  value={form.username ?? "root"}
                  onChange={(e) => setForm({ ...form, username: e.target.value })}
                />
              </div>
              <div className="space-y-1.5">
                <Label>Port</Label>
                <Input
                  type="number"
                  value={form.port ?? 22}
                  onChange={(e) => setForm({ ...form, port: Number(e.target.value) })}
                />
              </div>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label>Keychain Credential (AES-256-GCM 密库引用)</Label>
            <Select
              value={form.credential_id || "__none__"}
              onValueChange={(val) =>
                setForm({
                  ...form,
                  credential_id: val === "__none__" ? null : (val ?? null),
                })
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">使用本机 SSH Agent / 默认私钥</SelectItem>
                {sshCreds.map((c) => (
                  <SelectItem key={c.id} value={c.id}>
                    {c.label} ({c.kind})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1.5">
            <Label>Identity File 路径 (可选)</Label>
            <Input
              placeholder="~/.ssh/id_ed25519"
              value={form.identity_file ?? ""}
              onChange={(e) => setForm({ ...form, identity_file: e.target.value })}
            />
          </div>

          <div className="space-y-1.5">
            <Label>ProxyJump 跳板机 (可选)</Label>
            <Select
              value={form.jump_host_id || "__none__"}
              onValueChange={(val) =>
                setForm({ ...form, jump_host_id: val === "__none__" ? null : (val ?? null) })
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">Direct (无跳板机)</SelectItem>
                {servers
                  .filter((s) => s.id !== form.id)
                  .map((s) => (
                    <SelectItem key={s.id} value={s.id}>
                      {s.name} ({s.host})
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
          </div>

          <DialogFooter className="pt-2">
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              取消
            </Button>
            <Button type="submit" className="bg-emerald-600 text-white hover:bg-emerald-500">
              Save Host
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
