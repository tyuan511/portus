import { useState } from "react";
import { KeyRound, Lock, Plus, ShieldCheck, Trash2, Unlock } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import type { VaultCredentialKind, VaultCredentialMeta, VaultStatus } from "@/types";

interface KeychainVaultViewProps {
  vaultStatus: VaultStatus;
  credentials: VaultCredentialMeta[];
  onOpenUnlock: () => void;
  onLockVault: () => void;
  onStoreCredential: (input: {
    label: string;
    kind: VaultCredentialKind;
    username?: string;
    secret: string;
  }) => Promise<void>;
  onDeleteCredential: (id: string) => Promise<void>;
}

export function KeychainVaultView({
  vaultStatus,
  credentials,
  onOpenUnlock,
  onLockVault,
  onStoreCredential,
  onDeleteCredential,
}: KeychainVaultViewProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [label, setLabel] = useState("");
  const [kind, setKind] = useState<VaultCredentialKind>("password");
  const [username, setUsername] = useState("");
  const [secret, setSecret] = useState("");

  const isUnlocked = vaultStatus.state === "unlocked";

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    await onStoreCredential({
      label,
      kind,
      username: username.trim() || undefined,
      secret,
    });
    setDialogOpen(false);
    setLabel("");
    setUsername("");
    setSecret("");
  };

  return (
    <div className="flex-1 space-y-5 overflow-y-auto p-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-white">Termius Keychain · AES-256-GCM 密库</h2>
          <p className="text-xs text-slate-400">
            所有 SSH 密码、私钥 PEM 与 L2 大模型 API Key 均通过 Argon2id + AES-256-GCM 独立加密，SQLite 零明文落盘。
          </p>
        </div>
        <div className="flex items-center gap-2">
          {isUnlocked ? (
            <>
              <Button
                size="sm"
                variant="outline"
                className="border-white/10 bg-[#141824]"
                onClick={onLockVault}
              >
                <Lock className="mr-1.5 size-4 text-amber-400" /> 立即锁定密库
              </Button>
              <Button
                size="sm"
                className="bg-emerald-600 text-white hover:bg-emerald-500"
                onClick={() => setDialogOpen(true)}
              >
                <Plus className="mr-1.5 size-4" /> 新增加密凭据
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              className="bg-indigo-600 text-white hover:bg-indigo-500"
              onClick={onOpenUnlock}
            >
              <Unlock className="mr-1.5 size-4" />
              {vaultStatus.state === "uninitialized" ? "初始化主密码" : "解锁密库"}
            </Button>
          )}
        </div>
      </div>

      <Card className="border-white/[0.07] bg-[#141824]">
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <ShieldCheck className={`size-5 ${isUnlocked ? "text-emerald-400" : "text-amber-400"}`} />
              <div>
                <CardTitle className="text-sm">
                  密库状态：
                  {vaultStatus.state === "unlocked"
                    ? "已解锁 (自动锁定: 15 分钟)"
                    : vaultStatus.state === "locked"
                      ? "已锁定 (内存密钥已归零)"
                      : "未初始化"}
                </CardTitle>
                <CardDescription>
                  当前共保存 {vaultStatus.credential_count} 条独立 AEAD 加密凭据
                </CardDescription>
              </div>
            </div>
            <Badge
              variant="outline"
              className={
                isUnlocked
                  ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-300"
                  : "border-amber-500/30 bg-amber-500/10 text-amber-300"
              }
            >
              {vaultStatus.state.toUpperCase()}
            </Badge>
          </div>
        </CardHeader>
      </Card>

      <Card className="border-white/[0.07] bg-[#141824]">
        <Table>
          <TableHeader>
            <TableRow className="border-white/[0.07] hover:bg-transparent">
              <TableHead>凭据名称</TableHead>
              <TableHead>类型</TableHead>
              <TableHead>Credential ID</TableHead>
              <TableHead>脱敏摘要</TableHead>
              <TableHead className="text-right">操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {credentials.length === 0 ? (
              <TableRow>
                <TableCell colSpan={5} className="py-10 text-center text-sm text-slate-500">
                  暂无凭据。解锁密库后可添加 SSH 密码、私钥或 L2 LLM API Key。
                </TableCell>
              </TableRow>
            ) : (
              credentials.map((c) => (
                <TableRow key={c.id} className="border-white/[0.06]">
                  <TableCell className="font-medium text-white">
                    <div className="flex items-center gap-2">
                      <KeyRound className="size-3.5 text-indigo-400" />
                      {c.label}
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline" className="border-white/10 text-xs text-slate-300">
                      {c.kind}
                    </Badge>
                  </TableCell>
                  <TableCell className="font-mono text-xs text-slate-400">{c.id}</TableCell>
                  <TableCell className="font-mono text-xs text-emerald-300">{c.preview}</TableCell>
                  <TableCell className="text-right">
                    <Button
                      size="icon"
                      variant="ghost"
                      disabled={!isUnlocked}
                      className="size-7 text-slate-400 hover:text-rose-300"
                      onClick={() => onDeleteCredential(c.id)}
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </Card>

      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="border-white/10 bg-[#141824] text-slate-100 sm:max-w-md">
          <DialogHeader>
            <DialogTitle>新增 AES-256-GCM 加密凭据</DialogTitle>
            <DialogDescription>明文仅在加密瞬间驻留内存并立即归零，前端永不回显。</DialogDescription>
          </DialogHeader>
          <form onSubmit={handleSubmit} className="space-y-3">
            <div className="space-y-1.5">
              <Label>凭据标签</Label>
              <Input
                required
                placeholder="prod-root-password / openai-l2-key"
                value={label}
                onChange={(e) => setLabel(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label>凭据类型</Label>
              <Select
                value={kind}
                onValueChange={(v) => setKind((v as VaultCredentialKind) ?? "password")}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="password">SSH Password</SelectItem>
                  <SelectItem value="private_key">SSH Private Key (PEM)</SelectItem>
                  <SelectItem value="api_key">L2 LLM / SystemOne API Key</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>关联用户名 (可选)</Label>
              <Input
                placeholder="ubuntu / root"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label>{kind === "private_key" ? "私钥 PEM 内容" : "密钥 / 密码"}</Label>
              {kind === "private_key" ? (
                <Textarea
                  required
                  rows={5}
                  placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
                  value={secret}
                  onChange={(e) => setSecret(e.target.value)}
                  className="font-mono text-xs"
                />
              ) : (
                <Input
                  required
                  type="password"
                  value={secret}
                  onChange={(e) => setSecret(e.target.value)}
                />
              )}
            </div>
            <DialogFooter className="pt-2">
              <Button type="button" variant="outline" onClick={() => setDialogOpen(false)}>
                取消
              </Button>
              <Button type="submit" className="bg-emerald-600 text-white hover:bg-emerald-500">
                加密存入密库
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
