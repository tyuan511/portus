import { useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import "@xterm/xterm/css/xterm.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ErrorText } from "@/components/ui/shared";
import { StatusPill } from "@/components/sshield/StatusDot";
import { wsUrl } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

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
type ConnState = "connecting" | "connected" | "disconnected";

function currentMode(): ThemeMode {
  return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

export default function TerminalPane() {
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const { t } = useI18n();
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const [error, setError] = useState<string>();
  const [mode, setMode] = useState<ThemeMode>(currentMode);
  const [status, setStatus] = useState<ConnState>("connecting");

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

  useEffect(() => {
    if (!serverId) return;
    let ws: WebSocket | null = null;
    let disposed = false;
    setStatus("connecting");

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'Geist Mono Variable', ui-monospace, SFMono-Regular, monospace",
      theme: XTERM_THEMES[currentMode()],
    });
    termRef.current = term;
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current!);
    try {
      fit.fit();
    } catch {
      /* hidden */
    }
    term.onResize(({ cols, rows }) => {
      ws?.send(JSON.stringify({ type: "resize", cols, rows }));
    });
    term.onData((data) => {
      ws?.send(JSON.stringify({ type: "input", data }));
    });
    term.focus();

    const cols = term.cols;
    const rows = term.rows;
    ws = new WebSocket(wsUrl(`/ws/terminal/${serverId}?cols=${cols}&rows=${rows}`));
    ws.binaryType = "arraybuffer";

    ws.onopen = () => {
      if (disposed) return;
      setError(undefined);
      setStatus("connected");
      term.writeln(`\x1b[32m${t("terminal.connected")}\x1b[0m`);
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data === "string") {
        try {
          const msg = JSON.parse(ev.data);
          if (msg.type === "error") setError(msg.message);
          if (msg.type === "exit") {
            setStatus("disconnected");
            term.writeln(`\r\n\x1b[33m${t("terminal.sessionEnded")}\x1b[0m`);
          }
          if (msg.type === "ready") term.clear();
        } catch {
          /* ignore */
        }
        return;
      }
      term.write(new Uint8Array(ev.data));
    };
    ws.onerror = () => {
      if (!disposed) {
        setStatus("disconnected");
        setError(t("terminal.wsError"));
      }
    };
    ws.onclose = () => {
      if (!disposed) {
        setStatus("disconnected");
        term.writeln(`\r\n\x1b[31m${t("terminal.disconnected")}\x1b[0m`);
      }
    };

    const onWindowResize = () => fit.fit();
    window.addEventListener("resize", onWindowResize);

    return () => {
      disposed = true;
      window.removeEventListener("resize", onWindowResize);
      ws?.close();
      term.dispose();
      termRef.current = null;
    };
  }, [serverId]);

  const statusTone = status === "connected" ? "success" : status === "connecting" ? "warning" : "danger";
  const statusLabel =
    status === "connected"
      ? t("terminal.statusConnected")
      : status === "connecting"
        ? t("terminal.statusConnecting")
        : t("terminal.statusDisconnected");

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <div className="flex h-13 shrink-0 items-center justify-between border-b border-border bg-card px-4">
        <div className="flex items-center gap-3">
          <Button variant="outline" size="xs" onClick={() => navigate("/")}>
            <ArrowLeft data-icon="inline-start" />
            {t("terminal.back")}
          </Button>
          <span className="text-sm font-semibold tracking-tight">{t("terminal.title")}</span>
          {serverId && (
            <span className="hidden rounded border border-border bg-secondary px-2 py-0.5 font-mono text-[11px] text-muted-foreground sm:inline-block">
              {serverId}
            </span>
          )}
        </div>
        <StatusPill label={statusLabel} tone={statusTone} pulse={status === "connected" || status === "connecting"} />
      </div>
      {error && (
        <div className="px-4 pt-3">
          <ErrorText>{error}</ErrorText>
        </div>
      )}
      <div
        ref={containerRef}
        className="terminal-host min-h-0 flex-1"
        style={{ backgroundColor: XTERM_THEMES[mode].background }}
      />
    </div>
  );
}
