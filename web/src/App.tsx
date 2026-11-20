import { useEffect } from "react";
import { Navigate, NavLink, Route, Routes, useLocation } from "react-router-dom";
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
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { applyTheme, getSavedThemeId } from "@/lib/themes";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { BrandLogo } from "@/components/BrandLogo";
import { LangToggle } from "@/components/sshield/LangToggle";
import { ThemeToggle } from "@/components/sshield/ThemeToggle";
import Login from "@/pages/Login";
import Landing from "@/pages/Landing";
import Hosts from "@/pages/Hosts";
import Audit from "@/pages/Audit";
import SecurityView from "@/pages/Security";
import Approvals from "@/pages/Approvals";
import SettingsView from "@/pages/Settings";
import AdminConsole from "@/pages/AdminConsole";
import TerminalPane from "@/pages/TerminalPane";

export default function App() {
  const auth = useAuth();
  const { t } = useI18n();
  const location = useLocation();

  useEffect(() => {
    applyTheme(getSavedThemeId());
  }, []);

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

  const navGroups: {
    label: string;
    items: { to: string; label: string; icon: typeof Server; show: boolean }[];
  }[] = [
    {
      label: t("nav.group.workspace"),
      items: [
        { to: "/", label: t("nav.hosts"), icon: Server, show: auth.hasPermission("host.read") },
        { to: "/approvals", label: t("nav.approvals"), icon: Activity, show: auth.hasPermission("approvals.decide") },
      ],
    },
    {
      label: t("nav.group.governance"),
      items: [
        {
          to: "/audit",
          label: t("nav.audit"),
          icon: ScrollText,
          show: auth.hasPermission("audit.view_self") || auth.hasPermission("audit.view_all"),
        },
        {
          to: "/security",
          label: t("nav.security"),
          icon: Shield,
          show: auth.hasPermission("rules.read") || auth.hasPermission("rules.manage") || auth.hasPermission("snippets.use"),
        },
      ],
    },
    {
      label: t("nav.group.system"),
      items: [
        {
          to: "/admin",
          label: t("nav.admin"),
          icon: Users,
          show: auth.hasPermission("users.manage") || auth.hasPermission("access.manage"),
        },
        { to: "/settings", label: t("nav.settings"), icon: Settings, show: true },
      ],
    },
  ]
    .map((g) => ({ ...g, items: g.items.filter((i) => i.show) }))
    .filter((g) => g.items.length > 0);

  const isTerminalRoute = location.pathname.startsWith("/terminal/");
  const displayName = auth.identity.display_name || auth.identity.username;
  const initials = displayName.slice(0, 2).toUpperCase();

  return (
    <TooltipProvider>
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
                {group.items.map((item) => (
                  <NavLink
                    key={item.to}
                    to={item.to}
                    end={item.to === "/"}
                    title={item.label}
                    className={({ isActive }) =>
                      cn(
                        "group flex items-center gap-2.5 rounded-lg border px-2.5 py-1.5 text-[13px] transition-all",
                        isActive
                          ? "border-border bg-card font-medium text-foreground shadow-2xs dark:bg-accent"
                          : "border-transparent text-muted-foreground hover:bg-sidebar-accent/70 hover:text-foreground"
                      )
                    }
                  >
                    <item.icon className="size-4 shrink-0" />
                    <span className="hidden truncate sm:inline">{item.label}</span>
                  </NavLink>
                ))}
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
                  {t(auth.identity.role === "admin" ? "role.admin" : "role.member")}
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
          <Routes>
            <Route path="/login" element={<Navigate to="/" replace />} />
            <Route path="/welcome" element={<Navigate to="/" replace />} />
            <Route path="/" element={<Hosts />} />
            <Route path="/terminal/:serverId" element={isTerminalRoute ? <TerminalPane /> : null} />
            <Route path="/audit" element={<Audit />} />
            <Route path="/security" element={<SecurityView />} />
            <Route path="/approvals" element={<Approvals />} />
            <Route path="/settings" element={<SettingsView />} />
            <Route path="/admin" element={<AdminConsole />} />
            <Route path="/admin/*" element={<Navigate to="/admin" replace />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </main>
      </div>
    </TooltipProvider>
  );
}
