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
import { useAuth } from "@/lib/auth";
import { useI18n } from "@/lib/i18n";
import { applyTheme, getSavedThemeId } from "@/lib/themes";
import { Button } from "@/components/ui/button";
import { BrandLogo } from "@/components/BrandLogo";
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
      <div className="flex h-screen items-center justify-center bg-background text-muted-foreground">
        {t("common.loading")}
      </div>
    );
  }
  if (!auth.identity) {
    // Anonymous visitors get the public landing page; everything else funnels
    // through sign-in.
    return (
      <Routes>
        <Route path="/" element={<Landing />} />
        <Route path="/welcome" element={<Landing />} />
        <Route path="/login" element={<Login />} />
        <Route path="*" element={<Navigate to="/login" replace />} />
      </Routes>
    );
  }

  // Six entries max — consolidated surfaces keep the sidebar scannable.
  const nav: { to: string; label: string; icon: typeof Server; show: boolean }[] = [
    { to: "/", label: t("nav.hosts"), icon: Server, show: auth.hasPermission("host.read") },
    { to: "/approvals", label: t("nav.approvals"), icon: Activity, show: auth.hasPermission("approvals.decide") },
    { to: "/audit", label: t("nav.audit"), icon: ScrollText, show: auth.hasPermission("audit.view_self") || auth.hasPermission("audit.view_all") },
    {
      to: "/security",
      label: t("nav.security"),
      icon: Shield,
      show: auth.hasPermission("rules.read") || auth.hasPermission("rules.manage") || auth.hasPermission("snippets.use"),
    },
    {
      to: "/admin",
      label: t("nav.admin"),
      icon: Users,
      show: auth.hasPermission("users.manage") || auth.hasPermission("access.manage"),
    },
    { to: "/settings", label: t("nav.settings"), icon: Settings, show: true },
  ].filter((n) => n.show);

  const isTerminalRoute = location.pathname.startsWith("/terminal/");

  return (
    <div className="flex h-screen bg-background text-foreground">
      <aside className="flex w-14 flex-col border-r bg-sidebar sm:w-44">
        <div className="flex h-12 items-center gap-2 border-b px-3">
          <BrandLogo size={24} />
          <span className="hidden font-semibold sm:inline">SSHield</span>
        </div>
        <nav className="flex-1 space-y-0.5 overflow-y-auto p-1.5">
          {nav.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) =>
                `flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-sm ${
                  isActive ? "bg-primary/10 font-medium text-primary" : "text-muted-foreground hover:bg-muted hover:text-foreground"
                }`
              }
            >
              <item.icon className="size-4 shrink-0" />
              <span className="hidden sm:inline">{item.label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="border-t p-1.5">
          <div className="hidden px-2.5 pb-1 text-xs text-muted-foreground sm:block">
            {auth.identity.display_name || auth.identity.username}
            <span className="ml-1 rounded bg-muted px-1 py-0.5 text-[10px]">{t(auth.identity.role === "admin" ? "role.admin" : "role.member")}</span>
          </div>
          <Button variant="ghost" size="sm" className="w-full justify-start" onClick={() => void auth.logout()}>
            <LogOut className="size-4" />
            <span className="hidden sm:inline">{t("nav.signout")}</span>
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
  );
}
