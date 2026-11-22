import { useEffect } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import {
  Activity,
  LogOut,
  ScrollText,
  Server,
  Settings,
  Shield,
  Users,
} from "lucide-react";
import { cn } from "cn";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { applyTheme, getSavedThemeId } from "@/lib/themes";
import { WorkspaceTabsProvider, useWorkspaceTabs, type PageKey } from "@/lib/workspace-tabs";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { BrandLogo } from "@/components/BrandLogo";
import { LangToggle } from "@/components/sshield/LangToggle";
import { ThemeToggle } from "@/components/sshield/ThemeToggle";
import { WorkspaceTabBar } from "@/components/sshield/WorkspaceTabBar";
import { WorkspaceView } from "@/components/sshield/WorkspaceView";
import Login from "@/pages/Login";
import Landing from "@/pages/Landing";

function AuthenticatedShell() {
  const auth = useAuth();
  const { t } = useI18n();
  const { activeTab, tabs, openPage, canAccessPage } = useWorkspaceTabs();

  type NavGroup = {
    label: string;
    items: { page: PageKey; label: string; icon: typeof Server; show: boolean }[];
  };

  const rawNavGroups: NavGroup[] = [
    {
      label: t("nav.group.workspace"),
      items: [
        { page: "hosts", label: t("nav.hosts"), icon: Server, show: canAccessPage("hosts") },
        { page: "approvals", label: t("nav.approvals"), icon: Activity, show: canAccessPage("approvals") },
      ],
    },
    {
      label: t("nav.group.governance"),
      items: [
        { page: "audit", label: t("nav.audit"), icon: ScrollText, show: canAccessPage("audit") },
        { page: "security", label: t("nav.security"), icon: Shield, show: canAccessPage("security") },
      ],
    },
    {
      label: t("nav.group.system"),
      items: [
        { page: "admin", label: t("nav.admin"), icon: Users, show: canAccessPage("admin") },
        { page: "settings", label: t("nav.settings"), icon: Settings, show: canAccessPage("settings") },
      ],
    },
  ];

  const navGroups: NavGroup[] = rawNavGroups
    .map((g) => ({ ...g, items: g.items.filter((i) => i.show) }))
    .filter((g) => g.items.length > 0);

  const identity = auth.identity!;
  const displayName = identity.display_name || identity.username;
  const initials = displayName.slice(0, 2).toUpperCase();

  return (
    <div className="flex h-screen bg-background text-foreground">
      <aside className="flex w-14 shrink-0 flex-col border-r border-sidebar-border bg-sidebar sm:w-56">
        <div className="flex h-13 items-center justify-between border-b border-sidebar-border px-3 sm:px-4">
          <div className="flex items-center gap-2.5">
            <BrandLogo size={24} />
            <span className="hidden text-sm font-semibold tracking-tight sm:inline">SSHield</span>
          </div>
          <span className="hidden rounded-full border border-border bg-secondary px-1.5 py-0.5 font-mono text-[10px] font-medium text-muted-foreground sm:inline-block">
            ENT
          </span>
        </div>

        <nav className="flex flex-1 flex-col gap-5 overflow-y-auto px-2 py-3 sm:px-3">
          {navGroups.map((group) => (
            <div key={group.label} className="flex flex-col gap-1">
              <div className="hidden px-2.5 pb-1 font-mono text-[10px] font-medium uppercase tracking-wider text-muted-foreground/70 sm:block">
                {group.label}
              </div>
              {group.items.map((item) => {
                const isActive = activeTab?.kind === "page" && activeTab.page === item.page;
                const isOpenInTabs = tabs.some(
                  (tb) => tb.kind === "page" && tb.page === item.page
                );

                return (
                  <button
                    key={item.page}
                    type="button"
                    title={item.label}
                    onClick={() => openPage(item.page)}
                    className={cn(
                      "group flex w-full items-center justify-between gap-2 rounded-lg border px-2.5 py-1.5 text-left text-[13px] transition-all",
                      isActive
                        ? "border-border bg-card font-medium text-foreground shadow-2xs dark:bg-accent"
                        : "border-transparent text-muted-foreground hover:bg-sidebar-accent/70 hover:text-foreground"
                    )}
                  >
                    <span className="flex min-w-0 items-center gap-2.5">
                      <item.icon className="size-4 shrink-0" />
                      <span className="hidden truncate sm:inline">{item.label}</span>
                    </span>
                    {isOpenInTabs && !isActive && (
                      <span
                        aria-hidden="true"
                        className="hidden size-1.5 shrink-0 rounded-full bg-muted-foreground/40 sm:inline-block"
                      />
                    )}
                  </button>
                );
              })}
            </div>
          ))}
        </nav>

        <div className="flex flex-col gap-2 border-t border-sidebar-border p-2 sm:p-3">
          <div className="hidden items-center justify-between gap-1 sm:flex">
            <LangToggle compact />
            <ThemeToggle compact />
          </div>

          <div className="flex items-center gap-2 rounded-lg border border-border/80 bg-background/60 p-1.5 sm:px-2.5 sm:py-2">
            <div className="flex size-7 shrink-0 items-center justify-center rounded-full border border-border bg-secondary font-mono text-[11px] font-medium text-foreground">
              {initials}
            </div>
            <div className="hidden min-w-0 flex-1 sm:block">
              <div className="truncate text-xs font-medium text-foreground">{displayName}</div>
              <div className="truncate font-mono text-[10px] text-muted-foreground">
                {t(identity.role === "admin" ? "role.admin" : "role.member")}
              </div>
            </div>
            <Button
              variant="ghost"
              size="icon-xs"
              title={t("nav.signout")}
              onClick={() => void auth.logout()}
              className="hidden shrink-0 text-muted-foreground hover:text-foreground sm:inline-flex"
            >
              <LogOut />
            </Button>
          </div>

          <Button
            variant="ghost"
            size="icon-sm"
            title={t("nav.signout")}
            onClick={() => void auth.logout()}
            className="w-full text-muted-foreground hover:text-foreground sm:hidden"
          >
            <LogOut />
          </Button>
        </div>
      </aside>

      <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <WorkspaceTabBar />
        <WorkspaceView />
      </main>
    </div>
  );
}

export default function App() {
  const auth = useAuth();
  const { t, setLang } = useI18n();

  useEffect(() => {
    applyTheme(getSavedThemeId());
  }, []);

  // Server-side preference sync: theme and language persisted per user in
  // the settings KV win over this device's local values after login.
  useEffect(() => {
    if (!auth.identity) return;
    void (async () => {
      try {
        const theme = await api.get<{ value: string }>("/api/settings/ui.theme");
        if (theme.value && theme.value !== getSavedThemeId()) {
          applyTheme(theme.value);
        }
      } catch {
        /* preference sync is best-effort */
      }
      try {
        const lang = await api.get<{ value: string }>("/api/settings/ui.language");
        if (lang.value === "en" || lang.value === "zh-CN") {
          setLang(lang.value);
        }
      } catch {
        /* preference sync is best-effort */
      }
    })();
  }, [auth.identity, setLang]);

  if (auth.loading) {
    return (
      <div className="flex h-screen items-center justify-center bg-background font-mono text-xs text-muted-foreground">
        {t("common.loading")}
      </div>
    );
  }

  if (!auth.identity) {
    return (
      <TooltipProvider>
        <Routes>
          <Route path="/" element={<Landing />} />
          <Route path="/welcome" element={<Landing />} />
          <Route path="/login" element={<Login />} />
          <Route path="*" element={<Navigate to="/login" replace />} />
        </Routes>
      </TooltipProvider>
    );
  }

  return (
    <TooltipProvider>
      <WorkspaceTabsProvider>
        <AuthenticatedShell />
      </WorkspaceTabsProvider>
    </TooltipProvider>
  );
}
