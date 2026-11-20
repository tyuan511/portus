import { useEffect, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import "@xterm/xterm/css/xterm.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { wsUrl } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { ErrorText } from "@/components/ui/shared";

// xterm surfaces per brand theme (DESIGN.md): the canvas-night code-block
// surface for Night, the white canvas for Daylight — emerald stays the only
// chromatic event in both.
const XTERM_THEMES = {
  dark: {
    background: "#1c1c1c",
    foreground: "#ededed",
    cursor: "#3ecf8e",
    selectionBackground: "#3ecf8e55",
  },
  light: {
    background: "#ffffff",
    foreground: "#171717",
    cursor: "#3ecf8e",
    selectionBackground: "#3ecf8e55",
  },
} as const;

type ThemeMode = keyof typeof XTERM_THEMES;

function currentMode(): ThemeMode {
  return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

export default function TerminalPane() {
  const { serverId } = useParams<{ serverId: string }>();
  const { t } = useI18n();
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const [error, setError] = useState<string>();
  const [mode, setMode] = useState<ThemeMode>(currentMode);

  // The terminal follows the global Daylight/Night theme; applyTheme()
  // mutates the root element, so watch it there.
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

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'Geist Mono', ui-monospace, SFMono-Regular, monospace",
      theme: XTERM_THEMES[currentMode()],
    });
    termRef.current = term;
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current!);
    try { fit.fit(); } catch { /* hidden */ }
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
      term.writeln(`\x1b[32m${t("terminal.connected")}\x1b[0m`);
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data === "string") {
        try {
          const msg = JSON.parse(ev.data);
          if (msg.type === "error") setError(msg.message);
          if (msg.type === "exit") term.writeln(`\r\n\x1b[33m${t("terminal.sessionEnded")}\x1b[0m`);
          if (msg.type === "ready") term.clear();
        } catch { /* ignore */ }
        return;
      }
      term.write(new Uint8Array(ev.data));
    };
    // StrictMode dev double-mount tears the first socket down immediately;
    // its abort must not surface as an error (the replacement socket works).
    ws.onerror = () => { if (!disposed) setError(t("terminal.wsError")); };
    ws.onclose = () => { if (!disposed) term.writeln(`\r\n\x1b[31m${t("terminal.disconnected")}\x1b[0m`); };

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

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-12 shrink-0 items-center border-b px-4">
        <h1 className="text-base font-semibold">{t("terminal.title")}</h1>
      </div>
      <ErrorText>{error}</ErrorText>
      {/* Host background matches the xterm surface so the sub-row leftover
          space below the last line blends in. */}
      <div
        ref={containerRef}
        className="terminal-host min-h-0 flex-1"
        style={{ backgroundColor: XTERM_THEMES[mode].background }}
      />
    </div>
  );
}
