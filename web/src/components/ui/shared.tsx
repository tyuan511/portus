import type { ReactNode } from "react";
import { AlertCircle } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { cn } from "cn";

export function PageHeader({
  title,
  description,
  actions,
}: {
  title: string;
  description?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="shrink-0 border-b border-border bg-card/80 backdrop-blur-xs">
      <div className="mx-auto flex h-13 max-w-6xl items-center justify-between gap-4 px-4 sm:px-6">
        <div className="flex min-w-0 items-baseline gap-3">
          <h1 className="truncate text-sm font-semibold tracking-tight sm:text-base">
            {title}
          </h1>
          {description && (
            <span className="hidden truncate text-xs text-muted-foreground md:inline">
              {description}
            </span>
          )}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
      </div>
    </header>
  );
}

export function ErrorText({ children }: { children?: ReactNode }) {
  if (!children) return null;
  return (
    <Alert variant="destructive" className="mb-4">
      <AlertCircle />
      <AlertDescription>{String(children)}</AlertDescription>
    </Alert>
  );
}

export function Page({
  header,
  children,
  fullWidth = false,
  className,
}: {
  header: ReactNode;
  children: ReactNode;
  fullWidth?: boolean;
  className?: string;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      {header}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div
          className={cn(
            fullWidth ? "h-full w-full p-4 sm:p-6" : "mx-auto w-full max-w-6xl p-4 sm:p-6",
            className
          )}
        >
          {children}
        </div>
      </div>
    </div>
  );
}
