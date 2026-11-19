import { useEffect, useState } from "react";
import { Brain, Plus, Sparkles, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { DecisionBadge } from "./DecisionBadge";
import type {
  DualEvaluateResp,
  L2Protocol,
  L2ProviderConfig,
  SecurityRule,
  VaultCredentialMeta,
} from "@/types";

interface AstShieldViewProps {
  rules: SecurityRule[];
  l2Config: L2ProviderConfig;
  credentials: VaultCredentialMeta[];
  onOpenNewRule: () => void;
  onToggleRule: (rule: SecurityRule, enabled: boolean) => void;
  onDeleteRule: (id: string) => void;
  onSaveL2Config: (cfg: L2ProviderConfig) => Promise<void>;
  onEvaluateCommand: (command: string, intent?: string) => Promise<DualEvaluateResp>;
}

export function AstShieldView({
  rules,
  l2Config,
  credentials,
  onOpenNewRule,
  onToggleRule,
  onDeleteRule,
  onSaveL2Config,
  onEvaluateCommand,
}: AstShieldViewProps) {
  const [l2Form, setL2Form] = useState<L2ProviderConfig>(l2Config);
  const [sandboxCmd, setSandboxCmd] = useState("ls -la /var/log && echo $(rm -rf /tmp/x)");
  const [sandboxIntent, setSandboxIntent] = useState("Inspect nginx access logs");
  const [verdict, setVerdict] = useState<DualEvaluateResp | null>(null);

  useEffect(() => {
    setL2Form(l2Config);
  }, [l2Config]);

  const apiKeyCreds = credentials.filter((c) => c.kind === "api_key");

  const handleEvaluate = async () => {
    const res = await onEvaluateCommand(sandboxCmd, sandboxIntent);
    setVerdict(res);
  };

  return (
    <div className="flex-1 space-y-6 overflow-y-auto p-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold text-white">Dual-Layer Shield · L1 AST + L2 AI 语义审核</h2>
          <p className="text-xs text-slate-400">
            L1 基于 brush-parser 语法树硬拦截；L2 支持 Chat/Responses/Messages 协议与 SystemOne 快速决策模型。
          </p>
        </div>
        <Button size="sm" className="bg-indigo-600 text-white hover:bg-indigo-500" onClick={onOpenNewRule}>
          <Plus className="mr-1.5 size-4" /> 新增 L1 规则
        </Button>
      </div>

      <Tabs defaultValue="rules">
        <TabsList className="bg-[#141824]">
          <TabsTrigger value="rules">L1 AST 规则 ({rules.length})</TabsTrigger>
          <TabsTrigger value="l2">L2 语义策略引擎</TabsTrigger>
          <TabsTrigger value="sandbox">双层沙盒演练</TabsTrigger>
        </TabsList>

        <TabsContent value="rules" className="mt-4">
          <Card className="border-white/[0.07] bg-[#141824]">
            <Table>
              <TableHeader>
                <TableRow className="border-white/[0.07] hover:bg-transparent">
                  <TableHead>启用</TableHead>
                  <TableHead>规则名称</TableHead>
                  <TableHead>匹配方式</TableHead>
                  <TableHead>Pattern</TableHead>
                  <TableHead>裁决</TableHead>
                  <TableHead className="text-right">操作</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rules.map((r) => (
                  <TableRow key={r.id} className="border-white/[0.06]">
                    <TableCell>
                      <Switch
                        checked={r.enabled}
                        onCheckedChange={(checked) => onToggleRule(r, checked)}
                      />
                    </TableCell>
                    <TableCell className="font-medium text-white">{r.name}</TableCell>
                    <TableCell className="text-xs text-slate-400">{r.match_kind}</TableCell>
                    <TableCell className="font-mono text-xs text-indigo-300">{r.pattern}</TableCell>
                    <TableCell>
                      <DecisionBadge decision={r.decision} />
                    </TableCell>
                    <TableCell className="text-right">
                      <Button
                        size="icon"
                        variant="ghost"
                        className="size-7 text-slate-400 hover:text-rose-300"
                        onClick={() => onDeleteRule(r.id)}
                      >
                        <Trash2 className="size-3.5" />
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </Card>
        </TabsContent>

        <TabsContent value="l2" className="mt-4">
          <Card className="border-white/[0.07] bg-[#141824]">
            <CardHeader>
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2.5">
                  <Brain className="size-5 text-indigo-400" />
                  <div>
                    <CardTitle className="text-sm">L2 AI 语义审核统一策略接口</CardTitle>
                    <CardDescription>
                      支持 OpenAI Chat / Responses、Anthropic Messages 与 SystemOne 快速决策模型；API Key 仅引用加密密库。
                    </CardDescription>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-xs text-slate-400">{l2Form.enabled ? "已启用" : "已禁用"}</span>
                  <Switch
                    checked={l2Form.enabled}
                    onCheckedChange={(enabled) => setL2Form({ ...l2Form, enabled })}
                  />
                </div>
              </div>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                <div className="space-y-1.5">
                  <Label>策略协议 (Protocol)</Label>
                  <Select
                    value={l2Form.protocol}
                    onValueChange={(v) => setL2Form({ ...l2Form, protocol: v as L2Protocol })}
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="system_one_fast">SystemOne 快速决策模型 (低延迟)</SelectItem>
                      <SelectItem value="open_ai_chat">OpenAI Chat (/v1/chat/completions)</SelectItem>
                      <SelectItem value="open_ai_responses">OpenAI Responses (/v1/responses)</SelectItem>
                      <SelectItem value="anthropic_messages">Anthropic Messages (/v1/messages)</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-1.5">
                  <Label>模型名称 (Model)</Label>
                  <Input
                    value={l2Form.model}
                    onChange={(e) => setL2Form({ ...l2Form, model: e.target.value })}
                    placeholder="systemone-guard-v1 / gpt-4o-mini / claude-3-5-haiku"
                  />
                </div>
                <div className="space-y-1.5">
                  <Label>Endpoint Base URL</Label>
                  <Input
                    value={l2Form.base_url}
                    onChange={(e) => setL2Form({ ...l2Form, base_url: e.target.value })}
                    placeholder="https://api.openai.com/v1"
                  />
                </div>
                <div className="space-y-1.5">
                  <Label>API Key 凭据引用 (来自 Keychain 密库)</Label>
                  <Select
                    value={l2Form.api_key_credential_id || "__none__"}
                    onValueChange={(v) =>
                      setL2Form({
                        ...l2Form,
                        api_key_credential_id: v === "__none__" ? null : (v ?? null),
                      })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__none__">无 (本地无需认证端点)</SelectItem>
                      {apiKeyCreds.map((c) => (
                        <SelectItem key={c.id} value={c.id}>
                          {c.label} ({c.preview})
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-1.5">
                  <Label>超时预算 (ms, 超时 Fail-Closed 转人工审批)</Label>
                  <Input
                    type="number"
                    value={l2Form.timeout_ms}
                    onChange={(e) => setL2Form({ ...l2Form, timeout_ms: Number(e.target.value) })}
                  />
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <div className="space-y-1.5">
                    <Label>转审批风险阈值</Label>
                    <Input
                      type="number"
                      min={0}
                      max={100}
                      value={l2Form.risk_threshold_ask}
                      onChange={(e) =>
                        setL2Form({ ...l2Form, risk_threshold_ask: Number(e.target.value) })
                      }
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label>直接阻断阈值</Label>
                    <Input
                      type="number"
                      min={0}
                      max={100}
                      value={l2Form.risk_threshold_deny}
                      onChange={(e) =>
                        setL2Form({ ...l2Form, risk_threshold_deny: Number(e.target.value) })
                      }
                    />
                  </div>
                </div>
              </div>
              <div className="flex justify-end">
                <Button className="bg-indigo-600 text-white hover:bg-indigo-500" onClick={() => onSaveL2Config(l2Form)}>
                  保存 L2 策略配置
                </Button>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="sandbox" className="mt-4">
          <Card className="border-white/[0.07] bg-[#141824]">
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-sm">
                <Sparkles className="size-4 text-indigo-400" /> L1 AST + L2 AI 双层沙盒演练
              </CardTitle>
              <CardDescription>模拟 AI 声明意图与待执行 Shell 命令，实时观察双层裁决链路。</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                <Input
                  value={sandboxIntent}
                  onChange={(e) => setSandboxIntent(e.target.value)}
                  placeholder="AI 声明意图 (Intent)"
                  className="border-white/10 bg-[#0D1017] text-sm"
                />
                <div className="flex gap-2">
                  <Input
                    value={sandboxCmd}
                    onChange={(e) => setSandboxCmd(e.target.value)}
                    className="border-white/10 bg-[#0D1017] font-mono text-sm"
                  />
                  <Button onClick={handleEvaluate}>双层评估</Button>
                </div>
              </div>

              {verdict && (
                <div className="space-y-3 rounded-xl border border-white/[0.07] bg-[#0D1017] p-4">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="flex items-center gap-2 text-sm">
                      <span className="text-slate-400">最终联合裁决:</span>
                      <DecisionBadge decision={verdict.final_decision} />
                    </div>
                    <span className="text-xs text-slate-400">{verdict.l1.reason}</span>
                  </div>
                  {verdict.l1.parsed_commands.map((c, idx) => (
                    <div
                      key={idx}
                      className="flex items-center justify-between rounded-lg border border-white/[0.06] bg-[#141824] px-3 py-2 font-mono text-xs"
                    >
                      <div>
                        <span className="text-indigo-300">{c.program}</span>{" "}
                        <span className="text-slate-400">{c.args.join(" ")}</span>
                        <span className="ml-2 text-slate-500">[{c.context}]</span>
                      </div>
                      <DecisionBadge decision={c.decision} />
                    </div>
                  ))}
                  {verdict.l2 && (
                    <div className="rounded-lg border border-indigo-500/30 bg-indigo-500/10 p-3 text-xs">
                      <div className="flex items-center justify-between">
                        <span className="font-semibold text-indigo-300">
                          L2 ({verdict.l2.provider_name} · {verdict.l2.model})
                        </span>
                        <div className="flex items-center gap-2">
                          <Badge variant="outline">Risk: {verdict.l2.risk_score}/100</Badge>
                          <Badge variant="outline">{verdict.l2.latency_ms}ms</Badge>
                        </div>
                      </div>
                      <p className="mt-1.5 text-slate-200">{verdict.l2.reasoning}</p>
                    </div>
                  )}
                </div>
              )}
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
