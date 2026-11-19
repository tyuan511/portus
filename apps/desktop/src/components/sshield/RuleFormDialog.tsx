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
import type { PolicyDecision, RuleMatchKind, SecurityRule } from "@/types";

interface RuleFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  form: Partial<SecurityRule>;
  setForm: (form: Partial<SecurityRule>) => void;
  onSubmit: (e: React.FormEvent) => void;
}

export function RuleFormDialog({
  open,
  onOpenChange,
  form,
  setForm,
  onSubmit,
}: RuleFormDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="border-border bg-card text-foreground sm:max-w-md">
        <DialogHeader>
          <DialogTitle>新增 L1 AST 安全规则</DialogTitle>
          <DialogDescription>配置程序名、命令前缀或正则规则及对应放行策略。</DialogDescription>
        </DialogHeader>
        <form onSubmit={onSubmit} className="space-y-3">
          <div className="space-y-1.5">
            <Label>规则名称</Label>
            <Input
              required
              placeholder="Allow kubectl get"
              value={form.name ?? ""}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
            />
          </div>
          <div className="space-y-1.5">
            <Label>匹配类型</Label>
            <Select
              value={form.match_kind ?? "command_prefix"}
              onValueChange={(val) =>
                setForm({ ...form, match_kind: (val as RuleMatchKind) ?? "command_prefix" })
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="program_exact">Program Exact (精确程序名)</SelectItem>
                <SelectItem value="command_prefix">Command Prefix (命令前缀)</SelectItem>
                <SelectItem value="regex">Regex (正则表达式)</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1.5">
            <Label>Pattern</Label>
            <Input
              required
              placeholder="kubectl get"
              className="font-mono"
              value={form.pattern ?? ""}
              onChange={(e) => setForm({ ...form, pattern: e.target.value })}
            />
          </div>
          <div className="space-y-1.5">
            <Label>裁决策略</Label>
            <Select
              value={form.decision ?? "ask_approval"}
              onValueChange={(val) =>
                setForm({ ...form, decision: (val as PolicyDecision) ?? "ask_approval" })
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="allow">Allow (自动放行)</SelectItem>
                <SelectItem value="ask_approval">Ask Approval (人工审批)</SelectItem>
                <SelectItem value="deny">Deny (黑名单阻断)</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <DialogFooter className="pt-2">
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              取消
            </Button>
            <Button type="submit" className="bg-indigo-600 text-foreground hover:bg-indigo-500">
              保存规则
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
