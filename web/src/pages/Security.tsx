import { useCallback, useEffect, useState } from "react";
import { Pencil, Play, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import { StatusPill, decisionTone } from "@/components/sshield/StatusDot";
import SnippetsPanel from "@/pages/Snippets";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import type {
  CommandVerdict,
  L2ProviderConfig,
  PolicyDecision,
  SecurityRule,
  SemanticReviewVerdict,
} from "@/types";

const RULE_KINDS = ["program_exact", "command_prefix", "regex"] as const;
const DECISIONS: PolicyDecision[] = ["allow", "ask_approval", "deny"];

export default function SecurityView() {
  const auth = useAuth();
  const { t } = useI18n();
  const canManage = auth.hasPermission("rules.manage");
  const [rules, setRules] = useState<SecurityRule[]>([]);
  const [l2, setL2] = useState<L2ProviderConfig | null>(null);
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<Partial<SecurityRule> | null>(null);

  const [sandboxCmd, setSandboxCmd] = useState("ls -la /var/log && df -h");
  const [sandboxResult, setSandboxResult] = useState<{
    l1: CommandVerdict;
    l2: SemanticReviewVerdict | null;
    final_decision: PolicyDecision;
  } | null>(null);

  const load = useCallback(async () => {
    try {
      setRules(await api.get<SecurityRule[]>("/api/rules"));
      setL2(await api.get<L2ProviderConfig>("/api/l2-config"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const saveRule = async () => {
    if (!form?.name || !form?.pattern) return;
    try {
      await api.post("/api/rules", {
        id: form.id ?? "",
        name: form.name,
        match_kind: form.match_kind ?? "command_prefix",
        pattern: form.pattern,
        decision: form.decision ?? "ask_approval",
        priority: form.priority ?? 50,
        enabled: form.enabled ?? true,
        description: form.description ?? "",
      });
      setForm(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "save failed");
    }
  };

  const removeRule = async (id: string) => {
    if (!confirm(t("security.deleteConfirm"))) return;
    try {
      await api.del(`/api/rules/${id}`);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "delete failed");
    }
  };

  const saveL2 = async () => {
    if (!l2) return;
    try {
      await api.put("/api/l2-config", l2);
      setError(undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "save failed");
    }
  };

  const evaluate = async () => {
    try {
      setSandboxResult(
        await api.post("/api/security/evaluate", {
          command: sandboxCmd,
          intent: "sandbox evaluation",
        })
      );
    } catch (err) {
      setError(err instanceof Error ? err.message : "evaluate failed");
    }
  };

  return (
    <Page header={<PageHeader title={t("security.title")} />}>
      <ErrorText>{error}</ErrorText>

      <Tabs defaultValue="rules" className="gap-5">
        <TabsList variant="line" className="w-full justify-start border-b border-border pb-0">
          <TabsTrigger value="rules">{t("security.rules")}</TabsTrigger>
          <TabsTrigger value="l2">{t("security.l2")}</TabsTrigger>
          <TabsTrigger value="sandbox">{t("security.sandbox")}</TabsTrigger>
          {auth.hasPermission("snippets.use") && (
            <TabsTrigger value="snippets">{t("security.snippets")}</TabsTrigger>
          )}
        </TabsList>

        <TabsContent value="rules" className="flex flex-col gap-3">
          {canManage && (
            <div className="flex justify-end">
              <Button
                size="sm"
                onClick={() =>
                  setForm({
                    name: "",
                    match_kind: "command_prefix",
                    pattern: "",
                    decision: "ask_approval",
                    priority: 50,
                    enabled: true,
                  })
                }
              >
                <Plus data-icon="inline-start" />
                {t("security.newRule")}
              </Button>
            </div>
          )}

          <Card className="p-0">
            <Table>
              <TableHeader className="bg-secondary/50">
                <TableRow>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("security.colName")}
                  </TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("security.colMatch")}
                  </TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("security.colPattern")}
                  </TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("security.colDecision")}
                  </TableHead>
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("security.colPrio")}
                  </TableHead>
                  <TableHead className="px-4" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {rules.map((r) => (
                  <TableRow key={r.id}>
                    <TableCell className="px-4 font-medium">
                      <span>{r.name}</span>
                      {!r.enabled && (
                        <Badge variant="secondary" className="ml-2">
                          {t("common.off")}
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="px-4 font-mono text-xs text-muted-foreground">
                      {r.match_kind}
                    </TableCell>
                    <TableCell
                      className="max-w-72 truncate px-4 font-mono text-xs"
                      title={r.pattern}
                    >
                      {r.pattern}
                    </TableCell>
                    <TableCell className="px-4">
                      <StatusPill label={r.decision} tone={decisionTone(r.decision)} />
                    </TableCell>
                    <TableCell className="px-4 font-mono text-xs tabular-nums">
                      {r.priority}
                    </TableCell>
                    <TableCell className="px-4 text-right">
                      {canManage && !r.id.startsWith("builtin-") && (
                        <div className="flex justify-end gap-1">
                          <Button variant="ghost" size="icon-xs" onClick={() => setForm(r)}>
                            <Pencil />
                          </Button>
                          <Button
                            variant="destructive"
                            size="icon-xs"
                            onClick={() => void removeRule(r.id)}
                          >
                            <Trash2 />
                          </Button>
                        </div>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </Card>
        </TabsContent>

        <TabsContent value="l2">
          {l2 && (
            <Card className="max-w-2xl">
              <CardHeader className="flex flex-row items-center justify-between border-b border-border/70 pb-4">
                <div>
                  <CardTitle>{t("security.l2Review")}</CardTitle>
                  <CardDescription className="mt-0.5 font-mono text-xs">
                    {l2.protocol} · {l2.model || "default"}
                  </CardDescription>
                </div>
                <Switch
                  checked={l2.enabled}
                  onCheckedChange={(v) => setL2({ ...l2, enabled: v })}
                />
              </CardHeader>

              <CardContent className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <div className="flex flex-col gap-1.5">
                  <Label>{t("security.l2Protocol")}</Label>
                  <Select
                    value={l2.protocol}
                    onValueChange={(v) =>
                      v && setL2({ ...l2, protocol: v as L2ProviderConfig["protocol"] })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        <SelectItem value="system_one_fast">SystemOne Fast</SelectItem>
                        <SelectItem value="openai_chat">OpenAI Chat</SelectItem>
                        <SelectItem value="openai_responses">OpenAI Responses</SelectItem>
                        <SelectItem value="anthropic_messages">Anthropic Messages</SelectItem>
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label>{t("security.l2BaseUrl")}</Label>
                  <Input
                    value={l2.base_url}
                    onChange={(e) => setL2({ ...l2, base_url: e.target.value })}
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label>{t("security.l2Model")}</Label>
                  <Input
                    value={l2.model}
                    onChange={(e) => setL2({ ...l2, model: e.target.value })}
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label>{t("security.l2ApiKey")}</Label>
                  <Input
                    value={l2.api_key_credential_id ?? ""}
                    onChange={(e) => setL2({ ...l2, api_key_credential_id: e.target.value })}
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label>{t("security.l2RiskAsk")}</Label>
                  <Input
                    type="number"
                    value={l2.risk_threshold_ask}
                    onChange={(e) => setL2({ ...l2, risk_threshold_ask: Number(e.target.value) })}
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <Label>{t("security.l2RiskDeny")}</Label>
                  <Input
                    type="number"
                    value={l2.risk_threshold_deny}
                    onChange={(e) =>
                      setL2({ ...l2, risk_threshold_deny: Number(e.target.value) })
                    }
                  />
                </div>
              </CardContent>

              {canManage && (
                <CardFooter className="justify-end">
                  <Button size="sm" onClick={() => void saveL2()}>
                    {t("security.saveL2")}
                  </Button>
                </CardFooter>
              )}
            </Card>
          )}
        </TabsContent>

        <TabsContent value="sandbox" className="flex max-w-3xl flex-col gap-4">
          <Card>
            <CardHeader>
              <CardTitle>{t("security.sandbox")}</CardTitle>
              <CardDescription>{t("security.sandboxHint")}</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-col gap-4">
              <div className="flex gap-2">
                <Input
                  className="font-mono text-xs"
                  value={sandboxCmd}
                  onChange={(e) => setSandboxCmd(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && void evaluate()}
                />
                <Button size="sm" onClick={() => void evaluate()}>
                  <Play data-icon="inline-start" />
                  {t("security.evaluate")}
                </Button>
              </div>

              {sandboxResult && (
                <div className="flex flex-col gap-3">
                  <div className="grid gap-px overflow-hidden rounded-lg border border-border bg-border sm:grid-cols-3">
                    <div className="bg-card p-3">
                      <div className="font-mono text-[10px] uppercase text-muted-foreground">
                        01 · L1 AST
                      </div>
                      <div className="mt-1.5">
                        <StatusPill
                          label={sandboxResult.l1.decision}
                          tone={decisionTone(sandboxResult.l1.decision)}
                        />
                      </div>
                      <p className="mt-1.5 text-xs text-muted-foreground">
                        {sandboxResult.l1.reason}
                      </p>
                    </div>
                    <div className="bg-card p-3">
                      <div className="font-mono text-[10px] uppercase text-muted-foreground">
                        02 · L2 AI
                      </div>
                      <div className="mt-1.5">
                        {sandboxResult.l2 ? (
                          <StatusPill
                            label={`${sandboxResult.l2.decision} (${sandboxResult.l2.risk_score}/100)`}
                            tone={decisionTone(sandboxResult.l2.decision)}
                          />
                        ) : (
                          <StatusPill label="skipped" tone="neutral" />
                        )}
                      </div>
                      <p className="mt-1.5 text-xs text-muted-foreground">
                        {sandboxResult.l2?.reasoning ?? "—"}
                      </p>
                    </div>
                    <div className="bg-card p-3">
                      <div className="font-mono text-[10px] uppercase text-muted-foreground">
                        03 · Final
                      </div>
                      <div className="mt-1.5">
                        <StatusPill
                          label={sandboxResult.final_decision}
                          tone={decisionTone(sandboxResult.final_decision)}
                        />
                      </div>
                    </div>
                  </div>

                  <details className="rounded-lg border border-border bg-secondary/30 p-3">
                    <summary className="cursor-pointer font-mono text-xs text-muted-foreground">
                      {t("security.rawJson")}
                    </summary>
                    <pre className="mt-2 max-h-80 overflow-auto rounded-lg bg-[#09090b] p-3 font-mono text-xs text-[#ededed]">
                      {JSON.stringify(sandboxResult, null, 2)}
                    </pre>
                  </details>
                </div>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        {auth.hasPermission("snippets.use") && (
          <TabsContent value="snippets">
            <SnippetsPanel />
          </TabsContent>
        )}
      </Tabs>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{form?.id ? t("security.editRule") : t("security.newRule")}</DialogTitle>
          </DialogHeader>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>{t("common.name")}</Label>
              <Input
                value={form?.name ?? ""}
                onChange={(e) => setForm({ ...form!, name: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("security.matchKind")}</Label>
              <Select
                value={form?.match_kind ?? "command_prefix"}
                onValueChange={(v) =>
                  v && setForm({ ...form!, match_kind: v as SecurityRule["match_kind"] })
                }
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {RULE_KINDS.map((k) => (
                      <SelectItem key={k} value={k}>
                        {k}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            <div className="col-span-2 flex flex-col gap-1.5">
              <Label>{t("security.pattern")}</Label>
              <Input
                className="font-mono text-xs"
                value={form?.pattern ?? ""}
                onChange={(e) => setForm({ ...form!, pattern: e.target.value })}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("security.decision")}</Label>
              <Select
                value={form?.decision ?? "ask_approval"}
                onValueChange={(v) => v && setForm({ ...form!, decision: v as PolicyDecision })}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {DECISIONS.map((d) => (
                      <SelectItem key={d} value={d}>
                        {d}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label>{t("common.priority")}</Label>
              <Input
                type="number"
                value={form?.priority ?? 50}
                onChange={(e) => setForm({ ...form!, priority: Number(e.target.value) })}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => void saveRule()}>{t("common.save")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
