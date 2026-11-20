import { useEffect, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import "@xterm/xterm/css/xterm.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { api, wsUrl } from "@/lib/api";
import type { ServerAsset } from "@/types";
import { ErrorText } from "@/components/ui/shared";

export default function TerminalPane() {
  const { serverId } = useParams<{ serverId: string }>();
  const containerRef = useRef<HTMLDivElement>(null);
  const [serverName, setServerName] = useState("");
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!serverId) return;
    let ws: WebSocket | null = null;
    let disposed = false;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "'Geist Mono', ui-monospace, SFMono-Regular, monospace",
      theme: {
        background: "#0D1017",
        foreground: "#E6EDF3",
        cursor: "#10B981",
        selectionBackground: "#10B98155",
      },
    });
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

    api.get<ServerAsset[]>(`/api/servers`).then((all) => {
      const s = all.find((x) => x.id === serverId);
      if (s) setServerName(`${s.name} — ${s.username}@${s.host}`);
    }).catch(() => { /* non-fatal */ });

    const cols = term.cols;
    const rows = term.rows;
    ws = new WebSocket(wsUrl(`/ws/terminal/${serverId}?cols=${cols}&rows=${rows}`));
    ws.binaryType = "arraybuffer";

    ws.onopen = () => term.writeln("\x1b[32m✔ connected\x1b[0m");
    ws.onmessage = (ev) => {
      if (typeof ev.data === "string") {
        try {
          const msg = JSON.parse(ev.data);
          if (msg.type === "error") setError(msg.message);
          if (msg.type === "exit") term.writeln("\r\n\x1b[33m— session ended —\x1b[0m");
          if (msg.type === "ready") term.clear();
        } catch { /* ignore */ }
        return;
      }
      term.write(new Uint8Array(ev.data));
    };
    ws.onerror = () => setError("websocket error");
    ws.onclose = () => { if (!disposed) term.writeln("\r\n\x1b[31m— disconnected —\x1b[0m"); };

    const onWindowResize = () => fit.fit();
    window.addEventListener("resize", onWindowResize);

    return () => {
      disposed = true;
      window.removeEventListener("resize", onWindowResize);
      ws?.close();
      term.dispose();
    };
  }, [serverId]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center justify-between border-b px-4 py-2">
        <div>
          <h1 className="text-sm font-semibold">Terminal {serverName && <span className="text-muted-foreground">· {serverName}</span>}</h1>
        </div>
      </div>
      <ErrorText>{error}</ErrorText>
      <div ref={containerRef} className="min-h-0 flex-1 p-2" />
    </div>
  );
}
