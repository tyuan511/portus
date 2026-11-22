import { useCallback, useEffect, useState } from "react";
import { Activity, Check, Copy, History, Server, ShieldCheck, Timer, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import { Pagination } from "@/components/sshield/Pagination";
import { StatusPill, decisionTone } from "@/components/sshield/StatusDot";
import { api, wsUrl } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { ApprovalHistoryItem, ApprovalRequest, Paged } from "@/types";

interface WsEvent {
  type: string;
  payload?: ApprovalRequest | null;
  approval_id?: string;
  decision?: string;
}

const HISTORY_PAGE_SIZE = 50;

export default function Approvals() {
  const { t } = useI18n();
  const [pending, setPending] = useState<ApprovalRequest[]>([]);
  const [error, setError] = useState<string>();
  const [recent, setRecent] = useState<string[]>([]);
  const [live, setLive] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const [tab, setTab] = useState("queue");
  const [history, setHistory] = useState<Paged<ApprovalHistoryItem> | null>(null);
  const [histPage, setHistPage] = useState(1);
  const [histError, setHistError] = useState<string>();

  const [decideTarget, setDecideTarget] = useState<{
    req: ApprovalRequest;
    approved: boolean;
  } | null>(null);
  const [reason, setReason] = useState("");
  const [decideError, setDecideError] = useState<string>();
  const [deciding, setDeciding] = useState(false);

  const load = useCallback(async () => {
    try {
      setPending(await api.get<ApprovalRequest[]>("/api/approvals"));
    } catch (err) {
      setError(err instanceof Error ? err.message : "load failed");
    }
  }, []);

  const loadHistory = useCallback(async (page: number) => {
    try {
      const data = await api.get<Paged<ApprovalHistoryItem>>(
        `/api/approvals/history?limit=${HISTORY_PAGE_SIZE}&offset=${(page - 1) * HISTORY_PAGE_SIZE}`
      );
      setHistory(data);
      setHistPage(page);
      setHistError(undefined);
    } catch (err) {
      setHistError(err instanceof Error ? err.message : "load failed");
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

  // 1s tick so every pending card can show its live countdown.
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  const remainingSecs = (req: ApprovalRequest): number => {
    const created = Date.parse(req.created_at);
    if (Number.isNaN(created)) return req.timeout_secs;
    return req.timeout_secs - (now - created) / 1000;
  };

  const openDecide = (req: ApprovalRequest, approved: boolean) => {
    setReason("");
    setDecideError(undefined);
    setDecideTarget({ req, approved });
  };

  const confirmDecide = async () => {
    if (!decideTarget) return;
    setDeciding(true);
    try {
      const note = reason.trim();
      await api.post(`/api/approvals/${decideTarget.req.id}/decide`, {
        approved: decideTarget.approved,
        ...(note ? { reason: note } : {}),
      });
      setPending((p) => p.filter((x) => x.id !== decideTarget.req.id));
      setDecideTarget(null);
      // Keep the history tab in sync if it has been opened before.
      if (history !== null) void loadHistory(histPage);
    } catch (err) {
      setDecideError(err instanceof Error ? err.message : "decide failed");
    } finally {
      setDeciding(false);
    }
  };

  const copyCommand = async (id: string, command: string) => {
    await navigator.clipboard.writeText(command);
    setCopiedId(id);
    window.setTimeout(() => setCopiedId((cur) => (cur === id ? null : cur)), 1600);
  };

  const openTab = (value: string) => {
    setTab(value);
    if (value === "history" && history === null) void loadHistory(1);
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
      <Tabs value={tab} onValueChange={(value) => openTab(String(value))} className="gap-5">
        <TabsList variant="line" className="w-full justify-start border-b border-border pb-0">
          <TabsTrigger value="queue">
            {t("approvals.title")}
            {pending.length > 0 && (
              <Badge variant="secondary" className="ml-1.5 font-mono text-[10px] tabular-nums">
                {pending.length}
              </Badge>
            )}
          </TabsTrigger>
          <TabsTrigger value="history">{t("approvals.history")}</TabsTrigger>
        </TabsList>

        <TabsContent value="queue" className="space-y-4">
          <ErrorText>{error}</ErrorText>

          {recent.length > 0 && (
            <div className="flex flex-wrap gap-2">
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
              {pending.map((req) => {
                const remaining = remainingSecs(req);
                const expired = remaining <= 0;
                return (
                  <Card key={req.id} className="vercel-surface gap-3">
                    <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-3 pb-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <StatusPill label="PENDING" tone="warning" pulse />
                        {expired ? (
                          <Badge variant="outline" className="font-mono text-[11px] font-normal text-geist-error">
                            timed_out
                          </Badge>
                        ) : (
                          <Badge variant="outline" className="font-mono text-[11px] font-normal text-geist-warning">
                            <Timer />
                            {t("approvals.waitTimer", { secs: Math.ceil(remaining) })}
                          </Badge>
                        )}
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
                        <Button size="sm" onClick={() => openDecide(req, true)}>
                          <Check data-icon="inline-start" />
                          {t("approvals.approve")}
                        </Button>
                        <Button size="sm" variant="destructive" onClick={() => openDecide(req, false)}>
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
                );
              })}
            </div>
          )}
        </TabsContent>

        <TabsContent value="history" className="space-y-3">
          <ErrorText>{histError}</ErrorText>

          {history !== null && history.items.length === 0 ? (
            <Empty className="mt-6 border border-border bg-card py-16">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <History />
                </EmptyMedia>
                <EmptyTitle>{t("approvals.historyTitle")}</EmptyTitle>
                <EmptyDescription>{t("approvals.emptyHistory")}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            history !== null && (
              <Card className="p-0">
                <Table>
                  <TableHeader className="bg-secondary/50">
                    <TableRow>
                      <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                        {t("approvals.colTime")}
                      </TableHead>
                      <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                        {t("approvals.colOutcome")}
                      </TableHead>
                      <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                        {t("audit.colServer")}
                      </TableHead>
                      <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                        {t("approvals.colBy")}
                      </TableHead>
                      <TableHead className="px-4 font-mono text-[11px] uppercase text-muted-foreground">
                        {t("approvals.reason")}
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {history.items.map((item) => (
                      <TableRow key={item.id}>
                        <TableCell className="whitespace-nowrap px-4 font-mono text-xs text-muted-foreground tabular-nums">
                          {new Date(item.timestamp).toLocaleString()}
                        </TableCell>
                        <TableCell className="px-4">
                          <StatusPill
                            label={item.approval_status}
                            tone={decisionTone(item.approval_status)}
                          />
                        </TableCell>
                        <TableCell className="px-4 text-xs font-medium">{item.server_name || "—"}</TableCell>
                        <TableCell className="max-w-96 px-4">
                          <div className="truncate font-mono text-xs" title={item.command}>
                            {item.command}
                          </div>
                          <div className="truncate text-xs text-muted-foreground">
                            {item.username || item.ssh_user || item.user_id || "—"}
                          </div>
                        </TableCell>
                        <TableCell
                          className="max-w-56 truncate px-4 text-xs text-muted-foreground"
                          title={item.note || undefined}
                        >
                          {item.note || "—"}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </Card>
            )
          )}

          <Pagination
            page={histPage}
            pageSize={HISTORY_PAGE_SIZE}
            total={history?.total ?? 0}
            onPageChange={(page) => void loadHistory(page)}
          />
        </TabsContent>
      </Tabs>

      {/* Approve / reject with an optional reason, recorded in the audit log. */}
      <Dialog
        open={decideTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDecideTarget(null);
        }}
      >
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>
              {decideTarget?.approved ? t("approvals.approve") : t("approvals.reject")}
            </DialogTitle>
            {decideTarget && (
              <DialogDescription>
                <code className="block break-all font-mono text-xs">
                  <span className="mr-1.5 select-none text-muted-foreground">$</span>
                  {decideTarget.req.command}
                </code>
              </DialogDescription>
            )}
          </DialogHeader>
          <form
            className="flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              void confirmDecide();
            }}
          >
            {decideError && <p className="text-xs text-destructive">{decideError}</p>}
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="approval-reason">{t("approvals.reason")}</Label>
              <Input
                id="approval-reason"
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                placeholder={t("approvals.reasonPlaceholder")}
                autoFocus
              />
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setDecideTarget(null)}
                disabled={deciding}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="submit"
                variant={decideTarget?.approved ? "default" : "destructive"}
                disabled={deciding}
              >
                {decideTarget?.approved ? t("approvals.approve") : t("approvals.reject")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </Page>
  );
}
