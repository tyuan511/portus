import { useCallback, useEffect, useRef, useState } from "react";
import "@xterm/xterm/css/xterm.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Pause, Play } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ErrorText } from "@/components/ui/shared";
import { api } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import type { TerminalEvent, TerminalSession } from "@/types";

// Mirrors TerminalPane's xterm palette so replays look identical to live sessions.
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

const SPEEDS = [1, 2, 4, 8, 16] as const;

const DIM = "\x1b[90m"; // dark gray, readable on both themes
const RED = "\x1b[31m";
const RESET = "\x1b[0m";

function currentMode(): ThemeMode {
  return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

// Strip CR/LF so typed text can be matched against the terminal echo.
const asEchoText = (s: string) => s.replace(/[\r\n]+/g, "");

// Marks input events whose text is echoed by a following output event (before
// the next keystroke); those are skipped during playback to avoid rendering
// the keystroke twice.
function computeEchoFlags(events: TerminalEvent[]): boolean[] {
  const echoed = new Array<boolean>(events.length).fill(false);
  for (let i = 0; i < events.length; i++) {
    if (events[i].direction !== "input") continue;
    const text = asEchoText(events[i].data);
    if (!text) {
      echoed[i] = true; // bare Enter / control chars: nothing worth rendering
      continue;
    }
    for (let j = i + 1; j < events.length && events[j].direction !== "input"; j++) {
      if (events[j].direction === "output" && asEchoText(events[j].data).includes(text)) {
        echoed[i] = true;
        break;
      }
    }
  }
  return echoed;
}

// --- small formatting helpers shared with the audit recordings list ---

export function fmtBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${i === 0 ? v : v.toFixed(1)} ${units[i]}`;
}

export function fmtDuration(ms?: number | null): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return "—";
  const total = Math.floor(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

export interface TerminalReplayProps {
  session: TerminalSession | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function TerminalReplay({ session, open, onOpenChange }: TerminalReplayProps) {
  const { t } = useI18n();
  const [rec, setRec] = useState<TerminalSession | null>(session);
  const [events, setEvents] = useState<TerminalEvent[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState<number>(1);
  const [progress, setProgress] = useState(0); // rendered playback clock, ms
  const [mode, setMode] = useState<ThemeMode>(currentMode);

  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const eventsRef = useRef<TerminalEvent[]>([]);
  const echoRef = useRef<boolean[]>([]);
  const clockRef = useRef(0); // ms into the recording
  const cursorRef = useRef(0); // next event index to feed
  const lastFrameRef = useRef<number | null>(null);
  const lastUiRef = useRef(0);

  const t0 = events.length > 0 ? events[0].ts_ms : 0;
  const durationMs = events.length > 0 ? Math.max(0, events[events.length - 1].ts_ms - t0) : 0;

  // Theme adaptation, same approach as TerminalPane (class + data-theme attrs).
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

  // Read-only xterm instance, created while the dialog is open and disposed on close.
  useEffect(() => {
    if (!open) return;
    const el = containerRef.current;
    if (!el) return;
    const term = new Terminal({
      disableStdin: true,
      cursorBlink: false,
      fontSize: 13,
      fontFamily: "'Geist Mono Variable', ui-monospace, SFMono-Regular, monospace",
      scrollback: 10000,
      theme: XTERM_THEMES[currentMode()],
    });
    termRef.current = term;
    const fit = new FitAddon();
    fitRef.current = fit;
    term.loadAddon(fit);
    term.open(el);
    try {
      fit.fit();
    } catch {
      /* hidden */
    }
    const ro = new ResizeObserver(() => {
      if (el.clientWidth === 0 || el.clientHeight === 0) return;
      try {
        fit.fit();
      } catch {
        /* hidden */
      }
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [open]);

  // Fetch the recording each time the dialog opens.
  useEffect(() => {
    if (!open || !session) return;
    let cancelled = false;
    setLoading(true);
    setError(undefined);
    setPlaying(false);
    setProgress(0);
    setEvents([]);
    setRec(session);
    clockRef.current = 0;
    cursorRef.current = 0;
    eventsRef.current = [];
    echoRef.current = [];
    api
      .get<{ session: TerminalSession; events: TerminalEvent[] }>(
        `/api/terminal-sessions/${encodeURIComponent(session.id)}/events`,
      )
      .then((r) => {
        if (cancelled) return;
        setRec(r.session ?? session);
        setEvents(r.events);
        eventsRef.current = r.events;
        echoRef.current = computeEchoFlags(r.events);
        setPlaying(true); // start as soon as the recording is ready
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : "load failed");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, session]);

  // Drains every event recorded up to `ms` into the terminal.
  const feedUpTo = useCallback(
    (ms: number) => {
      const term = termRef.current;
      if (!term) return;
      const evs = eventsRef.current;
      const flags = echoRef.current;
      const start = evs.length > 0 ? evs[0].ts_ms : 0;
      while (cursorRef.current < evs.length && evs[cursorRef.current].ts_ms - start <= ms) {
        const ev = evs[cursorRef.current];
        cursorRef.current += 1;
        if (ev.direction === "output") {
          term.write(ev.data);
        } else if (ev.direction === "notice") {
          term.write(`${RED}${ev.data}${RESET}`);
        } else if (!flags[cursorRef.current - 1]) {
          const cmd = asEchoText(ev.data);
          if (cmd) term.write(`\r\n${DIM}[${t("audit.sessions.input", { cmd })}]${RESET}\r\n`);
        }
      }
    },
    [t],
  );

  // Real-time playback: rAF advances the clock by dt * speed and drains events.
  useEffect(() => {
    if (!playing || events.length === 0) return;
    lastFrameRef.current = null;
    let raf = 0;
    const tick = (now: number) => {
      const last = lastFrameRef.current ?? now;
      lastFrameRef.current = now;
      const dt = Math.min(now - last, 250); // clamp jumps after tab switches
      clockRef.current = Math.min(durationMs, clockRef.current + dt * speed);
      feedUpTo(clockRef.current);
      if (now - lastUiRef.current >= 100) {
        lastUiRef.current = now;
        setProgress(clockRef.current); // ~10fps is plenty for the scrubber
      }
      if (clockRef.current >= durationMs && cursorRef.current >= events.length) {
        setPlaying(false);
        setProgress(clockRef.current);
        return;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing, speed, durationMs, events, feedUpTo]);

  // Seek: rewind the buffer and instantly replay everything up to the target.
  const seek = (ms: number) => {
    clockRef.current = ms;
    cursorRef.current = 0;
    termRef.current?.reset();
    feedUpTo(ms);
    setProgress(ms);
    lastUiRef.current = performance.now();
  };

  const handleOpenChange = (o: boolean) => {
    if (!o) setPlaying(false); // stops the rAF loop before xterm disposal
    onOpenChange(o);
  };

  const duration = rec?.closed_at ? Date.parse(rec.closed_at) - Date.parse(rec.opened_at) : null;

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="gap-3 sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle className="flex flex-wrap items-center gap-2 text-sm">
            <span>{rec?.username || "—"}</span>
            <span className="font-mono text-xs font-normal text-muted-foreground">
              {rec?.server_name || rec?.server_host || "—"}
            </span>
            {rec && (
              <Badge variant={rec.policy_mode === "enforce" ? "default" : "secondary"}>
                {rec.policy_mode}
              </Badge>
            )}
            {rec?.truncated && <Badge variant="destructive">{t("audit.sessions.truncated")}</Badge>}
          </DialogTitle>
          <DialogDescription className="flex flex-wrap items-center gap-x-3 gap-y-1 font-mono text-xs">
            <span>{rec ? new Date(rec.opened_at).toLocaleString() : ""}</span>
            <span>
              {t("audit.sessions.colDuration")} {fmtDuration(duration)}
            </span>
            <span>
              {t("audit.sessions.colSize")} {fmtBytes(rec?.byte_size ?? 0)}
            </span>
          </DialogDescription>
        </DialogHeader>

        <ErrorText>{error}</ErrorText>

        <div className="relative overflow-hidden rounded-lg border border-border">
          <div
            ref={containerRef}
            className="terminal-host h-[440px]"
            style={{ backgroundColor: XTERM_THEMES[mode].background }}
          />
          {loading && (
            <div className="absolute inset-0 flex items-center justify-center bg-background/70 font-mono text-xs text-muted-foreground">
              {t("common.loading")}
            </div>
          )}
        </div>

        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="icon-sm"
            disabled={events.length === 0}
            onClick={() => setPlaying((p) => !p)}
            aria-label={playing ? t("audit.sessions.pause") : t("audit.sessions.play")}
          >
            {playing ? <Pause className="size-4" /> : <Play className="size-4" />}
          </Button>
          <select
            className="h-7 rounded-lg border bg-background px-2 text-xs"
            value={speed}
            onChange={(e) => setSpeed(Number(e.target.value))}
            title={t("audit.sessions.speed")}
          >
            {SPEEDS.map((s) => (
              <option key={s} value={s}>
                {s}×
              </option>
            ))}
          </select>
          <input
            type="range"
            min={0}
            max={Math.max(durationMs, 1)}
            step={50}
            value={Math.round(progress)}
            onChange={(e) => seek(Number(e.target.value))}
            className="h-1 min-w-0 flex-1 cursor-pointer accent-primary"
            aria-label={t("audit.sessions.replay")}
          />
          <span className="shrink-0 font-mono text-xs text-muted-foreground">
            {fmtDuration(progress)} / {fmtDuration(durationMs)}
          </span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
