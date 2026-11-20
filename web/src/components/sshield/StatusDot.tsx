import { cn } from "cn";

export type StatusTone = "success" | "warning" | "danger" | "neutral" | "info";

const TONE_DOT: Record<StatusTone, string> = {
  success: "bg-geist-success",
  warning: "bg-geist-warning",
  danger: "bg-geist-error",
  info: "bg-geist-blue",
  neutral: "bg-muted-foreground/50",
};

export function decisionTone(decision: string): StatusTone {
  switch (decision) {
    case "allow":
    case "approved":
    case "valid":
      return "success";
    case "ask_approval":
    case "pending":
      return "warning";
    case "deny":
    case "rejected":
    case "failed":
      return "danger";
    default:
      return "neutral";
  }
}

export function StatusPill({
  label,
  tone = "neutral",
  pulse = false,
  className,
}: {
  label: string;
  tone?: StatusTone;
  pulse?: boolean;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border border-border bg-card px-2 py-0.5 font-mono text-[11px] font-medium text-foreground",
        className
      )}
    >
      <span className="relative flex size-1.5">
        {pulse && (
          <span
            className={cn(
              "absolute inline-flex size-full animate-ping rounded-full opacity-75",
              TONE_DOT[tone]
            )}
          />
        )}
        <span className={cn("relative inline-flex size-1.5 rounded-full", TONE_DOT[tone])} />
      </span>
      {label}
    </span>
  );
}
