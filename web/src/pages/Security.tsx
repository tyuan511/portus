import { useCallback, useEffect, useState } from "react";
import { LoaderCircle, Pencil, Play, Plus, Search, Trash2, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/sshield/ConfirmDialog";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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
  CredentialMeta,
  DualEvaluateResp,
  L2ProviderConfig,
  PolicyDecision,
  SecurityRule,
} from "@/types";

const RULE_KINDS = ["program_exact", "command_prefix", "regex"] as const;
const DECISIONS: PolicyDecision[] = ["allow", "ask_approval", "deny"];
const CREDENTIAL_NONE = "__none__";

export default function SecurityView() {
  const auth = useAuth();
  const { t } = useI18n();
  const canManage = auth.hasPermission("rules.manage");
  const [rules, setRules] = useState<SecurityRule[]>([]);
  const [l2, setL2] = useState<L2ProviderConfig | null>(null);
  const [credentials, setCredentials] = useState<CredentialMeta[]>([]);
  const [error, setError] = useState<string>();
  const [form, setForm] = useState<Partial<SecurityRule> | null>(null);
  const [deleteRule, setDeleteRule] = useState<SecurityRule | null>(null);
  const [ruleQuery, setRuleQuery] = useState("");
  const [togglingId, setTogglingId] = useState<string | null>(null);

  const [sandboxCmd, setSandboxCmd] = useState("ls -la /var/log && df -h");
  const [sandboxResult, setSandboxResult] = useState<DualEvaluateResp | null>(null);

  const [l2Testing, setL2Testing] = useState(false);
  const [l2TestMsg, setL2TestMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const load = useCallback(async () => {
    try {
      setRules(await api.get<SecurityRule[]>("/api/rules"));
      setL2(await api.get<L2ProviderConfig>("/api/l2-config"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  const loadCredentials = useCallback(async () => {
    if (!auth.hasPermission("host.manage")) return;
    try {
      setCredentials(await api.get<CredentialMeta[]>("/api/credentials"));
    } catch {
      setCredentials([]);
    }
  }, [auth.identity?.user_id]);

  useEffect(() => {
    void load();
    void loadCredentials();
  }, [load, loadCredentials]);

  const filteredRules = rules.filter((r) => {
    const q = ruleQuery.trim().toLowerCase();
    if (!q) return true;
    return r.name.toLowerCase().includes(q) || r.pattern.toLowerCase().includes(q);
  });

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

  const toggleRule = async (rule: SecurityRule, enabled: boolean) => {
    setTogglingId(rule.id);
    try {
      await api.put(`/api/rules/${rule.id}`, { ...rule, enabled });
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "save failed");
    } finally {
      setTogglingId(null);
    }
  };

  const removeRule = async (id: string) => {
    try {
      await api.del(`/api/rules/${id}`);
      setDeleteRule(null);
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

  const testL2 = async () => {
    setL2Testing(true);
    setL2TestMsg(null);
    try {
      const res = await api.post<DualEvaluateResp>("/api/security/evaluate", {
        command: "echo sshield-l2-connectivity-test",
      });
      if (res.l2) {
        setL2TestMsg({ ok: true, text: t("l2.testOk", { ms: res.l2.latency_ms }) });
      } else {
        setL2TestMsg({ ok: true, text: `${t("security.l2Review")} · ${t("common.off")}` });
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : "evaluate failed";
      setL2TestMsg({ ok: false, text: `${t("l2.testFailed")}: ${msg}` });
    } finally {
      setL2Testing(false);
    }
  };

  const evaluate = async () => {
    try {
      setSandboxResult(
        await api.post<DualEvaluateResp>("/api/security/evaluate", {
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
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="relative w-full max-w-xs">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={ruleQuery}
                onChange={(e) => setRuleQuery(e.target.value)}
                placeholder={t("rules.search")}
                className="pl-8"
              />
            </div>
            {canManage && (
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
            )}
          </div>

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
                  <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                    {t("rules.enabledToggle")}
                  </TableHead>
                  <TableHead className="px-4" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredRules.map((r) => (
                  <TableRow key={r.id}>
                    <TableCell className="px-4 font-medium">{r.name}</TableCell>
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
                    <TableCell className="px-4">
                      <Switch
                        checked={r.enabled}
                        disabled={
                          !canManage || r.id.startsWith("builtin-") || togglingId === r.id
                        }
                        onCheckedChange={(v) => void toggleRule(r, v)}
                      />
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
                            onClick={() => setDeleteRule(r)}
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
                    items={{
                      system_one_fast: "SystemOne Fast",
                      openai_chat: "OpenAI Chat",
                      openai_responses: "OpenAI Responses",
                      anthropic_messages: "Anthropic Messages",
                    }}
                    onValueChange={(v) => v && setL2({ ...l2, protocol: v })}
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
                  <Label>{t("l2.credential")}</Label>
                  <Select
                    value={l2.api_key_credential_id || CREDENTIAL_NONE}
                    items={{
                      [CREDENTIAL_NONE]: t("l2.noCredential"),
                      ...Object.fromEntries(credentials.map((c) => [c.id, c.label])),
                    }}
                    onValueChange={(v) =>
                      setL2({ ...l2, api_key_credential_id: !v || v === CREDENTIAL_NONE ? null : v })
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        <SelectItem value={CREDENTIAL_NONE}>{t("l2.noCredential")}</SelectItem>
                        {credentials.map((c) => (
                          <SelectItem key={c.id} value={c.id}>
                            {c.label}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-muted-foreground">{t("l2.credentialHint")}</p>
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
                <CardFooter className="justify-end gap-3">
                  {l2TestMsg && (
                    <p
                      title={l2TestMsg.text}
                      className={`min-w-0 flex-1 truncate text-xs ${
                        l2TestMsg.ok ? "text-muted-foreground" : "text-destructive"
                      }`}
                    >
                      {l2TestMsg.text}
                    </p>
                  )}
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={l2Testing}
                    onClick={() => void testL2()}
                  >
                    {l2Testing ? (
                      <LoaderCircle className="animate-spin" data-icon="inline-start" />
                    ) : (
                      <Zap data-icon="inline-start" />
                    )}
                    {t("l2.test")}
                  </Button>
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
                onValueChange={(v) => v && setForm({ ...form!, match_kind: v })}
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
                onValueChange={(v) => v && setForm({ ...form!, decision: v })}
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
            <div className="col-span-2 flex items-center justify-between rounded-lg border border-border px-3 py-2">
              <Label htmlFor="rule-enabled">{t("rules.enabledToggle")}</Label>
              <Switch
                id="rule-enabled"
                checked={form?.enabled ?? true}
                onCheckedChange={(v) => setForm({ ...form!, enabled: v })}
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

      <ConfirmDialog
        open={deleteRule !== null}
        title={t("common.delete")}
        description={t("rules.deleteConfirm", { name: deleteRule?.name ?? "" })}
        confirmLabel={t("common.delete")}
        onConfirm={() => deleteRule && void removeRule(deleteRule.id)}
        onOpenChange={(o) => !o && setDeleteRule(null)}
      />
    </Page>
  );
}
