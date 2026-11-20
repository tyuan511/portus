import { useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAuth } from "@/lib/auth";
import type { Permission } from "@/types";
import AdminUsers from "@/pages/admin/Users";
import AdminRoles from "@/pages/admin/Roles";
import AdminGroups from "@/pages/admin/Groups";
import AdminAccess from "@/pages/admin/Access";
import AdminSessions from "@/pages/admin/Sessions";

// Consolidated admin console: the five management surfaces collapse into a
// single sidebar entry with tabs. Each tab is gated by its own permission.
const TABS: { id: string; label: string; permission: Permission; element: React.ReactNode }[] = [
  { id: "users", label: "Users", permission: "users.manage", element: <AdminUsers /> },
  { id: "roles", label: "Roles", permission: "roles.manage", element: <AdminRoles /> },
  { id: "groups", label: "Groups", permission: "groups.manage", element: <AdminGroups /> },
  { id: "access", label: "Host Access", permission: "access.manage", element: <AdminAccess /> },
  { id: "sessions", label: "Sessions", permission: "users.manage", element: <AdminSessions /> },
];

export default function AdminConsole() {
  const auth = useAuth();
  const visible = TABS.filter((t) => auth.hasPermission(t.permission));
  const [active, setActive] = useState(visible[0]?.id ?? "users");

  if (visible.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        No administration permissions assigned.
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="border-b px-4 py-3">
        <h1 className="text-base font-semibold">Administration</h1>
        <p className="text-xs text-muted-foreground">Users, roles, groups, host access and live sessions</p>
      </div>
      <Tabs value={active} onValueChange={setActive} className="flex min-h-0 flex-1 flex-col">
        <TabsList className="mx-4 mt-3 w-fit">
          {visible.map((t) => (
            <TabsTrigger key={t.id} value={t.id}>{t.label}</TabsTrigger>
          ))}
        </TabsList>
        {visible.map((t) => (
          <TabsContent key={t.id} value={t.id} className="min-h-0 flex-1 overflow-y-auto p-4">
            {t.element}
          </TabsContent>
        ))}
      </Tabs>
    </div>
  );
}
