import { useEffect, useState } from "react";
import { Check, Copy, Lock, ShieldAlert, ShieldCheck, Terminal, UserCheck } from "lucide-react";
import { cn } from "cn";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import { GridCrosshair } from "./GridCrosshair";
import { StatusPill, type StatusTone } from "./StatusDot";

type ScenarioId = "safe" | "escalate" | "blocked" | "config";

interface PipelineStep {
  label: string;
  detail: string;
  tone: StatusTone;
  active: boolean;
}

interface Scenario {
  id: ScenarioId;
  command: string;
  host: string;
  actor: string;
  verdict: string;
  tone: StatusTone;
  latency: string;
  steps: PipelineStep[];
  output: string[];
}

const SCENARIOS: Scenario[] = [
  {
    id: "safe",
    command: "df -h /var/log && uptime",
    host: "prod-web-01",
    actor: "claude-code (mcp)",
    verdict: "ALLOW",
    tone: "success",
    latency: "0.4ms",
    steps: [
      { label: "L1 AST", detail: "program_exact: df, uptime", tone: "success", active: true },
      { label: "L2 AI", detail: "bypassed (allowlist)", tone: "neutral", active: false },
      { label: "Approval", detail: "not required", tone: "neutral", active: false },
      { label: "Audit", detail: "sha256:9f84…c1a2", tone: "success", active: true },
    ],
    output: [
      "$ df -h /var/log && uptime",
      "Filesystem      Size  Used Avail Use% Mounted on",
      "/dev/nvme0n1p1  120G   42G   78G  36% /var/log",
      " 22:39:10 up 142 days, 18:12,  0 users,  load average: 0.41, 0.38, 0.35",
    ],
  },
  {
    id: "escalate",
    command: "systemctl restart nginx",
    host: "prod-edge-02",
    actor: "ops-agent (mcp)",
    verdict: "APPROVED",
    tone: "warning",
    latency: "1.8s",
    steps: [
      { label: "L1 AST", detail: "escalate: systemctl", tone: "warning", active: true },
      { label: "L2 AI", detail: "risk 54/100 · service reload", tone: "warning", active: true },
      { label: "Approval", detail: "approved by @alice", tone: "success", active: true },
      { label: "Audit", detail: "sha256:4b12…8e70", tone: "success", active: true },
    ],
    output: [
      "[sshield] L1 AST -> ask_approval (service mutation)",
      "[sshield] L2 Semantic -> risk=54/100 (matches intent: reload TLS cert)",
      "[sshield] Waiting for human approval in live feed...",
      "[sshield] Approved by @alice (0.9s) -> executing on prod-edge-02",
      "✔ nginx.service restarted cleanly (exit 0)",
    ],
  },
  {
    id: "blocked",
    command: "rm -rf /var/lib/postgresql --no-preserve-root",
    host: "prod-db-01",
    actor: "deploy-bot (mcp)",
    verdict: "DENY",
    tone: "danger",
    latency: "0.2ms",
    steps: [
      { label: "L1 AST", detail: "denied: destructive rm", tone: "danger", active: true },
      { label: "L2 AI", detail: "halted (fail-closed)", tone: "neutral", active: false },
      { label: "Approval", detail: "rejected upstream", tone: "neutral", active: false },
      { label: "Audit", detail: "sha256:e77c…09d4", tone: "danger", active: true },
    ],
    output: [
      "[sshield] L1 AST -> DENY (rule: builtin-deny-destructive-rm)",
      "[sshield] Command never reached prod-db-01.",
      "[sshield] Event sealed into tamper-evident SHA-256 audit chain.",
    ],
  },
  {
    id: "config",
    command: "~/.cursor/mcp.json",
    host: "streamable-http",
    actor: "12 scoped tools",
    verdict: "RBAC",
    tone: "info",
    latency: "TLS",
    steps: [
      { label: "Token", detail: "sk-sshield-… (user-scoped)", tone: "info", active: true },
      { label: "RBAC", detail: "group × host matrix", tone: "info", active: true },
      { label: "Pipeline", detail: "L1 + L2 enforced", tone: "success", active: true },
      { label: "Audit", detail: "per-token attribution", tone: "success", active: true },
    ],
    output: [
      "{",
      '  "mcpServers": {',
      '    "sshield": {',
      '      "url": "https://sshield.example.com/mcp",',
      '      "headers": { "Authorization": "Bearer sk-sshield-…" }',
      "    }",
      "  }",
      "}",
    ],
  },
];

const MCP_JSON = `{
  "mcpServers": {
    "sshield": {
      "url": "https://sshield.example.com/mcp",
      "headers": {
        "Authorization": "Bearer sk-sshield-..."
      }
    }
  }
}`;

export function HeroShowcase() {
  const { t } = useI18n();
  const [selected, setSelected] = useState<ScenarioId>("escalate");
  const [paused, setPaused] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (paused) return;
    const order: ScenarioId[] = ["safe", "escalate", "blocked", "config"];
    const timer = window.setInterval(() => {
      setSelected((cur) => order[(order.indexOf(cur) + 1) % order.length]);
    }, 4500);
    return () => window.clearInterval(timer);
  }, [paused]);

  const active = SCENARIOS.find((s) => s.id === selected) ?? SCENARIOS[0];

  const tabLabel = (id: ScenarioId) => {
    switch (id) {
      case "safe":
        return t("landing.showcase.safe");
      case "escalate":
        return t("landing.showcase.escalate");
      case "blocked":
        return t("landing.showcase.blocked");
      case "config":
        return t("landing.showcase.config");
    }
  };

  const handleCopy = async () => {
    await navigator.clipboard.writeText(MCP_JSON);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1800);
  };

  return (
    <div className="relative mx-auto max-w-5xl">
      <GridCrosshair position="top-left" />
      <GridCrosshair position="top-right" />
      <GridCrosshair position="bottom-left" />
      <GridCrosshair position="bottom-right" />

      <div className="vercel-surface overflow-hidden rounded-xl bg-card text-left">
        {/* Window Chrome */}
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border bg-secondary/70 px-4 py-2.5">
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-1.5">
              <span className="size-2.5 rounded-full bg-border" />
              <span className="size-2.5 rounded-full bg-border" />
              <span className="size-2.5 rounded-full bg-border" />
            </div>
            <span className="hidden font-mono text-[11px] text-muted-foreground sm:inline">
              {t("landing.showcase.live")}
            </span>
          </div>

          <div className="flex flex-wrap items-center gap-1 rounded-lg border border-border bg-background p-0.5">
            {SCENARIOS.map((s) => (
              <button
                key={s.id}
                type="button"
                onClick={() => {
                  setPaused(true);
                  setSelected(s.id);
                }}
                className={cn(
                  "rounded-md px-2.5 py-1 font-mono text-[11px] font-medium transition-all",
                  selected === s.id
                    ? "bg-primary text-primary-foreground shadow-xs"
                    : "text-muted-foreground hover:text-foreground"
                )}
              >
                {tabLabel(s.id)}
              </button>
            ))}
          </div>
        </div>

        {/* 4-Stage Pipeline Bar */}
        <div className="grid grid-cols-2 gap-px border-b border-border bg-border sm:grid-cols-4">
          {active.steps.map((step, idx) => (
            <div
              key={step.label}
              className={cn(
                "flex flex-col gap-1 bg-card px-4 py-3 transition-colors",
                !step.active && "opacity-50"
              )}
            >
              <div className="flex items-center justify-between">
                <span className="font-mono text-[11px] font-medium text-foreground">
                  0{idx + 1} · {step.label}
                </span>
                <span
                  className={cn(
                    "size-1.5 rounded-full",
                    step.tone === "success" && "bg-geist-success",
                    step.tone === "warning" && "bg-geist-warning",
                    step.tone === "danger" && "bg-geist-error",
                    step.tone === "info" && "bg-geist-blue",
                    step.tone === "neutral" && "bg-muted-foreground/40"
                  )}
                />
              </div>
              <span className="truncate font-mono text-[11px] text-muted-foreground">
                {step.detail}
              </span>
            </div>
          ))}
        </div>

        {/* Dark High-Contrast Terminal Surface */}
        <div className="bg-[#09090b] p-5 text-[#ededed]">
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2 border-b border-white/10 pb-3">
            <div className="flex items-center gap-2 font-mono text-xs">
              {selected === "blocked" ? (
                <ShieldAlert className="size-3.5 text-geist-error" />
              ) : selected === "escalate" ? (
                <UserCheck className="size-3.5 text-geist-warning" />
              ) : selected === "config" ? (
                <Lock className="size-3.5 text-geist-blue" />
              ) : (
                <ShieldCheck className="size-3.5 text-geist-success" />
              )}
              <span className="text-white/90">{active.host}</span>
              <span className="text-white/30">·</span>
              <span className="text-white/60">{active.actor}</span>
            </div>

            <div className="flex items-center gap-2">
              <span className="font-mono text-[11px] text-white/50">{active.latency}</span>
              <StatusPill
                label={active.verdict}
                tone={active.tone}
                pulse={selected === "escalate"}
                className="border-white/15 bg-white/5 text-white"
              />
              {selected === "config" && (
                <Button
                  type="button"
                  size="xs"
                  variant="ghost"
                  className="h-6 border border-white/15 bg-white/5 px-2 font-mono text-[11px] text-white hover:bg-white/10 hover:text-white"
                  onClick={() => void handleCopy()}
                >
                  {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
                  {copied ? t("common.copied") : t("common.copy")}
                </Button>
              )}
            </div>
          </div>

          <pre className="overflow-x-auto font-mono text-[12.5px] leading-6 whitespace-pre-wrap break-words sm:whitespace-pre">
            {active.output.map((line, i) => (
              <div
                key={i}
                className={cn(
                  line.startsWith("$") && "font-medium text-white",
                  line.startsWith("✔") && "text-emerald-400",
                  line.includes("DENY") && "text-red-400",
                  line.includes("risk=") && "text-amber-300",
                  !line.startsWith("$") &&
                    !line.startsWith("✔") &&
                    !line.includes("DENY") &&
                    !line.includes("risk=") &&
                    "text-zinc-300"
                )}
              >
                {line}
              </div>
            ))}
          </pre>

          <div className="mt-4 flex flex-wrap items-center justify-between gap-2 border-t border-white/10 pt-3 font-mono text-[11px] text-zinc-500">
            <span className="flex min-w-0 items-center gap-1.5">
              <Terminal className="size-3 shrink-0" />
              <span className="truncate">{active.command}</span>
            </span>
            <span className="shrink-0">L1 AST → L2 AI → Approval → SHA-256</span>
          </div>
        </div>
      </div>
    </div>
  );
}
