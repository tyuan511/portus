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
import { useI18n } from "@/lib/i18n";

// Supabaze-inspired landing (DESIGN.md): white canvas, ink ladder, a single
// emerald CTA per viewport, weight-500 display with negative tracking, and
// composited product panels in canvas-night instead of photography.
// Copy lives in i18n under landing.<key>.title / landing.<key>.body.
const PIPELINE = [
  { icon: ShieldCheck, key: "l1" },
  { icon: Activity, key: "l2" },
  { icon: KeyRound, key: "approval" },
  { icon: ScrollText, key: "audit" },
] as const;

const FEATURES = [
  { icon: GitBranch, key: "fRbac" },
  { icon: FileTerminal, key: "fTerminal" },
  { icon: Database, key: "fMcp" },
  { icon: Lock, key: "fHardened" },
] as const;

export default function Landing() {
  const auth = useAuth();
  const { t } = useI18n();
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
              {t("landing.cta.signin")}
            </Link>
            <Button size="sm" className="font-medium" onClick={() => navigate("/login")}>
              {t("landing.cta.console")}
            </Button>
          </nav>
        </div>
      </header>

      {/* hero */}
      <section className="mx-auto max-w-6xl px-6 pb-16 pt-20 text-center sm:pt-28">
        <p className="mb-4 inline-flex items-center gap-2 rounded-full border border-border bg-secondary px-3 py-1 text-xs text-muted-foreground">
          <span className="size-1.5 rounded-full bg-primary" />
          {t("landing.badge")}
        </p>
        <h1 className="mx-auto max-w-3xl text-4xl font-medium leading-[1.1] tracking-tight sm:text-6xl">
          {t("landing.hero.title1")}
          <br />
          <span className="text-muted-foreground">{t("landing.hero.title2")}</span>
        </h1>
        <p className="mx-auto mt-5 max-w-2xl text-base leading-relaxed text-muted-foreground sm:text-lg">
          {t("landing.hero.body")}
        </p>
        <div className="mt-8 flex items-center justify-center gap-3">
          <Button className="h-10 px-5 font-medium" onClick={() => navigate(auth.identity ? "/" : "/login")}>
            {auth.identity ? t("landing.cta.console") : t("landing.cta.signin")}
            <ArrowRight className="size-4" />
          </Button>
          <Button variant="outline" className="h-10 px-5 font-medium" onClick={() => document.getElementById("pipeline")?.scrollIntoView({ behavior: "smooth" })}>
            {t("landing.cta.pipeline")}
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
            <span className="ml-2 font-mono text-xs text-muted-foreground">{t("landing.mockCaption")}</span>
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
          <h2 className="text-3xl font-medium tracking-tight">{t("landing.pipeline.title")}</h2>
          <p className="mt-2 max-w-2xl text-muted-foreground">{t("landing.pipeline.sub")}</p>
          <div className="mt-10 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {PIPELINE.map((item, i) => (
              <div key={item.key} className="rounded-xl border border-border bg-card p-6">
                <div className="mb-3 flex items-center justify-between">
                  <item.icon className="size-5 text-primary" />
                  <span className="font-mono text-xs text-muted-foreground">0{i + 1}</span>
                </div>
                <h3 className="text-sm font-medium">{t(`landing.${item.key}.title`)}</h3>
                <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{t(`landing.${item.key}.body`)}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* feature grid */}
      <section className="py-20">
        <div className="mx-auto max-w-6xl px-6">
          <h2 className="text-3xl font-medium tracking-tight">{t("landing.features.title")}</h2>
          <div className="mt-10 grid gap-4 sm:grid-cols-2">
            {FEATURES.map((f) => (
              <div key={f.key} className="rounded-xl border border-border bg-card p-8">
                <f.icon className="mb-4 size-5 text-primary" />
                <h3 className="text-lg font-medium tracking-tight">{t(`landing.${f.key}.title`)}</h3>
                <p className="mt-2 leading-relaxed text-muted-foreground">{t(`landing.${f.key}.body`)}</p>
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
            <Link to="/login" className="hover:text-foreground">{t("landing.cta.signin")}</Link>
            <span>PostgreSQL · Go · React</span>
          </div>
        </div>
      </footer>
    </div>
  );
}
