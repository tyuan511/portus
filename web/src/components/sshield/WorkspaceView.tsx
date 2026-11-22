import { Activity, FolderOpen, Layers, ScrollText, Server, Settings, Shield, Users } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { useI18n, type MessageKey } from "@/lib/i18n";
import { useWorkspaceTabs, type PageKey } from "@/lib/workspace-tabs";
import Hosts from "@/pages/Hosts";
import Approvals from "@/pages/Approvals";
import Audit from "@/pages/Audit";
import SecurityView from "@/pages/Security";
import AdminConsole from "@/pages/AdminConsole";
import SettingsView from "@/pages/Settings";
import TerminalPane from "@/pages/TerminalPane";
import FilesManager from "@/pages/FilesManager";

const QUICK_LAUNCH: { page: PageKey; labelKey: MessageKey; icon: typeof Server }[] = [
  { page: "hosts", labelKey: "nav.hosts", icon: Server },
  { page: "approvals", labelKey: "nav.approvals", icon: Activity },
  { page: "audit", labelKey: "nav.audit", icon: ScrollText },
  { page: "security", labelKey: "nav.security", icon: Shield },
  { page: "admin", labelKey: "nav.admin", icon: Users },
  { page: "settings", labelKey: "nav.settings", icon: Settings },
];

function renderPage(page: PageKey) {
  switch (page) {
    case "hosts":
      return <Hosts />;
    case "approvals":
      return <Approvals />;
    case "audit":
      return <Audit />;
    case "security":
      return <SecurityView />;
    case "admin":
      return <AdminConsole />;
    case "settings":
      return <SettingsView />;
  }
}

export function WorkspaceView() {
  const { t } = useI18n();
  const {
    tabs,
    activeTabId,
    openPage,
    duplicateTerminal,
    updateTerminalStatus,
    canAccessPage,
  } = useWorkspaceTabs();

  if (tabs.length === 0) {
    const allowed = QUICK_LAUNCH.filter((item) => canAccessPage(item.page));
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center p-6">
        <Empty className="max-w-lg border border-border bg-card py-12">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Layers />
            </EmptyMedia>
            <EmptyTitle>{t("tabs.emptyTitle")}</EmptyTitle>
            <EmptyDescription>{t("tabs.emptyDesc")}</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <div className="flex flex-wrap justify-center gap-2">
              {allowed.map((item) => (
                <Button
                  key={item.page}
                  variant="outline"
                  size="sm"
                  onClick={() => openPage(item.page)}
                >
                  <item.icon data-icon="inline-start" />
                  {t(item.labelKey)}
                </Button>
              ))}
            </div>
          </EmptyContent>
        </Empty>
      </div>
    );
  }

  return (
    <div className="relative min-h-0 flex-1 overflow-hidden">
      {tabs.map((tab) => {
        const isActive = tab.id === activeTabId;
        return (
          <div
            key={tab.id}
            role="tabpanel"
            aria-hidden={!isActive}
            className={isActive ? "flex h-full min-h-0 flex-col" : "hidden"}
          >
            {tab.kind === "page" ? (
              renderPage(tab.page)
            ) : tab.kind === "files" ? (
              <FilesManager serverId={tab.serverId} serverName={tab.title} />
            ) : (
              <TerminalPane
                serverId={tab.serverId}
                serverName={tab.title}
                serverTarget={tab.target}
                isActive={isActive}
                reconnectSeq={tab.reconnectSeq}
                onStatusChange={(status) => updateTerminalStatus(tab.id, status)}
                onDuplicate={() => duplicateTerminal(tab.id)}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}
