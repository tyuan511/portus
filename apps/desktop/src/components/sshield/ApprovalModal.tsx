import { ShieldAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { DecisionBadge } from "./DecisionBadge";
import type { ApprovalRequest } from "@/types";

interface ApprovalModalProps {
  approval: ApprovalRequest | null;
  onDecide: (id: string, approved: boolean) => void;
}

export function ApprovalModal({ approval, onDecide }: ApprovalModalProps) {
  return (
    <Dialog open={Boolean(approval)} onOpenChange={() => {}}>
      <DialogContent
        showCloseButton={false}
        className="border-amber-500/40 bg-card text-foreground sm:max-w-xl"
      >
        {approval && (
          <>
            <DialogHeader>
              <div className="flex items-center gap-3">
                <div className="flex size-10 items-center justify-center rounded-xl bg-amber-500/20 text-amber-400">
                  <ShieldAlert className="size-5" />
                </div>
                <div>
                  <DialogTitle>AI Agent 命令执行审批</DialogTitle>
                  <DialogDescription>
                    目标主机：{approval.server_name} ({approval.username}@{approval.host}) · Trace{" "}
                    {approval.trace_id.slice(0, 8)}
                  </DialogDescription>
                </div>
              </div>
            </DialogHeader>

            <div className="space-y-3 text-sm">
              <div>
                <div className="text-xs text-muted-foreground">AI 声明意图 (Intent)</div>
                <div className="mt-1 rounded-lg border border-border bg-muted/60 p-2.5 text-foreground">
                  {approval.intent}
                </div>
              </div>
              <div>
                <div className="text-xs text-muted-foreground">待执行命令</div>
                <pre className="mt-1 overflow-x-auto rounded-lg border border-border bg-muted/60 p-3 font-mono text-xs text-amber-300">
                  {approval.command}
                </pre>
              </div>
              {approval.l2_verdict && (
                <div className="rounded-lg border border-indigo-500/30 bg-indigo-500/10 p-3 text-xs">
                  <div className="flex items-center justify-between font-medium text-indigo-300">
                    <span>
                      L2 AI 语义审核 ({approval.l2_verdict.provider_name} ·{" "}
                      {approval.l2_verdict.model})
                    </span>
                    <Badge variant="outline">Risk: {approval.l2_verdict.risk_score}/100</Badge>
                  </div>
                  <p className="mt-1 text-foreground">{approval.l2_verdict.reasoning}</p>
                </div>
              )}
              {approval.verdict.parsed_commands.length > 0 && (
                <div>
                  <div className="text-xs text-muted-foreground">L1 AST 语法树拆解</div>
                  <div className="mt-1 space-y-1">
                    {approval.verdict.parsed_commands.map((c, idx) => (
                      <div
                        key={idx}
                        className="flex items-center justify-between rounded bg-muted/60 px-2.5 py-1.5 font-mono text-xs"
                      >
                        <span>
                          {c.program} {c.args.join(" ")}
                        </span>
                        <DecisionBadge decision={c.decision} />
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>

            <DialogFooter className="pt-2">
              <Button
                variant="outline"
                className="border-rose-500/40 bg-rose-500/10 text-rose-200 hover:bg-rose-500/20"
                onClick={() => onDecide(approval.id, false)}
              >
                拒绝执行
              </Button>
              <Button
                className="bg-emerald-600 text-foreground hover:bg-emerald-500"
                onClick={() => onDecide(approval.id, true)}
              >
                批准放行
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
