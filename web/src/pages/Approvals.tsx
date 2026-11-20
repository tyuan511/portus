import { useCallback, useEffect, useState } from "react";
import { Activity, Check, Copy, Server, ShieldCheck, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import { StatusPill } from "@/components/sshield/StatusDot";
import { api, wsUrl } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { ApprovalRequest } from "@/types";

interface WsEvent {
  type: string;
  payload?: ApprovalRequest | null;
  approval_id?: string;
  decision?: string;
}

export default function Approvals() {
  const { t } = useI18n();
  const [pending, setPending] = useState<ApprovalRequest[]>([]);
  const [error, setError] = useState<string>();
  const [recent, setRecent] = useState<string[]>([]);
  const [live, setLive] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setPending(await api.get<ApprovalRequest[]>("/api/approvals"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  useEffect(() => {
    void load();
    const ws = new WebSocket(wsUrl("/ws"));
    ws.onopen = () => setLive(true);
    ws.onclose = () => setLive(false);
    ws.onmessage = (ev) => {
      try {
        const event = JSON.parse(ev.data) as WsEvent;
        if (event.type === "approval_requested" && event.payload) {
          setPending((p) => [...p.filter((x) => x.id !== event.payload!.id), event.payload!]);
        } else if (event.type === "approval_resolved" && event.approval_id) {
          setPending((p) => p.filter((x) => x.id !== event.approval_id));
          setRecent((r) => [`${event.approval_id}: ${event.decision}`, ...r].slice(0, 5));
        }
      } catch {
        /* ignore */
      }
    };
    return () => ws.close();
  }, [load]);

  const decide = async (id: string, approved: boolean) => {
    try {
      await api.post(`/api/approvals/${id}/decide`, { approved });
      setPending((p) => p.filter((x) => x.id !== id));
    } catch (err) {
      setError(err instanceof Error ? err.message : "decide failed");
    }
  };

  const copyCommand = async (id: string, command: string) => {
    await navigator.clipboard.writeText(command);
    setCopiedId(id);
    window.setTimeout(() => setCopiedId((cur) => (cur === id ? null : cur)), 1600);
  };

  return (
    <Page
      header={
        <PageHeader
          title={t("approvals.title")}
          actions={
            <StatusPill
              label={live ? t("approvals.live") : t("approvals.offline")}
              tone={live ? "success" : "warning"}
              pulse={live}
            />
          }
        />
      }
    >
      <ErrorText>{error}</ErrorText>

      {recent.length > 0 && (
        <div className="mb-4 flex flex-wrap gap-2">
          {recent.map((r, i) => (
            <Badge key={i} variant="outline" className="font-mono text-[11px] font-normal text-muted-foreground">
              <Activity />
              {t("approvals.recentlyResolved")}: {r}
            </Badge>
          ))}
        </div>
      )}

      {pending.length === 0 ? (
        <Empty className="mt-6 border border-border bg-card py-16">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <ShieldCheck />
            </EmptyMedia>
            <EmptyTitle>{t("approvals.title")}</EmptyTitle>
            <EmptyDescription>{t("approvals.empty")}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="flex flex-col gap-3">
          {pending.map((req) => (
            <Card key={req.id} className="vercel-surface gap-3">
              <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-3 pb-0">
                <div className="flex flex-wrap items-center gap-2">
                  <StatusPill label="PENDING" tone="warning" pulse />
                  <span className="inline-flex items-center gap-1.5 text-sm font-semibold">
                    <Server className="size-3.5 text-muted-foreground" />
                    {req.server_name}
                  </span>
                  <Badge variant="secondary" className="font-mono text-[11px]">
                    {req.requested_by || req.user_id || t("approvals.agent")}
                  </Badge>
                  <span className="font-mono text-xs text-muted-foreground">
                    {req.username}@{req.host}
                  </span>
                </div>

                <div className="flex items-center gap-2">
                  <Button size="sm" onClick={() => void decide(req.id, true)}>
                    <Check data-icon="inline-start" />
                    {t("approvals.approve")}
                  </Button>
                  <Button size="sm" variant="destructive" onClick={() => void decide(req.id, false)}>
                    <X data-icon="inline-start" />
                    {t("approvals.reject")}
                  </Button>
                </div>
              </CardHeader>

              <CardContent className="flex flex-col gap-2">
                <div className="flex items-start justify-between gap-2 rounded-lg border border-border bg-[#09090b] p-3 text-[#ededed]">
                  <code className="break-all font-mono text-xs leading-relaxed">
                    <span className="mr-2 select-none text-zinc-500">$</span>
                    {req.command}
                  </code>
                  <Button
                    type="button"
                    size="icon-xs"
                    variant="ghost"
                    className="shrink-0 text-zinc-400 hover:bg-white/10 hover:text-white"
                    onClick={() => void copyCommand(req.id, req.command)}
                  >
                    {copiedId === req.id ? <Check /> : <Copy />}
                  </Button>
                </div>
                <div className="text-xs text-muted-foreground">
                  <span className="font-mono uppercase text-foreground/70">{t("approvals.intent")}:</span>{" "}
                  {req.intent}
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </Page>
  );
}
