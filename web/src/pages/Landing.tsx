import { Link, useNavigate } from "react-router-dom";
import {
  Activity,
  ArrowRight,
  Database,
  FileTerminal,
  GitBranch,
  KeyRound,
  Lock,
  ScrollText,
  ShieldCheck,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { BrandLogo } from "@/components/BrandLogo";
import { useAuth } from "@/lib/auth";

// Supabaze-inspired landing (DESIGN.md): white canvas, ink ladder, a single
// emerald CTA per viewport, weight-500 display with negative tracking, and
// composited product panels in canvas-night instead of photography.
const PIPELINE = [
  {
    icon: ShieldCheck,
    title: "L1 — Shell AST rules",
    body: "Every command is parsed into a bash AST before it ever reaches a host. Destructive programs are denied, unknown commands escalate — fail-closed by construction.",
  },
  {
    icon: Activity,
    title: "L2 — AI semantic review",
    body: "Optional second opinion from OpenAI, Anthropic or a dedicated decision model. Risk scoring and intent alignment; any provider failure fails closed.",
  },
  {
    icon: KeyRound,
    title: "Human approval",
    body: "Escalated commands pause in a live approval feed. Admins approve or reject in real time — the agent simply waits.",
  },
  {
    icon: ScrollText,
    title: "Tamper-evident audit",
    body: "Every execution, login and decision lands in a SHA-256 hash chain. Verify the whole history with one click.",
  },
];

const FEATURES = [
  {
    icon: GitBranch,
    title: "RBAC down to the host",
    body: "Users, roles and user groups; hosts and host groups grant connect / execute levels. Members see exactly what is granted — nothing else.",
  },
  {
    icon: FileTerminal,
    title: "Audited web terminal",
    body: "Full PTY sessions in the browser over websocket, with every submitted command recorded per user.",
  },
  {
    icon: Database,
    title: "MCP for AI agents",
    body: "Fourteen scoped tools over streamable HTTP with personal access tokens. Agents inherit the same RBAC as humans.",
  },
  {
    icon: Lock,
    title: "Hardened by default",
    body: "Argon2id passwords, TOTP two-factor, persisted brute-force lockouts, session revocation, and a sealed credential vault with idle auto-lock.",
  },
];

export default function Landing() {
  const auth = useAuth();
  const navigate = useNavigate();

  return (
    <div className="min-h-screen bg-background text-foreground">
      {/* nav-bar-light */}
      <header className="border-b border-border">
        <div className="mx-auto flex h-14 max-w-6xl items-center justify-between px-6">
          <div className="flex items-center gap-2">
            <BrandLogo size={26} />
            <span className="text-sm font-medium tracking-tight">SSHield</span>
          </div>
          <nav className="flex items-center gap-3">
            <Link to="/login" className="text-sm text-muted-foreground hover:text-foreground">
              Sign in
            </Link>
            <Button size="sm" className="font-medium" onClick={() => navigate("/login")}>
              Open console
            </Button>
          </nav>
        </div>
      </header>

      {/* hero */}
      <section className="mx-auto max-w-6xl px-6 pb-16 pt-20 text-center sm:pt-28">
        <p className="mb-4 inline-flex items-center gap-2 rounded-full border border-border bg-secondary px-3 py-1 text-xs text-muted-foreground">
          <span className="size-1.5 rounded-full bg-primary" />
          SSHield Enterprise — multi-user SSH gateway
        </p>
        <h1 className="mx-auto max-w-3xl text-4xl font-medium leading-[1.1] tracking-tight sm:text-6xl">
          Ship commands to production
          <br />
          <span className="text-muted-foreground">with a safety net.</span>
        </h1>
        <p className="mx-auto mt-5 max-w-2xl text-base leading-relaxed text-muted-foreground sm:text-lg">
          SSHield sits between your team, your AI agents and your servers: RBAC-scoped
          access, a layered command security pipeline, and a tamper-evident audit trail —
          in one binary with an embedded console.
        </p>
        <div className="mt-8 flex items-center justify-center gap-3">
          <Button className="h-10 px-5 font-medium" onClick={() => navigate(auth.identity ? "/" : "/login")}>
            {auth.identity ? "Open console" : "Sign in"}
            <ArrowRight className="size-4" />
          </Button>
          <Button variant="outline" className="h-10 px-5 font-medium" onClick={() => document.getElementById("pipeline")?.scrollIntoView({ behavior: "smooth" })}>
            How it guards commands
          </Button>
        </div>
      </section>

      {/* composited "product pane" — canvas-night config mockup */}
      <section className="mx-auto max-w-6xl px-6 pb-20">
        <div className="overflow-hidden rounded-xl border border-border shadow-[0_8px_24px_rgba(0,0,0,0.08)]">
          <div className="flex items-center gap-1.5 border-b border-border bg-secondary px-4 py-2.5">
            <span className="size-2.5 rounded-full bg-border" />
            <span className="size-2.5 rounded-full bg-border" />
            <span className="size-2.5 rounded-full bg-primary" />
            <span className="ml-2 font-mono text-xs text-muted-foreground">mcp.json — agent config</span>
          </div>
          <pre className="overflow-x-auto bg-[#1c1c1c] p-5 font-mono text-[13px] leading-relaxed text-[#ededed]">
{`{
  "mcpServers": {
    "sshield": {
      "url": "https://sshield.example.com/mcp",
      "headers": { "Authorization": "Bearer sk-sshield-…" }
    }
  }
}`}
            <span className="text-[#3ecf8e]">{"\n# every tool call is RBAC-scoped to the token owner"}</span>
            <span className="text-[#3ecf8e]">{"\n# every command: AST rules → L2 review → approval → audit"}</span>
          </pre>
        </div>
      </section>

      {/* security pipeline */}
      <section id="pipeline" className="border-t border-border bg-secondary/50 py-20">
        <div className="mx-auto max-w-6xl px-6">
          <h2 className="text-3xl font-medium tracking-tight">Four layers between intent and impact</h2>
          <p className="mt-2 max-w-2xl text-muted-foreground">
            Commands from the console and from AI agents pass the same pipeline —
            and every layer is recorded.
          </p>
          <div className="mt-10 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {PIPELINE.map((item, i) => (
              <div key={item.title} className="rounded-xl border border-border bg-card p-6">
                <div className="mb-3 flex items-center justify-between">
                  <item.icon className="size-5 text-primary" />
                  <span className="font-mono text-xs text-muted-foreground">0{i + 1}</span>
                </div>
                <h3 className="text-sm font-medium">{item.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{item.body}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* feature grid */}
      <section className="py-20">
        <div className="mx-auto max-w-6xl px-6">
          <h2 className="text-3xl font-medium tracking-tight">Enterprise access control, developer-grade UX</h2>
          <div className="mt-10 grid gap-4 sm:grid-cols-2">
            {FEATURES.map((f) => (
              <div key={f.title} className="rounded-xl border border-border bg-card p-8">
                <f.icon className="mb-4 size-5 text-primary" />
                <h3 className="text-lg font-medium tracking-tight">{f.title}</h3>
                <p className="mt-2 leading-relaxed text-muted-foreground">{f.body}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* footer-light */}
      <footer className="border-t border-border py-12">
        <div className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-6 text-sm text-muted-foreground sm:flex-row">
          <div className="flex items-center gap-2">
            <BrandLogo size={20} />
            SSHield Enterprise
          </div>
          <div className="flex items-center gap-6">
            <Link to="/login" className="hover:text-foreground">Sign in</Link>
            <span>PostgreSQL · Go · React</span>
          </div>
        </div>
      </footer>
    </div>
  );
}
