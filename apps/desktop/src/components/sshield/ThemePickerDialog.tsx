import { useState } from "react";
import { Check, Moon, Palette, Sun } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { SSHIELD_THEMES, type SshieldTheme } from "@/lib/themes";

interface ThemePickerDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  activeThemeId: string;
  onSelectTheme: (themeId: string) => void;
}

export function ThemePickerDialog({
  open,
  onOpenChange,
  activeThemeId,
  onSelectTheme,
}: ThemePickerDialogProps) {
  const [filter, setFilter] = useState<"all" | "dark" | "light">("all");

  const visibleThemes = SSHIELD_THEMES.filter(
    (t) => filter === "all" || t.mode === filter
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="border-border bg-card text-card-foreground sm:max-w-2xl">
        <DialogHeader>
          <div className="flex items-center gap-2.5">
            <div className="flex size-8 items-center justify-center rounded-lg bg-primary/15 text-primary">
              <Palette className="size-4" />
            </div>
            <div>
              <DialogTitle>Appearance &amp; Termius Themes</DialogTitle>
              <DialogDescription>
                支持浅色 / 深色及 Termius 内置经典配色，即时生效并自动记忆。
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <Tabs value={filter} onValueChange={(v) => setFilter(v as "all" | "dark" | "light")}>
          <TabsList>
            <TabsTrigger value="all">全部 ({SSHIELD_THEMES.length})</TabsTrigger>
            <TabsTrigger value="dark" className="gap-1">
              <Moon className="size-3.5" /> 深色
            </TabsTrigger>
            <TabsTrigger value="light" className="gap-1">
              <Sun className="size-3.5" /> 浅色
            </TabsTrigger>
          </TabsList>
        </Tabs>

        <div className="grid max-h-[55vh] grid-cols-1 gap-2.5 overflow-y-auto pr-1 sm:grid-cols-2">
          {visibleThemes.map((theme: SshieldTheme) => {
            const active = theme.id === activeThemeId;
            return (
              <button
                key={theme.id}
                onClick={() => onSelectTheme(theme.id)}
                className={`flex items-start justify-between rounded-xl border p-3 text-left transition ${
                  active
                    ? "border-primary bg-primary/10 ring-1 ring-primary/40"
                    : "border-border bg-muted/40 hover:border-primary/40 hover:bg-muted/70"
                }`}
              >
                <div className="space-y-1.5">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-semibold text-foreground">{theme.name}</span>
                    <Badge variant="outline" className="text-[10px]">
                      {theme.mode}
                    </Badge>
                  </div>
                  <p className="text-xs text-muted-foreground">{theme.description}</p>
                  <div className="flex items-center gap-1.5 pt-1">
                    {theme.swatches.map((hex, i) => (
                      <span
                        key={i}
                        className="size-4 rounded-full border border-black/20 shadow-xs"
                        style={{ backgroundColor: hex }}
                      />
                    ))}
                  </div>
                </div>
                {active && (
                  <div className="flex size-5 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground">
                    <Check className="size-3" />
                  </div>
                )}
              </button>
            );
          })}
        </div>
      </DialogContent>
    </Dialog>
  );
}
