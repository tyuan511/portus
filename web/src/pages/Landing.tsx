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
import { GridCrosshair } from "@/components/sshield/GridCrosshair";
import { HeroShowcase } from "@/components/sshield/HeroShowcase";
import { LangToggle } from "@/components/sshield/LangToggle";
import { StatusPill } from "@/components/sshield/StatusDot";
import { ThemeToggle } from "@/components/sshield/ThemeToggle";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";

const PIPELINE = [
  { icon: ShieldCheck, key: "l1", code: "ast.Parse -> deny | ask" },
  { icon: Activity, key: "l2", code: "risk_score: 0..100" },
  { icon: KeyRound, key: "approval", code: "ws://approvals -> live" },
  { icon: ScrollText, key: "audit", code: "sha256(prev || event)" },
] as const;

const MCP_TOOLS = ["ssh_list_hosts", "ssh_exec", "ssh_read_file", "ssh_tail_log", "+8 tools"];
const HARDENED_PILLS = ["Argon2id", "TOTP 2FA", "AES-GCM Vault", "Lockout"];

export default function Landing() {
  const auth = useAuth();
  const { t } = useI18n();
  const navigate = useNavigate();

  return (
    <div className="min-h-screen bg-background text-foreground selection:bg-primary selection:text-primary-foreground">
      {/* Sticky Frosted Navbar */}
      <header className="sticky top-0 z-40 border-b border-border/80 bg-background/80 backdrop-blur-md">
        <div className="mx-auto flex h-14 max-w-6xl items-center justify-between px-4 sm:px-6">
          <div className="flex items-center gap-6">
            <Link to="/" className="flex items-center gap-2.5">
              <BrandLogo size={24} />
              <span className="text-sm font-semibold tracking-tight">SSHield</span>
              <span className="hidden rounded-full border border-border bg-secondary px-2 py-0.5 font-mono text-[10px] font-medium text-muted-foreground sm:inline-block">
                ENT
              </span>
            </Link>
            <nav className="hidden items-center gap-5 text-sm text-muted-foreground md:flex">
              <a href="#pipeline" className="transition-colors hover:text-foreground">
                {t("landing.nav.pipeline")}
              </a>
              <a href="#features" className="transition-colors hover:text-foreground">
                {t("landing.nav.features")}
              </a>
              <a href="#security" className="transition-colors hover:text-foreground">
                {t("landing.nav.security")}
              </a>
            </nav>
          </div>

          <div className="flex items-center gap-1.5 sm:gap-2">
            <LangToggle compact />
            <ThemeToggle compact />
            <Button
              variant="ghost"
              size="sm"
              className="hidden sm:inline-flex"
              onClick={() => navigate("/login")}
            >
              {t("landing.cta.signin")}
            </Button>
            <Button size="sm" onClick={() => navigate(auth.identity ? "/" : "/login")}>
              {t("landing.cta.console")}
              <ArrowRight data-icon="inline-end" />
            </Button>
          </div>
        </div>
      </header>

      {/* Architectural Hero */}
      <section className="relative overflow-hidden border-b border-border">
        <div
          aria-hidden="true"
          className="vercel-grid-bg pointer-events-none absolute inset-0 [mask-image:radial-gradient(ellipse_75%_65%_at_50%_20%,#000_40%,transparent_100%)]"
        />
        <div aria-hidden="true" className="vercel-hero-glow pointer-events-none absolute inset-0" />

        <div className="relative mx-auto max-w-6xl px-4 pb-16 pt-16 text-center sm:px-6 sm:pb-24 sm:pt-24">
          <div className="mb-6 flex justify-center">
            <StatusPill label={t("landing.badge")} tone="success" pulse />
          </div>

          <h1 className="mx-auto max-w-4xl text-balance text-4xl font-semibold leading-[1.06] tracking-[-0.04em] sm:text-6xl lg:text-[64px]">
            {t("landing.hero.title1")}
            <br />
            <span className="bg-gradient-to-b from-foreground to-muted-foreground bg-clip-text text-transparent">
              {t("landing.hero.title2")}
            </span>
          </h1>

          <p className="mx-auto mt-6 max-w-2xl text-pretty text-base leading-relaxed text-muted-foreground sm:text-lg">
            {t("landing.hero.body")}
          </p>

          <div className="mt-8 flex flex-wrap items-center justify-center gap-3">
            <Button
              size="lg"
              className="h-10 rounded-full px-6 font-medium shadow-xs"
              onClick={() => navigate(auth.identity ? "/" : "/login")}
            >
              {auth.identity ? t("landing.cta.console") : t("landing.cta.signin")}
              <ArrowRight data-icon="inline-end" />
            </Button>
            <Button
              variant="outline"
              size="lg"
              className="h-10 rounded-full bg-background/80 px-6 font-medium backdrop-blur-xs"
              onClick={() =>
                document.getElementById("pipeline")?.scrollIntoView({ behavior: "smooth" })
              }
            >
              {t("landing.cta.pipeline")}
            </Button>
          </div>

          <div className="mt-12 sm:mt-16">
            <HeroShowcase />
          </div>
        </div>
      </section>

      {/* Architectural Telemetry Strip */}
      <section className="border-b border-border bg-card">
        <div className="mx-auto grid max-w-6xl grid-cols-2 divide-x divide-y divide-border sm:grid-cols-4 sm:divide-y-0">
          {[
            { label: t("landing.stat.ast"), value: "< 1 ms" },
            { label: t("landing.stat.posture"), value: t("landing.stat.postureVal") },
            { label: t("landing.stat.mcp"), value: "12 Tools" },
            { label: t("landing.stat.audit"), value: "SHA-256" },
          ].map((stat) => (
            <div key={stat.label} className="flex flex-col gap-1 px-6 py-5">
              <span className="font-mono text-[11px] uppercase tracking-wider text-muted-foreground">
                {stat.label}
              </span>
              <span className="text-xl font-semibold tracking-tight tabular-nums sm:text-2xl">
                {stat.value}
              </span>
            </div>
          ))}
        </div>
      </section>

      {/* Four-Layer Security Pipeline */}
      <section id="pipeline" className="mx-auto max-w-6xl px-4 py-20 sm:px-6 sm:py-24">
        <div className="max-w-2xl">
          <p className="font-mono text-xs uppercase tracking-widest text-muted-foreground">
            {t("landing.pipeline.eyebrow")}
          </p>
          <h2 className="mt-2 text-balance text-3xl font-semibold tracking-[-0.03em] sm:text-4xl">
            {t("landing.pipeline.title")}
          </h2>
          <p className="mt-3 text-pretty text-base leading-relaxed text-muted-foreground">
            {t("landing.pipeline.sub")}
          </p>
        </div>

        <div className="relative mt-10 border border-border">
          <GridCrosshair position="top-left" />
          <GridCrosshair position="top-right" />
          <GridCrosshair position="bottom-left" />
          <GridCrosshair position="bottom-right" />

          <div className="grid gap-px bg-border sm:grid-cols-2 lg:grid-cols-4">
            {PIPELINE.map((item, i) => (
              <div
                key={item.key}
                className="group flex flex-col justify-between bg-card p-6 transition-colors hover:bg-secondary/40"
              >
                <div>
                  <div className="mb-5 flex items-center justify-between">
                    <div className="flex size-9 items-center justify-center rounded-lg border border-border bg-background text-foreground shadow-2xs transition-transform group-hover:scale-105">
                      <item.icon className="size-4" />
                    </div>
                    <span className="font-mono text-xs text-muted-foreground tabular-nums">
                      0{i + 1}
                    </span>
                  </div>
                  <h3 className="text-sm font-semibold tracking-tight">
                    {t(`landing.${item.key}.title`)}
                  </h3>
                  <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                    {t(`landing.${item.key}.body`)}
                  </p>
                </div>

                <div className="mt-6 border-t border-border/70 pt-3">
                  <code className="font-mono text-[11px] text-muted-foreground">
                    {item.code}
                  </code>
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Bento Capabilities Grid */}
      <section id="features" className="border-t border-border bg-secondary/30 py-20 sm:py-24">
        <div className="mx-auto max-w-6xl px-4 sm:px-6">
          <div className="max-w-2xl">
            <p className="font-mono text-xs uppercase tracking-widest text-muted-foreground">
              {t("landing.features.eyebrow")}
            </p>
            <h2 className="mt-2 text-balance text-3xl font-semibold tracking-[-0.03em] sm:text-4xl">
              {t("landing.features.title")}
            </h2>
            <p className="mt-3 text-pretty text-base leading-relaxed text-muted-foreground">
              {t("landing.features.sub")}
            </p>
          </div>

          <div className="mt-10 overflow-hidden rounded-xl border border-border bg-border">
            <div className="grid gap-px md:grid-cols-2">
              {/* RBAC Card */}
              <div className="flex flex-col justify-between bg-card p-7 sm:p-8">
                <div>
                  <div className="mb-4 flex size-9 items-center justify-center rounded-lg border border-border bg-secondary">
                    <GitBranch className="size-4" />
                  </div>
                  <h3 className="text-lg font-semibold tracking-tight">
                    {t("landing.fRbac.title")}
                  </h3>
                  <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                    {t("landing.fRbac.body")}
                  </p>
                </div>
                <div className="mt-6 rounded-lg border border-border bg-background p-3 font-mono text-xs">
                  <div className="flex flex-wrap items-center justify-between gap-2 text-muted-foreground">
                    <div className="flex items-center gap-2 whitespace-nowrap">
                      <span>ops-team</span>
                      <span>→</span>
                      <span className="text-foreground">prod-cluster</span>
                    </div>
                    <StatusPill label="connect · exec" tone="success" className="shrink-0 whitespace-nowrap" />
                  </div>
                </div>
              </div>

              {/* Audited Web Terminal */}
              <div className="flex flex-col justify-between bg-card p-7 sm:p-8">
                <div>
                  <div className="mb-4 flex size-9 items-center justify-center rounded-lg border border-border bg-secondary">
                    <FileTerminal className="size-4" />
                  </div>
                  <h3 className="text-lg font-semibold tracking-tight">
                    {t("landing.fTerminal.title")}
                  </h3>
                  <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                    {t("landing.fTerminal.body")}
                  </p>
                </div>
                <div className="mt-6 flex items-center justify-between rounded-lg border border-border bg-[#09090b] px-3.5 py-2.5 font-mono text-xs text-zinc-200">
                  <span>
                    <span className="text-emerald-400">ops@prod-01</span>
                    <span className="text-zinc-500">:~$</span> journalctl -u nginx -n 20
                  </span>
                  <span className="text-[10px] text-zinc-500">PTY · WS</span>
                </div>
              </div>

              {/* MCP for AI Agents */}
              <div className="flex flex-col justify-between bg-card p-7 sm:p-8">
                <div>
                  <div className="mb-4 flex size-9 items-center justify-center rounded-lg border border-border bg-secondary">
                    <Database className="size-4" />
                  </div>
                  <h3 className="text-lg font-semibold tracking-tight">
                    {t("landing.fMcp.title")}
                  </h3>
                  <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                    {t("landing.fMcp.body")}
                  </p>
                </div>
                <div className="mt-6 flex flex-wrap gap-1.5">
                  {MCP_TOOLS.map((tool) => (
                    <span
                      key={tool}
                      className="rounded-md border border-border bg-background px-2 py-1 font-mono text-[11px] text-muted-foreground"
                    >
                      {tool}
                    </span>
                  ))}
                </div>
              </div>

              {/* Hardened by Default */}
              <div id="security" className="flex flex-col justify-between bg-card p-7 sm:p-8">
                <div>
                  <div className="mb-4 flex size-9 items-center justify-center rounded-lg border border-border bg-secondary">
                    <Lock className="size-4" />
                  </div>
                  <h3 className="text-lg font-semibold tracking-tight">
                    {t("landing.fHardened.title")}
                  </h3>
                  <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                    {t("landing.fHardened.body")}
                  </p>
                </div>
                <div className="mt-6 flex flex-wrap gap-1.5">
                  {HARDENED_PILLS.map((pill) => (
                    <span
                      key={pill}
                      className="inline-flex items-center gap-1.5 rounded-md border border-border bg-background px-2.5 py-1 font-mono text-[11px] text-foreground"
                    >
                      <span className="size-1.5 rounded-full bg-geist-success" />
                      {pill}
                    </span>
                  ))}
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Bottom CTA Banner */}
      <section className="relative overflow-hidden border-t border-border">
        <div
          aria-hidden="true"
          className="vercel-dot-bg pointer-events-none absolute inset-0 opacity-60 [mask-image:radial-gradient(ellipse_60%_70%_at_50%_50%,#000_20%,transparent_100%)]"
        />
        <div className="relative mx-auto flex max-w-6xl flex-col items-center justify-between gap-6 px-4 py-16 text-center sm:flex-row sm:px-6 sm:text-left">
          <div className="max-w-xl">
            <h2 className="text-2xl font-semibold tracking-[-0.03em] sm:text-3xl">
              {t("landing.ctaBanner.title")}
            </h2>
            <p className="mt-2 text-sm text-muted-foreground sm:text-base">
              {t("landing.ctaBanner.sub")}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-3">
            <Button
              size="lg"
              className="h-10 rounded-full px-6 font-medium"
              onClick={() => navigate(auth.identity ? "/" : "/login")}
            >
              {auth.identity ? t("landing.cta.console") : t("landing.cta.signin")}
              <ArrowRight data-icon="inline-end" />
            </Button>
          </div>
        </div>
      </section>

      {/* Architectural Footer */}
      <footer className="border-t border-border bg-card py-10">
        <div className="mx-auto flex max-w-6xl flex-col items-center justify-between gap-4 px-4 text-xs text-muted-foreground sm:flex-row sm:px-6">
          <div className="flex items-center gap-2.5">
            <BrandLogo size={20} />
            <span className="font-medium text-foreground">SSHield Enterprise</span>
            <span className="text-border">·</span>
            <span>RBAC + AST + MCP</span>
          </div>
          <div className="flex items-center gap-5">
            <Link to="/login" className="transition-colors hover:text-foreground">
              {t("landing.cta.signin")}
            </Link>
            <span className="font-mono">Go · React · PostgreSQL · SQLite</span>
          </div>
        </div>
      </footer>
    </div>
  );
}
