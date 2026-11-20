import { useCallback, useEffect, useState } from "react";
import { Check, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { api, wsUrl } from "@/lib/api";
import { ErrorText, Page, PageHeader } from "@/components/ui/shared";
import type { ApprovalRequest } from "@/types";

interface WsEvent {
  type: string;
  payload?: ApprovalRequest | null;
  approval_id?: string;
  decision?: string;
}

export default function Approvals() {
  const [pending, setPending] = useState<ApprovalRequest[]>([]);
  const [error, setError] = useState<string>();
  const [recent, setRecent] = useState<string[]>([]);

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
    ws.onmessage = (ev) => {
      try {
        const event = JSON.parse(ev.data) as WsEvent;
        if (event.type === "approval_requested" && event.payload) {
          setPending((p) => [...p.filter((x) => x.id !== event.payload!.id), event.payload!]);
        } else if (event.type === "approval_resolved" && event.approval_id) {
          setPending((p) => p.filter((x) => x.id !== event.approval_id));
          setRecent((r) => [`${event.approval_id}: ${event.decision}`, ...r].slice(0, 5));
        }
      } catch { /* ignore */ }
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

  return (
    <Page header={<PageHeader title="Pending Approvals" subtitle="Commands requiring human confirmation" />}>
      <ErrorText>{error}</ErrorText>
      {recent.length > 0 && (
        <div className="mb-3 space-y-0.5 text-xs text-muted-foreground">
          {recent.map((r, i) => <div key={i}>recently resolved — {r}</div>)}
        </div>
      )}
      <div className="space-y-2">
        {pending.map((req) => (
          <div key={req.id} className="rounded-xl border p-3">
            <div className="mb-1 flex items-center justify-between">
              <div className="flex items-center gap-2 text-sm font-medium">
                {req.server_name}
                <Badge variant="secondary">{req.requested_by || req.user_id || "agent"}</Badge>
              </div>
              <div className="flex gap-1.5">
                <Button size="xs" onClick={() => void decide(req.id, true)}><Check className="size-3" /> Approve</Button>
                <Button size="xs" variant="destructive" onClick={() => void decide(req.id, false)}><X className="size-3" /> Reject</Button>
              </div>
            </div>
            <code className="block break-all rounded-lg bg-muted/40 p-2 font-mono text-xs">{req.command}</code>
            <div className="mt-1 text-xs text-muted-foreground">
              intent: {req.intent} · {req.username}@{req.host}
            </div>
          </div>
        ))}
        {pending.length === 0 && <p className="mt-8 text-center text-sm text-muted-foreground">No pending approvals. Live updates active.</p>}
      </div>
    </Page>
  );
}
