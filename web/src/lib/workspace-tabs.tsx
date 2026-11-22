import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { api } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import type { ServerAsset } from "@/types";

export type PageKey = "hosts" | "approvals" | "audit" | "security" | "admin" | "settings";
export type TerminalConnStatus = "connecting" | "connected" | "disconnected";

export type WorkspaceTab =
  | {
      id: string;
      kind: "page";
      page: PageKey;
      pinned?: boolean;
    }
  | {
      id: string;
      kind: "terminal";
      serverId: string;
      title: string;
      target?: string;
      status: TerminalConnStatus;
      pinned?: boolean;
      reconnectSeq?: number;
    }
  | {
      id: string;
      kind: "files";
      serverId: string;
      title: string;
      pinned?: boolean;
    };

export const PAGE_ROUTES: Record<PageKey, string> = {
  hosts: "/",
  approvals: "/approvals",
  audit: "/audit",
  security: "/security",
  admin: "/admin",
  settings: "/settings",
};

function routeToInitialTabs(pathname: string): { tabs: WorkspaceTab[]; activeId: string } {
  const termMatch = pathname.match(/^\/terminal\/([^/]+)/);
  if (termMatch) {
    const serverId = decodeURIComponent(termMatch[1]);
    const termId = `term:${serverId}:1`;
    return {
      tabs: [
        { id: "page:hosts", kind: "page", page: "hosts" },
        { id: termId, kind: "terminal", serverId, title: serverId, status: "connecting" },
      ],
      activeId: termId,
    };
  }

  const filesMatch = pathname.match(/^\/files\/([^/]+)/);
  if (filesMatch) {
    const serverId = decodeURIComponent(filesMatch[1]);
    const filesId = `files:${serverId}`;
    return {
      tabs: [
        { id: "page:hosts", kind: "page", page: "hosts" },
        { id: filesId, kind: "files", serverId, title: serverId },
      ],
      activeId: filesId,
    };
  }

  const matchedPage = (Object.entries(PAGE_ROUTES) as [PageKey, string][]).find(
    ([key, route]) => key !== "hosts" && (pathname === route || pathname.startsWith(`${route}/`))
  )?.[0];

  const initialPage: PageKey = matchedPage ?? "hosts";
  const pageId = `page:${initialPage}`;
  return {
    tabs: [{ id: pageId, kind: "page", page: initialPage }],
    activeId: pageId,
  };
}

function fallbackActiveId(nextTabs: WorkspaceTab[], currentActiveId: string | null, preferredIdx: number): string | null {
  if (nextTabs.length === 0) return null;
  if (currentActiveId && nextTabs.some((t) => t.id === currentActiveId)) {
    return currentActiveId;
  }
  const clamped = Math.min(Math.max(preferredIdx, 0), nextTabs.length - 1);
  return nextTabs[clamped].id;
}

interface WorkspaceTabsContextValue {
  tabs: WorkspaceTab[];
  activeTabId: string | null;
  activeTab: WorkspaceTab | null;
  openPage: (page: PageKey) => void;
  openTerminal: (server: Pick<ServerAsset, "id" | "name" | "username" | "host" | "port">) => void;
  openFiles: (server: Pick<ServerAsset, "id" | "name">) => void;
  duplicateTerminal: (tabId: string) => void;
  reconnectTerminal: (tabId: string) => void;
  togglePinTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
  closeTab: (tabId: string) => void;
  closeOtherTabs: (tabId: string) => void;
  closeTabsToLeft: (tabId: string) => void;
  closeTabsToRight: (tabId: string) => void;
  closeAllTabs: () => void;
  updateTerminalStatus: (tabId: string, status: TerminalConnStatus) => void;
  canAccessPage: (page: PageKey) => boolean;
}

const WorkspaceTabsContext = createContext<WorkspaceTabsContextValue | null>(null);

export function WorkspaceTabsProvider({ children }: { children: ReactNode }) {
  const auth = useAuth();
  const seqRef = useRef(2);

  const canAccessPage = useCallback(
    (page: PageKey): boolean => {
      switch (page) {
        case "hosts":
          return auth.hasPermission("host.read");
        case "approvals":
          return auth.hasPermission("approvals.decide");
        case "audit":
          return auth.hasPermission("audit.view_self") || auth.hasPermission("audit.view_all");
        case "security":
          return (
            auth.hasPermission("rules.read") ||
            auth.hasPermission("rules.manage") ||
            auth.hasPermission("snippets.use")
          );
        case "admin":
          return auth.hasPermission("users.manage") || auth.hasPermission("access.manage");
        case "settings":
          return true;
      }
    },
    [auth]
  );

  const initial = useMemo(() => {
    const parsed = routeToInitialTabs(window.location.pathname);
    const firstPermitted = parsed.tabs.filter(
      (tab) => tab.kind !== "page" || canAccessPage(tab.page)
    );
    if (firstPermitted.length > 0) {
      const hasActive = firstPermitted.some((t) => t.id === parsed.activeId);
      return {
        tabs: firstPermitted,
        activeId: hasActive ? parsed.activeId : firstPermitted[0].id,
      };
    }
    return {
      tabs: [{ id: "page:settings", kind: "page" as const, page: "settings" as const }],
      activeId: "page:settings",
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const [tabs, setTabs] = useState<WorkspaceTab[]>(initial.tabs);
  const [activeTabId, setActiveTabId] = useState<string | null>(initial.activeId);

  const activeTab = useMemo(
    () => tabs.find((t) => t.id === activeTabId) ?? null,
    [tabs, activeTabId]
  );

  // Persist open page tabs per user (settings KV) so a reload restores the
  // workspace. Terminal/files tabs are session-scoped and not restored.
  const restoreLoaded = useRef(false);
  useEffect(() => {
    if (restoreLoaded.current) return;
    restoreLoaded.current = true;
    void api
      .get<{ value: string }>("/api/settings/ui.workspace_tabs")
      .then((res) => {
        const ids = (JSON.parse(res.value) as string[]).filter((id) => id.startsWith("page:"));
        if (ids.length === 0) return;
        const pages = ids
          .map((id) => id.slice("page:".length) as PageKey)
          .filter((page, i, arr) => arr.indexOf(page) === i)
          .filter((page) => canAccessPage(page));
        if (pages.length === 0) return;
        setTabs((prev) => {
          const restored = pages
            .filter((page) => !prev.some((t) => t.id === `page:${page}`))
            .map((page) => ({ id: `page:${page}`, kind: "page" as const, page }));
          return [...prev, ...restored];
        });
      })
      .catch(() => {
        /* first login or missing key: nothing to restore */
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const pageIds = tabs.filter((t) => t.kind === "page").map((t) => t.id);
    void api
      .put("/api/settings/ui.workspace_tabs", { value: JSON.stringify(pageIds) })
      .catch(() => {
        /* best-effort persistence */
      });
  }, [tabs]);

  useEffect(() => {
    if (!activeTab) return;
    const nextUrl =
      activeTab.kind === "page"
        ? PAGE_ROUTES[activeTab.page]
        : activeTab.kind === "files"
          ? `/files/${encodeURIComponent(activeTab.serverId)}`
          : `/terminal/${encodeURIComponent(activeTab.serverId)}`;
    if (window.location.pathname !== nextUrl) {
      window.history.replaceState(null, "", nextUrl);
    }
  }, [activeTab]);

  const openPage = useCallback((page: PageKey) => {
    const id = `page:${page}`;
    setTabs((prev) => {
      if (prev.some((t) => t.id === id)) return prev;
      return [...prev, { id, kind: "page", page }];
    });
    setActiveTabId(id);
  }, []);

  const openTerminal = useCallback(
    (server: Pick<ServerAsset, "id" | "name" | "username" | "host" | "port">) => {
      const seq = seqRef.current++;
      const id = `term:${server.id}:${seq}`;
      const target = `${server.username}@${server.host}:${server.port}`;
      setTabs((prev) => [
        ...prev,
        {
          id,
          kind: "terminal",
          serverId: server.id,
          title: server.name || server.host,
          target,
          status: "connecting",
        },
      ]);
      setActiveTabId(id);
    },
    []
  );

  const openFiles = useCallback((server: Pick<ServerAsset, "id" | "name">) => {
    const id = `files:${server.id}`;
    setTabs((prev) => {
      const existing = prev.find((t) => t.id === id);
      const next = existing
        ? prev
        : [...prev, { id, kind: "files" as const, serverId: server.id, title: server.name || server.id }];
      setActiveTabId(id);
      return next;
    });
  }, []);

  const duplicateTerminal = useCallback((tabId: string) => {
    setTabs((prev) => {
      const src = prev.find((t) => t.id === tabId && t.kind === "terminal");
      if (!src || src.kind !== "terminal") return prev;
      const seq = seqRef.current++;
      const nextId = `term:${src.serverId}:${seq}`;
      const nextTab: WorkspaceTab = {
        id: nextId,
        kind: "terminal",
        serverId: src.serverId,
        title: src.title,
        target: src.target,
        status: "connecting",
      };
      setActiveTabId(nextId);
      return [...prev, nextTab];
    });
  }, []);

  const reconnectTerminal = useCallback((tabId: string) => {
    setTabs((prev) =>
      prev.map((t) =>
        t.id === tabId && t.kind === "terminal"
          ? { ...t, status: "connecting", reconnectSeq: (t.reconnectSeq ?? 0) + 1 }
          : t
      )
    );
    setActiveTabId(tabId);
  }, []);

  const togglePinTab = useCallback((tabId: string) => {
    setTabs((prev) => {
      const target = prev.find((t) => t.id === tabId);
      if (!target) return prev;
      const updated = { ...target, pinned: !target.pinned };
      const rest = prev.filter((t) => t.id !== tabId);
      const pinnedList = rest.filter((t) => t.pinned);
      const unpinnedList = rest.filter((t) => !t.pinned);
      return [...pinnedList, updated, ...unpinnedList];
    });
  }, []);

  const setActiveTab = useCallback((tabId: string) => {
    setActiveTabId(tabId);
  }, []);

  const closeTab = useCallback((tabId: string) => {
    setTabs((prev) => {
      const idx = prev.findIndex((t) => t.id === tabId);
      if (idx === -1) return prev;
      const next = prev.filter((t) => t.id !== tabId);
      setActiveTabId((cur) => (cur === tabId ? fallbackActiveId(next, null, idx) : cur));
      return next;
    });
  }, []);

  const closeOtherTabs = useCallback((tabId: string) => {
    setTabs((prev) => {
      const next = prev.filter((t) => t.id === tabId || t.pinned);
      setActiveTabId((cur) => fallbackActiveId(next, cur === tabId ? tabId : cur, 0));
      return next;
    });
  }, []);

  const closeTabsToLeft = useCallback((tabId: string) => {
    setTabs((prev) => {
      const idx = prev.findIndex((t) => t.id === tabId);
      if (idx <= 0) return prev;
      const next = prev.filter((t, i) => i >= idx || t.pinned);
      setActiveTabId((cur) => fallbackActiveId(next, cur, next.findIndex((t) => t.id === tabId)));
      return next;
    });
  }, []);

  const closeTabsToRight = useCallback((tabId: string) => {
    setTabs((prev) => {
      const idx = prev.findIndex((t) => t.id === tabId);
      if (idx === -1 || idx >= prev.length - 1) return prev;
      const next = prev.filter((t, i) => i <= idx || t.pinned);
      setActiveTabId((cur) => fallbackActiveId(next, cur, idx));
      return next;
    });
  }, []);

  const closeAllTabs = useCallback(() => {
    setTabs((prev) => {
      const next = prev.filter((t) => t.pinned);
      setActiveTabId((cur) => fallbackActiveId(next, cur, 0));
      return next;
    });
  }, []);

  const updateTerminalStatus = useCallback((tabId: string, status: TerminalConnStatus) => {
    setTabs((prev) =>
      prev.map((t) => (t.id === tabId && t.kind === "terminal" && t.status !== status ? { ...t, status } : t))
    );
  }, []);

  const value = useMemo<WorkspaceTabsContextValue>(
    () => ({
      tabs,
      activeTabId,
      activeTab,
      openPage,
      openTerminal,
      openFiles,
      duplicateTerminal,
      reconnectTerminal,
      togglePinTab,
      setActiveTab,
      closeTab,
      closeOtherTabs,
      closeTabsToLeft,
      closeTabsToRight,
      closeAllTabs,
      updateTerminalStatus,
      canAccessPage,
    }),
    [
      tabs,
      activeTabId,
      activeTab,
      openPage,
      openTerminal,
      openFiles,
      duplicateTerminal,
      reconnectTerminal,
      togglePinTab,
      setActiveTab,
      closeTab,
      closeOtherTabs,
      closeTabsToLeft,
      closeTabsToRight,
      closeAllTabs,
      updateTerminalStatus,
      canAccessPage,
    ]
  );

  return <WorkspaceTabsContext.Provider value={value}>{children}</WorkspaceTabsContext.Provider>;
}

export function useWorkspaceTabs(): WorkspaceTabsContextValue {
  const ctx = useContext(WorkspaceTabsContext);
  if (!ctx) throw new Error("useWorkspaceTabs outside WorkspaceTabsProvider");
  return ctx;
}
