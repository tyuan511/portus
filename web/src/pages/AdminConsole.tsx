import { useState } from "react";
import { ShieldAlert } from "lucide-react";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Page, PageHeader } from "@/components/ui/shared";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAuth } from "@/lib/auth";
import { useI18n, type MessageKey } from "@/lib/i18n";
import type { Permission } from "@/types";
import AdminUsers from "@/pages/admin/Users";
import AdminUserGroups from "@/pages/admin/UserGroups";
import AdminAccess from "@/pages/admin/Access";
import AdminSessions from "@/pages/admin/Sessions";

const TABS: { id: string; labelKey: MessageKey; permission: Permission; element: React.ReactNode }[] = [
  { id: "users", labelKey: "admin.users", permission: "users.manage", element: <AdminUsers /> },
  { id: "groups", labelKey: "admin.userGroups", permission: "access.manage", element: <AdminUserGroups /> },
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
      <Page header={<PageHeader title={t("admin.title")} />}>
        <Empty className="border border-border bg-card py-16">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <ShieldAlert />
            </EmptyMedia>
            <EmptyTitle>{t("admin.title")}</EmptyTitle>
            <EmptyDescription>{t("admin.noPerms")}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      </Page>
    );
  }

  return (
    <Page header={<PageHeader title={t("admin.title")} />}>
      <Tabs value={active} onValueChange={setActive} className="gap-5">
        <TabsList variant="line" className="w-full justify-start border-b border-border pb-0">
          {visible.map((tb) => (
            <TabsTrigger key={tb.id} value={tb.id}>
              {t(tb.labelKey)}
            </TabsTrigger>
          ))}
        </TabsList>
        {visible.map((tb) => (
          <TabsContent key={tb.id} value={tb.id}>
            {tb.element}
          </TabsContent>
        ))}
      </Tabs>
    </Page>
  );
}
