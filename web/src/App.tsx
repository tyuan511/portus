import { useEffect } from "react";
import { Navigate, NavLink, Route, Routes, useLocation } from "react-router-dom";
import {
  Activity,
  FolderTree,
  KeyRound,
  Lock,
  LogOut,
  ScrollText,
  Server,
  Settings,
  Shield,
  Users,
  Wrench,
} from "lucide-react";
import { useAuth } from "@/lib/auth";
import { applyTheme, getSavedThemeId } from "@/lib/themes";
import { Button } from "@/components/ui/button";
import Login from "@/pages/Login";
import Landing from "@/pages/Landing";
import Hosts from "@/pages/Hosts";
import Audit from "@/pages/Audit";
import Snippets from "@/pages/Snippets";
import SecurityView from "@/pages/Security";
import VaultView from "@/pages/Vault";
import Approvals from "@/pages/Approvals";
import SettingsView from "@/pages/Settings";
import AdminUsers from "@/pages/admin/Users";
import AdminRoles from "@/pages/admin/Roles";
import AdminGroups from "@/pages/admin/Groups";
import AdminAccess from "@/pages/admin/Access";
import AdminSessions from "@/pages/admin/Sessions";
import TerminalPane from "@/pages/TerminalPane";

export default function App() {
  const auth = useAuth();
  const location = useLocation();

  useEffect(() => {
    applyTheme(getSavedThemeId());
  }, []);

  if (auth.loading) {
    return (
      <div className="flex h-screen items-center justify-center bg-background text-muted-foreground">
        Loading SSHield…
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

  const nav: { to: string; label: string; icon: typeof Server; show: boolean }[] = [
    { to: "/", label: "Hosts", icon: Server, show: auth.hasPermission("host.read") },
    { to: "/approvals", label: "Approvals", icon: Activity, show: auth.hasPermission("approvals.decide") },
    { to: "/audit", label: "Audit Logs", icon: ScrollText, show: auth.hasPermission("audit.view_self") || auth.hasPermission("audit.view_all") },
    { to: "/snippets", label: "Snippets", icon: FolderTree, show: auth.hasPermission("snippets.use") },
    { to: "/security", label: "Security", icon: Shield, show: auth.hasPermission("rules.read") || auth.hasPermission("rules.manage") },
    { to: "/vault", label: "Vault", icon: Lock, show: auth.hasPermission("vault.manage") },
    { to: "/admin/users", label: "Users", icon: Users, show: auth.hasPermission("users.manage") },
    { to: "/admin/roles", label: "Roles", icon: Wrench, show: auth.hasPermission("roles.manage") },
    { to: "/admin/groups", label: "Groups", icon: FolderTree, show: auth.hasPermission("groups.manage") },
    { to: "/admin/access", label: "Access", icon: KeyRound, show: auth.hasPermission("access.manage") },
    { to: "/admin/sessions", label: "Sessions", icon: Activity, show: auth.hasPermission("users.manage") },
    { to: "/settings", label: "Settings", icon: Settings, show: true },
  ].filter((n) => n.show);

  const isTerminalRoute = location.pathname.startsWith("/terminal/");

  return (
    <div className="flex h-screen bg-background text-foreground">
      <aside className="flex w-14 flex-col border-r bg-sidebar sm:w-44">
        <div className="flex h-12 items-center gap-2 border-b px-3">
          <div className="flex size-7 items-center justify-center rounded-lg bg-primary font-bold text-primary-foreground">S</div>
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
            <span className="ml-1 rounded bg-muted px-1 py-0.5 text-[10px]">{auth.identity.role_name}</span>
          </div>
          <Button variant="ghost" size="sm" className="w-full justify-start" onClick={() => void auth.logout()}>
            <LogOut className="size-4" />
            <span className="hidden sm:inline">Sign out</span>
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
          <Route path="/snippets" element={<Snippets />} />
          <Route path="/security" element={<SecurityView />} />
          <Route path="/vault" element={<VaultView />} />
          <Route path="/approvals" element={<Approvals />} />
          <Route path="/settings" element={<SettingsView />} />
          <Route path="/admin/users" element={<AdminUsers />} />
          <Route path="/admin/roles" element={<AdminRoles />} />
          <Route path="/admin/groups" element={<AdminGroups />} />
          <Route path="/admin/access" element={<AdminAccess />} />
          <Route path="/admin/sessions" element={<AdminSessions />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </main>
    </div>
  );
}
