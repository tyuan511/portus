import { useMemo, useState } from "react";
import { CheckCircle2, Download, ShieldAlert, ShieldCheck } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { DecisionBadge } from "./DecisionBadge";
import type { AuditChainStatus, AuditEvent } from "@/types";

interface AuditLogsViewProps {
  audits: AuditEvent[];
  chainStatus: AuditChainStatus | null;
  onVerifyChain: () => Promise<void>;
}

export function AuditLogsView({ audits, chainStatus, onVerifyChain }: AuditLogsViewProps) {
  const [auditSearch, setAuditSearch] = useState("");

  const filteredAudits = useMemo(() => {
    const q = auditSearch.trim().toLowerCase();
    if (!q) return audits;
    return audits.filter(
      (a) =>
        a.command.toLowerCase().includes(q) ||
        a.intent.toLowerCase().includes(q) ||
        a.server_name.toLowerCase().includes(q) ||
        a.trace_id.toLowerCase().includes(q)
    );
  }, [audits, auditSearch]);

  const handleExportAudits = () => {
    const blob = new Blob([JSON.stringify(audits, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `sshield-audit-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex-1 space-y-5 overflow-y-auto p-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-xl font-semibold text-foreground">全链路可回溯审计 · Tamper-Evident Trace</h2>
          <p className="text-xs text-muted-foreground">
            每条记录通过 SHA-256 哈希链串联，完整回溯 Trace ID、L1 AST、L2 AI 评分、人工审批与脱敏输出。
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Input
            value={auditSearch}
            onChange={(e) => setAuditSearch(e.target.value)}
            placeholder="过滤 Trace ID / 命令 / 意图 / 主机..."
            className="w-64 border-border bg-card text-sm"
          />
          <Button
            size="sm"
            variant="outline"
            className="border-border bg-card"
            onClick={onVerifyChain}
          >
            <ShieldCheck className="mr-1.5 size-4 text-emerald-400" /> 校验哈希链
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="border-border bg-card"
            onClick={handleExportAudits}
          >
            <Download className="mr-1.5 size-4" /> 导出 JSON
          </Button>
        </div>
      </div>

      {chainStatus && (
        <div
          className={`flex items-center justify-between rounded-xl border px-4 py-2.5 text-xs ${
            chainStatus.valid
              ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-200"
              : "border-rose-500/30 bg-rose-500/10 text-rose-200"
          }`}
        >
          <div className="flex items-center gap-2">
            {chainStatus.valid ? <CheckCircle2 className="size-4" /> : <ShieldAlert className="size-4" />}
            <span>
              {chainStatus.valid
                ? `SHA-256 审计哈希链完整无篡改（共验证 ${chainStatus.total_events} 条事件）`
                : `检测到审计链断裂！异常事件 ID: ${chainStatus.broken_at_id}`}
            </span>
          </div>
        </div>
      )}

      <div className="space-y-3">
        {filteredAudits.length === 0 ? (
          <Card className="border-border bg-card">
            <CardContent className="py-12 text-center text-sm text-muted-foreground">
              暂无审计记录。
            </CardContent>
          </Card>
        ) : (
          filteredAudits.map((a) => (
            <Card key={a.id} className="border-border bg-card">
              <CardContent className="space-y-2.5 p-4 text-sm">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-semibold text-foreground">{a.server_name}</span>
                    <DecisionBadge decision={a.decision} />
                    <Badge variant="outline" className="border-border font-mono text-[10px] text-muted-foreground">
                      {a.actor}
                    </Badge>
                    {a.dry_run && <Badge variant="outline">DRY RUN</Badge>}
                    {a.approval_status && (
                      <Badge variant="outline" className="border-indigo-500/30 text-indigo-300">
                        审批: {a.approval_status}
                      </Badge>
                    )}
                    {a.l2_verdict && (
                      <Badge variant="outline" className="border-cyan-500/30 text-cyan-300">
                        L2 Risk: {a.l2_verdict.risk_score}/100 ({a.l2_verdict.latency_ms}ms)
                      </Badge>
                    )}
                  </div>
                  <span className="text-xs text-muted-foreground">
                    {new Date(a.timestamp).toLocaleString()}
                    {a.duration_ms != null ? ` · ${a.duration_ms}ms` : ""}
                    {a.exit_code != null ? ` · exit ${a.exit_code}` : ""}
                  </span>
                </div>

                <div className="font-mono text-xs text-emerald-300">$ {a.command}</div>
                <div className="text-xs text-muted-foreground">Intent: {a.intent}</div>

                {a.l2_verdict && (
                  <div className="rounded bg-indigo-500/10 px-2.5 py-1.5 text-xs text-indigo-200">
                    L2 语义分析 ({a.l2_verdict.model}): {a.l2_verdict.reasoning}
                  </div>
                )}

                {a.error && (
                  <div className="rounded bg-rose-500/10 p-2 font-mono text-xs text-rose-300">
                    {a.error}
                  </div>
                )}
                {a.stdout_preview && (
                  <pre className="max-h-36 overflow-auto rounded bg-muted/60 p-2.5 font-mono text-xs text-foreground/90">
                    {a.stdout_preview}
                  </pre>
                )}

                <div className="flex flex-wrap items-center justify-between gap-2 pt-1 font-mono text-[10px] text-muted-foreground">
                  <span>trace: {a.trace_id}</span>
                  <span>hash: {a.event_hash.slice(0, 16)}…</span>
                </div>
              </CardContent>
            </Card>
          ))
        )}
      </div>
    </div>
  );
}
