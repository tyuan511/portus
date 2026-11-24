import { ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";

interface PaginationProps {
  page: number; // 1-based
  pageSize: number;
  total: number;
  onPageChange: (page: number) => void;
}

// Simple offset pagination for list pages backed by limit/offset APIs.
export function Pagination({ page, pageSize, total, onPageChange }: PaginationProps) {
  const { t } = useI18n();
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  if (total <= 0 && page <= 1) return null;
  const from = total === 0 ? 0 : (page - 1) * pageSize + 1;
  const to = Math.min(total, page * pageSize);

  return (
    <div className="flex items-center justify-between gap-3 px-1 py-2 text-xs text-muted-foreground">
      <span className="font-mono">
        {t("common.pageInfo", { from, to, total })}
      </span>
      <div className="flex items-center gap-1">
        <Button
          variant="outline"
          size="icon-xs"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1)}
          aria-label={t("common.prevPage")}
        >
          <ChevronLeft />
        </Button>
        <span className="min-w-14 text-center font-mono">
          {page} / {totalPages}
        </span>
        <Button
          variant="outline"
          size="icon-xs"
          disabled={page >= totalPages}
          onClick={() => onPageChange(page + 1)}
          aria-label={t("common.nextPage")}
        >
          <ChevronRight />
        </Button>
      </div>
    </div>
  );
}
