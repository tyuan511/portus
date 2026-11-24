import { useEffect, useMemo, useRef } from "react";
import {
  Activity,
  ArrowLeftToLine,
  ArrowRightToLine,
  CopyPlus,
  FolderOpen,
  Pin,
  PinOff,
  RotateCw,
  ScrollText,
  Server,
  Settings,
  Shield,
  Terminal,
  Users,
  X,
  XCircle,
} from "lucide-react";
import { cn } from "cn";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuGroup,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { useI18n, type MessageKey } from "@/lib/i18n";
import { useWorkspaceTabs, type PageKey, type TerminalConnStatus } from "@/lib/workspace-tabs";

const PAGE_META: Record<PageKey, { labelKey: MessageKey; icon: typeof Server }> = {
  hosts: { labelKey: "nav.hosts", icon: Server },
  approvals: { labelKey: "nav.approvals", icon: Activity },
  audit: { labelKey: "nav.audit", icon: ScrollText },
  security: { labelKey: "nav.security", icon: Shield },
  admin: { labelKey: "nav.admin", icon: Users },
  settings: { labelKey: "nav.settings", icon: Settings },
};

function TerminalStatusDot({ status }: { status: TerminalConnStatus }) {
  return (
    <span className="relative flex size-1.5 shrink-0">
      {status === "connecting" && (
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-geist-warning opacity-75" />
      )}
      <span
        className={cn(
          "relative inline-flex size-1.5 rounded-full",
          status === "connected" && "bg-geist-success",
          status === "connecting" && "bg-geist-warning",
          status === "disconnected" && "bg-geist-error"
        )}
      />
    </span>
  );
}

export function WorkspaceTabBar() {
  const { t } = useI18n();
  const {
    tabs,
    activeTabId,
    setActiveTab,
    duplicateTerminal,
    reconnectTerminal,
    togglePinTab,
    closeTab,
    closeOtherTabs,
    closeTabsToLeft,
    closeTabsToRight,
    closeAllTabs,
  } = useWorkspaceTabs();

  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});

  useEffect(() => {
    if (!activeTabId) return;
    tabRefs.current[activeTabId]?.scrollIntoView({
      behavior: "smooth",
      block: "nearest",
      inline: "nearest",
    });
  }, [activeTabId]);

  // Compute ordinal suffix when multiple terminal tabs connect to the same server.
  const terminalOrdinals = useMemo(() => {
    const counts = new Map<string, number>();
    const totals = new Map<string, number>();
    for (const tab of tabs) {
      if (tab.kind === "terminal") {
        totals.set(tab.serverId, (totals.get(tab.serverId) ?? 0) + 1);
      }
    }
    const result = new Map<string, string>();
    for (const tab of tabs) {
      if (tab.kind === "terminal") {
        const next = (counts.get(tab.serverId) ?? 0) + 1;
        counts.set(tab.serverId, next);
        if ((totals.get(tab.serverId) ?? 0) > 1) {
          result.set(tab.id, `#${next}`);
        }
      }
    }
    return result;
  }, [tabs]);

  const sshCount = useMemo(
    () => tabs.filter((tab) => tab.kind === "terminal").length,
    [tabs]
  );

  const hasClosable = tabs.some((tb) => !tb.pinned);

  return (
    <div className="flex h-13 shrink-0 items-center justify-between gap-2 border-b border-border bg-sidebar/70 px-2 sm:px-3">
      {/* Scrollable Tabs Strip */}
      <div
        role="tablist"
        aria-label="Workspace tabs"
        className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto py-1.5"
      >
        {tabs.map((tab, idx) => {
          const isActive = tab.id === activeTabId;
          const Icon =
            tab.kind === "page"
              ? PAGE_META[tab.page].icon
              : tab.kind === "files"
                ? FolderOpen
                : Terminal;
          const label =
            tab.kind === "page"
              ? t(PAGE_META[tab.page].labelKey)
              : tab.kind === "files"
                ? `${tab.title} · ${t("files.title")}`
                : tab.title;
          const ordinal = tab.kind === "terminal" ? terminalOrdinals.get(tab.id) : undefined;

          const hasOtherClosable = tabs.some((other) => other.id !== tab.id && !other.pinned);
          const hasLeftClosable = tabs.slice(0, idx).some((other) => !other.pinned);
          const hasRightClosable = tabs.slice(idx + 1).some((other) => !other.pinned);

          return (
            <ContextMenu key={tab.id}>
              <ContextMenuTrigger
                render={
                  <div
                    className={cn(
                      "group relative flex h-8 max-w-52 shrink-0 items-center rounded-lg border transition-all",
                      isActive
                        ? "border-border bg-card text-foreground shadow-2xs dark:bg-accent"
                        : "border-transparent text-muted-foreground hover:border-border/60 hover:bg-background/60 hover:text-foreground"
                    )}
                  />
                }
              >
                <button
                  ref={(el) => {
                    tabRefs.current[tab.id] = el;
                  }}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  title={tab.kind === "terminal" && tab.target ? `${label} (${tab.target})` : label}
                  onClick={() => setActiveTab(tab.id)}
                  onAuxClick={(e) => {
                    if (e.button === 1 && !tab.pinned) {
                      e.preventDefault();
                      closeTab(tab.id);
                    }
                  }}
                  className="flex h-full min-w-0 flex-1 items-center gap-1.5 pr-1.5 pl-2.5 text-xs font-medium outline-none"
                >
                  {tab.kind === "terminal" ? (
                    <TerminalStatusDot status={tab.status} />
                  ) : (
                    <Icon className="size-3.5 shrink-0" />
                  )}
                  <span className="truncate">{label}</span>
                  {ordinal && (
                    <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                      {ordinal}
                    </span>
                  )}
                </button>

                {tab.pinned ? (
                  <button
                    type="button"
                    aria-label={t("tabs.unpin")}
                    title={t("tabs.unpin")}
                    onClick={(e) => {
                      e.stopPropagation();
                      togglePinTab(tab.id);
                    }}
                    className="mr-1 flex size-5 shrink-0 items-center justify-center rounded-md text-muted-foreground/80 transition-colors hover:bg-muted hover:text-foreground"
                  >
                    <Pin className="size-3" />
                  </button>
                ) : (
                  <button
                    type="button"
                    aria-label={t("tabs.close")}
                    title={t("tabs.close")}
                    onClick={(e) => {
                      e.stopPropagation();
                      closeTab(tab.id);
                    }}
                    className={cn(
                      "mr-1 flex size-5 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-opacity hover:bg-muted hover:text-foreground",
                      isActive ? "opacity-80 hover:opacity-100" : "opacity-0 group-hover:opacity-100"
                    )}
                  >
                    <X className="size-3" />
                  </button>
                )}
              </ContextMenuTrigger>

              <ContextMenuContent className="w-52">
                {tab.kind === "terminal" && (
                  <>
                    <ContextMenuGroup>
                      <ContextMenuItem onClick={() => duplicateTerminal(tab.id)}>
                        <CopyPlus />
                        <span>{t("terminal.duplicate")}</span>
                      </ContextMenuItem>
                      <ContextMenuItem
                        disabled={tab.status !== "disconnected"}
                        onClick={() => reconnectTerminal(tab.id)}
                      >
                        <RotateCw />
                        <span>{t("terminal.reconnect")}</span>
                      </ContextMenuItem>
                    </ContextMenuGroup>
                    <ContextMenuSeparator />
                  </>
                )}

                <ContextMenuGroup>
                  <ContextMenuItem onClick={() => togglePinTab(tab.id)}>
                    {tab.pinned ? <PinOff /> : <Pin />}
                    <span>{tab.pinned ? t("tabs.unpin") : t("tabs.pin")}</span>
                  </ContextMenuItem>
                </ContextMenuGroup>

                <ContextMenuSeparator />

                <ContextMenuGroup>
                  <ContextMenuItem onClick={() => closeTab(tab.id)}>
                    <X />
                    <span>{t("tabs.close")}</span>
                  </ContextMenuItem>
                  <ContextMenuItem
                    disabled={!hasOtherClosable}
                    onClick={() => closeOtherTabs(tab.id)}
                  >
                    <XCircle />
                    <span>{t("tabs.closeOthers")}</span>
                  </ContextMenuItem>
                  <ContextMenuItem
                    disabled={!hasLeftClosable}
                    onClick={() => closeTabsToLeft(tab.id)}
                  >
                    <ArrowLeftToLine />
                    <span>{t("tabs.closeLeft")}</span>
                  </ContextMenuItem>
                  <ContextMenuItem
                    disabled={!hasRightClosable}
                    onClick={() => closeTabsToRight(tab.id)}
                  >
                    <ArrowRightToLine />
                    <span>{t("tabs.closeRight")}</span>
                  </ContextMenuItem>
                  <ContextMenuItem
                    variant="destructive"
                    disabled={!hasClosable}
                    onClick={closeAllTabs}
                  >
                    <XCircle />
                    <span>{t("tabs.closeAll")}</span>
                  </ContextMenuItem>
                </ContextMenuGroup>
              </ContextMenuContent>
            </ContextMenu>
          );
        })}
      </div>

      {sshCount > 0 && (
        <div className="flex shrink-0 items-center pl-1">
          <span className="hidden items-center gap-1.5 rounded-full border border-border bg-card px-2 py-0.5 font-mono text-[11px] text-muted-foreground sm:inline-flex">
            <span className="size-1.5 rounded-full bg-geist-success" />
            {t("tabs.sshCount", { count: sshCount })}
          </span>
        </div>
      )}
    </div>
  );
}
