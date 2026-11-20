import { useCallback, useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import SnippetsPanel from "@/pages/Snippets";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { CommandVerdict, L2ProviderConfig, PolicyDecision, SecurityRule, SemanticReviewVerdict } from "@/types";

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
  const [sandboxResult, setSandboxResult] = useState<{ l1: CommandVerdict; l2: SemanticReviewVerdict | null; final_decision: PolicyDecision } | null>(null);

  const load = useCallback(async () => {
    try {
      setRules(await api.get<SecurityRule[]>("/api/rules"));
      setL2(await api.get<L2ProviderConfig>("/api/l2-config"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

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
      setSandboxResult(await api.post("/api/security/evaluate", { command: sandboxCmd, intent: "sandbox evaluation" }));
    } catch (err) {
      setError(err instanceof Error ? err.message : "evaluate failed");
    }
  };

  return (
    <Page header={<PageHeader title={t("security.title")} />}>
      <ErrorText>{error}</ErrorText>
      <Tabs defaultValue="rules">
        <TabsList>
          <TabsTrigger value="rules">{t("security.rules")}</TabsTrigger>
          <TabsTrigger value="l2">{t("security.l2")}</TabsTrigger>
          <TabsTrigger value="sandbox">{t("security.sandbox")}</TabsTrigger>
          {auth.hasPermission("snippets.use") && <TabsTrigger value="snippets">{t("security.snippets")}</TabsTrigger>}
        </TabsList>

        <TabsContent value="rules">
          {canManage && (
            <Button size="sm" className="mb-3" onClick={() => setForm({ name: "", match_kind: "command_prefix", pattern: "", decision: "ask_approval", priority: 50, enabled: true })}>
              <Plus className="size-3.5" /> {t("security.newRule")}
            </Button>
          )}
          <div className="overflow-hidden rounded-xl border">
            <table className="w-full text-sm">
              <thead className="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
                <tr><th className="px-3 py-2">{t("security.colName")}</th><th className="px-3 py-2">{t("security.colMatch")}</th><th className="px-3 py-2">{t("security.colPattern")}</th><th className="px-3 py-2">{t("security.colDecision")}</th><th className="px-3 py-2">{t("security.colPrio")}</th><th className="px-3 py-2" /></tr>
              </thead>
              <tbody>
                {rules.map((r) => (
                  <tr key={r.id} className="border-b last:border-0">
                    <td className="px-3 py-1.5">{r.name}{!r.enabled && <Badge variant="secondary" className="ml-2">{t("common.off")}</Badge>}</td>
                    <td className="px-3 py-1.5 text-xs text-muted-foreground">{r.match_kind}</td>
                    <td className="max-w-72 truncate px-3 py-1.5 font-mono text-xs" title={r.pattern}>{r.pattern}</td>
                    <td className="px-3 py-1.5"><Badge variant={r.decision === "deny" ? "destructive" : r.decision === "allow" ? "default" : "secondary"}>{r.decision}</Badge></td>
                    <td className="px-3 py-1.5 text-xs">{r.priority}</td>
                    <td className="px-3 py-1.5 text-right">
                      {canManage && !r.id.startsWith("builtin-") && (
                        <>
                          <Button variant="ghost" size="icon-xs" className="mr-1" onClick={() => setForm(r)}>✎</Button>
                          <Button variant="destructive" size="icon-xs" onClick={() => void removeRule(r.id)}><Trash2 className="size-3" /></Button>
                        </>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </TabsContent>

        <TabsContent value="l2">
          {l2 && (
            <div className="max-w-2xl space-y-3 rounded-xl border p-4">
              <div className="flex items-center justify-between">
                <Label className="text-sm">{t("security.l2Review")}</Label>
                <Switch checked={l2.enabled} onCheckedChange={(v) => setL2({ ...l2, enabled: v })} />
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-1"><Label>{t("security.l2Protocol")}</Label>
                  <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={l2.protocol}
                    onChange={(e) => setL2({ ...l2, protocol: e.target.value as L2ProviderConfig["protocol"] })}>
                    <option value="system_one_fast">SystemOne Fast</option>
                    <option value="openai_chat">OpenAI Chat</option>
                    <option value="openai_responses">OpenAI Responses</option>
                    <option value="anthropic_messages">Anthropic Messages</option>
                  </select>
                </div>
                <div className="space-y-1"><Label>{t("security.l2BaseUrl")}</Label><Input value={l2.base_url} onChange={(e) => setL2({ ...l2, base_url: e.target.value })} /></div>
                <div className="space-y-1"><Label>{t("security.l2Model")}</Label><Input value={l2.model} onChange={(e) => setL2({ ...l2, model: e.target.value })} /></div>
                <div className="space-y-1"><Label>{t("security.l2ApiKey")}</Label><Input value={l2.api_key_credential_id ?? ""} onChange={(e) => setL2({ ...l2, api_key_credential_id: e.target.value })} /></div>
                <div className="space-y-1"><Label>{t("security.l2RiskAsk")}</Label><Input type="number" value={l2.risk_threshold_ask} onChange={(e) => setL2({ ...l2, risk_threshold_ask: Number(e.target.value) })} /></div>
                <div className="space-y-1"><Label>{t("security.l2RiskDeny")}</Label><Input type="number" value={l2.risk_threshold_deny} onChange={(e) => setL2({ ...l2, risk_threshold_deny: Number(e.target.value) })} /></div>
              </div>
              {canManage && <Button size="sm" onClick={() => void saveL2()}>{t("security.saveL2")}</Button>}
            </div>
          )}
        </TabsContent>

        <TabsContent value="sandbox">
          <div className="max-w-2xl space-y-3">
            <div className="flex gap-2">
              <Input className="font-mono text-xs" value={sandboxCmd} onChange={(e) => setSandboxCmd(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && void evaluate()} />
              <Button size="sm" onClick={() => void evaluate()}>{t("security.evaluate")}</Button>
            </div>
            {sandboxResult && (
              <pre className="max-h-96 overflow-auto rounded-xl border bg-muted/30 p-3 text-xs">
                {JSON.stringify(sandboxResult, null, 2)}
              </pre>
            )}
          </div>
        </TabsContent>
      </Tabs>

      <Dialog open={form !== null} onOpenChange={(o) => !o && setForm(null)}>
        <DialogContent>
          <DialogHeader><DialogTitle>{form?.id ? t("security.editRule") : t("security.newRule")}</DialogTitle></DialogHeader>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1"><Label>{t("common.name")}</Label><Input value={form?.name ?? ""} onChange={(e) => setForm({ ...form!, name: e.target.value })} /></div>
            <div className="space-y-1"><Label>{t("security.matchKind")}</Label>
              <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.match_kind}
                onChange={(e) => setForm({ ...form!, match_kind: e.target.value as SecurityRule["match_kind"] })}>
                {RULE_KINDS.map((k) => <option key={k} value={k}>{k}</option>)}
              </select>
            </div>
            <div className="col-span-2 space-y-1"><Label>{t("security.pattern")}</Label><Input value={form?.pattern ?? ""} onChange={(e) => setForm({ ...form!, pattern: e.target.value })} /></div>
            <div className="space-y-1"><Label>{t("security.decision")}</Label>
              <select className="h-8 w-full rounded-lg border bg-background px-2 text-sm" value={form?.decision}
                onChange={(e) => setForm({ ...form!, decision: e.target.value as PolicyDecision })}>
                {DECISIONS.map((d) => <option key={d} value={d}>{d}</option>)}
              </select>
            </div>
            <div className="space-y-1"><Label>{t("common.priority")}</Label><Input type="number" value={form?.priority ?? 50} onChange={(e) => setForm({ ...form!, priority: Number(e.target.value) })} /></div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setForm(null)}>{t("common.cancel")}</Button>
            <Button onClick={() => void saveRule()}>{t("common.save")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
