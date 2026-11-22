import { useEffect, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import "@xterm/xterm/css/xterm.css";
import { Terminal } from "@xterm/xterm";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import {
  AArrowDown,
  AArrowUp,
  ChevronDown,
  ChevronUp,
  ClipboardPaste,
  Copy,
  CopyPlus,
  RotateCw,
  Terminal as TerminalIcon,
  TextSelect,
  X,
} from "lucide-react";
import { cn } from "cn";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuGroup,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { ErrorText } from "@/components/ui/shared";
import { StatusPill } from "@/components/sshield/StatusDot";
import { wsUrl } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TerminalConnStatus } from "@/lib/workspace-tabs";

const XTERM_THEMES = {
  dark: {
    background: "#000000",
    foreground: "#ededed",
    cursor: "#ededed",
    selectionBackground: "#ededed33",
  },
  light: {
    background: "#ffffff",
    foreground: "#171717",
    cursor: "#171717",
    selectionBackground: "#17171722",
  },
} as const;

type ThemeMode = keyof typeof XTERM_THEMES;
type PolicyMode = "audit" | "enforce";

const FONT_SIZE_KEY = "sshield.term.fontSize";
const MIN_FONT_SIZE = 10;
const MAX_FONT_SIZE = 20;
const DEFAULT_FONT_SIZE = 13;
const MAX_RECONNECT_ATTEMPTS = 4;
const RECONNECT_DELAYS_MS = [1000, 2000, 4000, 8000];

// Search highlight decorations: geist amber ring for matches, blue for the active one.
const SEARCH_OPTIONS = {
  decorations: {
    matchBorder: "#f5a623",
    matchOverviewRuler: "#f5a623",
    activeMatchBorder: "#0070f3",
    activeMatchColorOverviewRuler: "#0070f3",
  },
};

function currentMode(): ThemeMode {
  return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

function loadFontSize(): number {
  try {
    const raw = window.localStorage.getItem(FONT_SIZE_KEY);
    const parsed = raw ? Number.parseInt(raw, 10) : NaN;
    if (Number.isFinite(parsed)) {
      return Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, parsed));
    }
  } catch {
    /* ignore */
  }
  return DEFAULT_FONT_SIZE;
}

export interface TerminalPaneProps {
  serverId?: string;
  serverName?: string;
  serverTarget?: string;
  isActive?: boolean;
  reconnectSeq?: number;
  onStatusChange?: (status: TerminalConnStatus) => void;
  onDuplicate?: () => void;
}

export default function TerminalPane({
  serverId: propServerId,
  serverName,
  serverTarget,
  isActive = true,
  reconnectSeq = 0,
  onStatusChange,
  onDuplicate,
}: TerminalPaneProps) {
  const params = useParams<{ serverId: string }>();
  const serverId = propServerId ?? params.serverId;
  const { t } = useI18n();
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const onStatusRef = useRef(onStatusChange);
  onStatusRef.current = onStatusChange;

  const [error, setError] = useState<string>();
  const [mode, setMode] = useState<ThemeMode>(currentMode);
  const [status, setStatus] = useState<TerminalConnStatus>("connecting");
  const [sessionNonce, setSessionNonce] = useState(0);
  const [reconnecting, setReconnecting] = useState(false);
  const [policy, setPolicy] = useState<PolicyMode | null>(null);
  const [fontSize, setFontSize] = useState<number>(loadFontSize);
  const [query, setQuery] = useState("");
  const [noResult, setNoResult] = useState(false);
  const [hasSelection, setHasSelection] = useState(false);

  const updateStatus = (next: TerminalConnStatus) => {
    setStatus(next);
    onStatusRef.current?.(next);
  };

  useEffect(() => {
    const root = document.documentElement;
    const obs = new MutationObserver(() => setMode(currentMode()));
    obs.observe(root, { attributes: true, attributeFilter: ["class", "data-theme"] });
    return () => obs.disconnect();
  }, []);

  useEffect(() => {
    const term = termRef.current;
    if (term) term.options.theme = XTERM_THEMES[mode];
  }, [mode]);

  // Re-fit and focus whenever this tab becomes visible again.
  useEffect(() => {
    if (!isActive) return;
    const id = requestAnimationFrame(() => {
      const el = containerRef.current;
      if (el && el.clientWidth > 0 && el.clientHeight > 0) {
        try {
          fitRef.current?.fit();
        } catch {
          /* ignore */
        }
        termRef.current?.focus();
      }
    });
    return () => cancelAnimationFrame(id);
  }, [isActive]);

  useEffect(() => {
    if (!serverId || !containerRef.current) return;
    let disposed = false;
    let exited = false;
    let attempts = 0;
    let reconnectTimer: number | null = null;
    updateStatus("connecting");
    setError(undefined);
    setPolicy(null);
    setReconnecting(false);
    setHasSelection(false);

    const term = new Terminal({
      cursorBlink: true,
      fontSize: loadFontSize(),
      fontFamily: "'Geist Mono Variable', ui-monospace, SFMono-Regular, monospace",
      theme: XTERM_THEMES[currentMode()],
    });
    termRef.current = term;
    const fit = new FitAddon();
    fitRef.current = fit;
    term.loadAddon(fit);
    // OSC 52 support so remote-side copy/paste reaches the browser clipboard.
    term.loadAddon(new ClipboardAddon());
    const search = new SearchAddon();
    term.loadAddon(search);
    searchRef.current = search;
    term.open(containerRef.current);

    const safeFit = () => {
      const el = containerRef.current;
      if (!el || el.clientWidth === 0 || el.clientHeight === 0) return;
      try {
        fit.fit();
      } catch {
        /* hidden */
      }
    };
    safeFit();

    const send = (msg: Record<string, unknown>) => {
      const sock = wsRef.current;
      if (sock?.readyState === WebSocket.OPEN) {
        sock.send(JSON.stringify(msg));
      }
    };

    term.onResize(({ cols, rows }) => {
      send({ type: "resize", cols, rows });
    });
    term.onData((data) => {
      send({ type: "input", data });
    });
    term.onSelectionChange(() => setHasSelection(term.hasSelection()));
    if (isActive) term.focus();

    // (Re)opens the data channel; the terminal instance is kept across retries.
    const connect = () => {
      if (disposed || !containerRef.current) return;
      const cols = term.cols;
      const rows = term.rows;
      const sock = new WebSocket(wsUrl(`/ws/terminal/${serverId}?cols=${cols}&rows=${rows}`));
      wsRef.current = sock;
      sock.binaryType = "arraybuffer";

      sock.onopen = () => {
        if (disposed) return;
        attempts = 0;
        setReconnecting(false);
        setError(undefined);
        updateStatus("connected");
        term.writeln(`\x1b[32m${t("terminal.connected")}\x1b[0m`);
      };
      sock.onmessage = (ev) => {
        if (typeof ev.data === "string") {
          try {
            const msg = JSON.parse(ev.data);
            if (msg.type === "error") setError(msg.message);
            if (msg.type === "notice") {
              term.write(`\r\n\x1b[33m${String(msg.message ?? "")}\x1b[0m\r\n`);
            }
            if (msg.type === "exit") {
              exited = true;
              updateStatus("disconnected");
              term.writeln(`\r\n\x1b[33m${t("terminal.sessionEnded")}\x1b[0m`);
            }
            if (msg.type === "ready") {
              setPolicy(msg.policy === "enforce" || msg.policy === "audit" ? msg.policy : null);
              term.clear();
            }
          } catch {
            /* ignore */
          }
          return;
        }
        term.write(new Uint8Array(ev.data));
      };
      sock.onerror = () => {
        if (!disposed) {
          updateStatus("disconnected");
          setError(t("terminal.wsError"));
        }
      };
      sock.onclose = () => {
        if (disposed) return;
        if (exited) {
          // Server-side session end: no auto-reconnect.
          updateStatus("disconnected");
          return;
        }
        // Unexpected drop: retry with exponential backoff, then fall back
        // to the manual reconnect button.
        if (attempts < MAX_RECONNECT_ATTEMPTS) {
          const delay = RECONNECT_DELAYS_MS[Math.min(attempts, RECONNECT_DELAYS_MS.length - 1)];
          attempts += 1;
          updateStatus("connecting");
          setReconnecting(true);
          term.writeln(`\r\n\x1b[33m${t("terminal.reconnecting")}\x1b[0m`);
          reconnectTimer = window.setTimeout(() => {
            if (!disposed) connect();
          }, delay);
        } else {
          setReconnecting(false);
          updateStatus("disconnected");
          term.writeln(`\r\n\x1b[31m${t("terminal.disconnected")}\x1b[0m`);
        }
      };
    };
    connect();

    const resizeObserver = new ResizeObserver(() => safeFit());
    resizeObserver.observe(containerRef.current);

    return () => {
      disposed = true;
      if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
      resizeObserver.disconnect();
      wsRef.current?.close();
      wsRef.current = null;
      searchRef.current = null;
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // Re-open only when serverId or user-triggered reconnect nonce changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId, sessionNonce, reconnectSeq]);

  const runSearch = (value: string, dir: "next" | "prev") => {
    const search = searchRef.current;
    if (!search) return;
    if (!value) {
      search.clearDecorations();
      setNoResult(false);
      return;
    }
    const found =
      dir === "next" ? search.findNext(value, SEARCH_OPTIONS) : search.findPrevious(value, SEARCH_OPTIONS);
    setNoResult(!found);
  };

  const changeQuery = (value: string) => {
    setQuery(value);
    runSearch(value, "next");
  };

  const closeSearch = () => {
    setQuery("");
    setNoResult(false);
    searchRef.current?.clearDecorations();
    termRef.current?.focus();
  };

  const copySelection = async () => {
    const text = termRef.current?.getSelection();
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      termRef.current?.focus();
    } catch {
      /* clipboard unavailable */
    }
  };

  const pasteClipboard = async () => {
    try {
      const text = await navigator.clipboard.readText();
      if (text) {
        // Routed through onData → ws "input" frames.
        termRef.current?.paste(text);
        termRef.current?.focus();
      }
    } catch {
      /* clipboard unavailable */
    }
  };

  const selectAll = () => {
    termRef.current?.selectAll();
  };

  const changeFontSize = (delta: number) => {
    const next = Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, fontSize + delta));
    if (next === fontSize) return;
    setFontSize(next);
    try {
      window.localStorage.setItem(FONT_SIZE_KEY, String(next));
    } catch {
      /* ignore */
    }
    const term = termRef.current;
    if (term) {
      term.options.fontSize = next;
      requestAnimationFrame(() => {
        try {
          fitRef.current?.fit();
        } catch {
          /* hidden */
        }
      });
    }
  };

  const statusTone = reconnecting
    ? "warning"
    : status === "connected"
      ? "success"
      : status === "connecting"
        ? "warning"
        : "danger";
  const statusLabel = reconnecting
    ? t("terminal.reconnecting")
    : status === "connected"
      ? t("terminal.statusConnected")
      : status === "connecting"
        ? t("terminal.statusConnecting")
        : t("terminal.statusDisconnected");

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <div className="flex h-11 shrink-0 items-center justify-between border-b border-border bg-card/70 px-4">
        <div className="flex min-w-0 items-center gap-2.5">
          <TerminalIcon className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="truncate text-xs font-semibold tracking-tight">
            {serverName || t("terminal.title")}
          </span>
          {serverTarget && (
            <span className="hidden truncate rounded border border-border bg-secondary px-2 py-0.5 font-mono text-[11px] text-muted-foreground sm:inline-block">
              {serverTarget}
            </span>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          {policy && (
            <span
              title={`${t("terminal.policyMode")}: ${policy}`}
              className={cn(
                "hidden shrink-0 rounded-full border px-2 py-0.5 font-mono text-[11px] font-medium md:inline-block",
                policy === "enforce"
                  ? "border-geist-warning/40 bg-geist-warning/10 text-geist-warning"
                  : "border-border bg-card text-muted-foreground"
              )}
            >
              {t("terminal.policyMode")}: {policy}
            </span>
          )}

          <div className="hidden items-center gap-0.5 sm:flex">
            <Input
              value={query}
              onChange={(e) => changeQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  runSearch(query, e.shiftKey ? "prev" : "next");
                }
              }}
              placeholder={t("terminal.search")}
              className="h-6 w-32 text-xs md:w-44 md:text-xs"
            />
            <Button
              variant="ghost"
              size="icon-xs"
              disabled={!query}
              title={t("terminal.search")}
              aria-label={t("terminal.search")}
              onClick={() => runSearch(query, "prev")}
            >
              <ChevronUp />
            </Button>
            <Button
              variant="ghost"
              size="icon-xs"
              disabled={!query}
              title={t("terminal.search")}
              aria-label={t("terminal.search")}
              onClick={() => runSearch(query, "next")}
            >
              <ChevronDown />
            </Button>
            <Button
              variant="ghost"
              size="icon-xs"
              disabled={!query}
              aria-label={t("common.cancel")}
              onClick={closeSearch}
            >
              <X />
            </Button>
            {query && noResult && (
              <span className="shrink-0 font-mono text-[11px] text-geist-error">
                {t("terminal.searchNoResult")}
              </span>
            )}
          </div>

          <Button
            variant="ghost"
            size="icon-xs"
            disabled={!hasSelection}
            title={t("terminal.copy")}
            aria-label={t("terminal.copy")}
            className="hidden sm:inline-flex"
            onClick={() => void copySelection()}
          >
            <Copy />
          </Button>
          <Button
            variant="ghost"
            size="icon-xs"
            title={t("terminal.paste")}
            aria-label={t("terminal.paste")}
            className="hidden sm:inline-flex"
            onClick={() => void pasteClipboard()}
          >
            <ClipboardPaste />
          </Button>

          <div className="hidden items-center gap-0.5 md:flex">
            <Button
              variant="ghost"
              size="icon-xs"
              disabled={fontSize <= MIN_FONT_SIZE}
              title={t("terminal.fontSize")}
              aria-label={t("terminal.fontSize")}
              onClick={() => changeFontSize(-1)}
            >
              <AArrowDown />
            </Button>
            <Button
              variant="ghost"
              size="icon-xs"
              disabled={fontSize >= MAX_FONT_SIZE}
              title={t("terminal.fontSize")}
              aria-label={t("terminal.fontSize")}
              onClick={() => changeFontSize(1)}
            >
              <AArrowUp />
            </Button>
          </div>

          {status === "disconnected" && !reconnecting && (
            <Button variant="outline" size="xs" onClick={() => setSessionNonce((n) => n + 1)}>
              <RotateCw data-icon="inline-start" />
              {t("terminal.reconnect")}
            </Button>
          )}
          {onDuplicate && (
            <Button variant="outline" size="xs" onClick={onDuplicate}>
              <CopyPlus data-icon="inline-start" />
              <span className="hidden sm:inline">{t("terminal.duplicate")}</span>
            </Button>
          )}
          <StatusPill
            label={statusLabel}
            tone={statusTone}
            pulse={reconnecting || status === "connected" || status === "connecting"}
          />
        </div>
      </div>

      {error && (
        <div className="px-4 pt-3">
          <ErrorText>{error}</ErrorText>
        </div>
      )}

      <ContextMenu>
        <ContextMenuTrigger
          render={
            <div
              ref={containerRef}
              className="terminal-host min-h-0 flex-1"
              style={{ backgroundColor: XTERM_THEMES[mode].background }}
            />
          }
        />
        <ContextMenuContent className="w-44">
          <ContextMenuGroup>
            <ContextMenuItem disabled={!hasSelection} onClick={() => void copySelection()}>
              <Copy />
              <span>{t("terminal.copy")}</span>
            </ContextMenuItem>
            <ContextMenuItem onClick={() => void pasteClipboard()}>
              <ClipboardPaste />
              <span>{t("terminal.paste")}</span>
            </ContextMenuItem>
            <ContextMenuItem onClick={selectAll}>
              <TextSelect />
              <span>{t("terminal.selectAll")}</span>
            </ContextMenuItem>
          </ContextMenuGroup>
        </ContextMenuContent>
      </ContextMenu>
    </div>
  );
}
