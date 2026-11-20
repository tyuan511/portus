import { useEffect, useState } from "react";
import { Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { applyTheme, getSavedThemeId } from "@/lib/themes";

export function ThemeToggle({ compact = false }: { compact?: boolean }) {
  const [themeId, setThemeId] = useState(getSavedThemeId);

  useEffect(() => {
    const sync = () => setThemeId(getSavedThemeId());
    window.addEventListener("sshield:theme", sync);
    const obs = new MutationObserver(() => {
      const current = document.documentElement.getAttribute("data-theme");
      if (current) setThemeId(current);
    });
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => {
      window.removeEventListener("sshield:theme", sync);
      obs.disconnect();
    };
  }, []);

  const next = themeId === "night" ? "daylight" : "night";
  const isNight = themeId === "night";

  const toggle = () => {
    const applied = applyTheme(next);
    setThemeId(applied.id);
    window.dispatchEvent(new CustomEvent("sshield:theme"));
  };

  return (
    <Button
      type="button"
      variant="ghost"
      size={compact ? "icon-sm" : "sm"}
      onClick={toggle}
      aria-label={isNight ? "Switch to Daylight theme" : "Switch to Night theme"}
      title={isNight ? "Daylight" : "Night"}
      className="text-muted-foreground hover:text-foreground"
    >
      {isNight ? <Sun data-icon="inline-start" /> : <Moon data-icon="inline-start" />}
      {!compact && <span>{isNight ? "Night" : "Daylight"}</span>}
    </Button>
  );
}
