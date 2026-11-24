import { Languages } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";

export function LangToggle({ compact = false }: { compact?: boolean }) {
  const { lang, setLang } = useI18n();
  const isZh = lang === "zh-CN";

  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      onClick={() => setLang(isZh ? "en" : "zh-CN")}
      aria-label="Toggle language"
      className="font-mono text-xs text-muted-foreground hover:text-foreground"
    >
      <Languages data-icon="inline-start" />
      <span>{compact ? (isZh ? "中" : "EN") : (isZh ? "中文" : "EN")}</span>
    </Button>
  );
}
