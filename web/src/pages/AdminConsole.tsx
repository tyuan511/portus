import { useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAuth } from "@/lib/auth";
import { useI18n, type MessageKey } from "@/lib/i18n";
import type { Permission } from "@/types";
import AdminUsers from "@/pages/admin/Users";
import AdminRoles from "@/pages/admin/Roles";
import AdminGroups from "@/pages/admin/Groups";
import AdminAccess from "@/pages/admin/Access";
import AdminSessions from "@/pages/admin/Sessions";

// Consolidated admin console: the five management surfaces collapse into a
// single sidebar entry with tabs. Each tab is gated by its own permission.
const TABS: { id: string; labelKey: MessageKey; permission: Permission; element: React.ReactNode }[] = [
  { id: "users", labelKey: "admin.users", permission: "users.manage", element: <AdminUsers /> },
  { id: "roles", labelKey: "admin.roles", permission: "roles.manage", element: <AdminRoles /> },
  { id: "groups", labelKey: "admin.groups", permission: "groups.manage", element: <AdminGroups /> },
  { id: "access", labelKey: "admin.access", permission: "access.manage", element: <AdminAccess /> },
  { id: "sessions", labelKey: "admin.sessions", permission: "users.manage", element: <AdminSessions /> },
];

export default function AdminConsole() {
  const auth = useAuth();
  const { t } = useI18n();
  const visible = TABS.filter((tb) => auth.hasPermission(tb.permission));
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
        <h1 className="text-base font-semibold">{t("admin.title")}</h1>
        <p className="text-xs text-muted-foreground">{t("admin.subtitle")}</p>
      </div>
      <Tabs value={active} onValueChange={setActive} className="flex min-h-0 flex-1 flex-col">
        <TabsList className="mx-4 mt-3 w-fit">
          {visible.map((tb) => (
            <TabsTrigger key={tb.id} value={tb.id}>{t(tb.labelKey)}</TabsTrigger>
          ))}
        </TabsList>
        {visible.map((tb) => (
          <TabsContent key={tb.id} value={tb.id} className="min-h-0 flex-1 overflow-y-auto p-4">
            {tb.element}
          </TabsContent>
        ))}
      </Tabs>
    </div>
  );
}
