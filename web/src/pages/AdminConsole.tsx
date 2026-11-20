import { useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAuth } from "@/lib/auth";
import { useI18n, type MessageKey } from "@/lib/i18n";
import type { Permission } from "@/types";
import AdminUsers from "@/pages/admin/Users";
import AdminAccess from "@/pages/admin/Access";
import AdminSessions from "@/pages/admin/Sessions";

// Consolidated admin console: users, access (user groups × machine groups)
// and sessions. Roles are built in — there is nothing configurable beyond
// assigning each user the admin or member role.
const TABS: { id: string; labelKey: MessageKey; permission: Permission; element: React.ReactNode }[] = [
  { id: "users", labelKey: "admin.users", permission: "users.manage", element: <AdminUsers /> },
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
        {t("admin.noPerms")}
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-12 shrink-0 items-center border-b px-4">
        <h1 className="text-base font-semibold">{t("admin.title")}</h1>
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
