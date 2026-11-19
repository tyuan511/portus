import { AlertTriangle, CheckCircle2, XCircle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import type { PolicyDecision } from "@/types";

export function DecisionBadge({ decision }: { decision: PolicyDecision }) {
  if (decision === "allow") {
    return (
      <Badge className="gap-1 border-emerald-500/30 bg-emerald-500/15 text-emerald-300 hover:bg-emerald-500/20">
        <CheckCircle2 className="size-3" /> Allow
      </Badge>
    );
  }
  if (decision === "ask_approval") {
    return (
      <Badge className="gap-1 border-amber-500/30 bg-amber-500/15 text-amber-300 hover:bg-amber-500/20">
        <AlertTriangle className="size-3" /> Ask Approval
      </Badge>
    );
  }
  return (
    <Badge className="gap-1 border-rose-500/30 bg-rose-500/15 text-rose-300 hover:bg-rose-500/20">
      <XCircle className="size-3" /> Deny
    </Badge>
  );
}
