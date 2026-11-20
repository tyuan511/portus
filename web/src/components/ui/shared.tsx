import type { ReactNode } from "react";

export function PageHeader({ title, actions }: { title: string; actions?: ReactNode }) {
  // h-12 matches the sidebar brand row so both bottom borders align.
  return (
    <div className="flex h-12 shrink-0 items-center justify-between border-b px-4">
      <h1 className="text-base font-semibold">{title}</h1>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  );
}

export function ErrorText({ children }: { children?: ReactNode }) {
  if (!children) return null;
  return <p className="text-sm text-destructive">{String(children)}</p>;
}

export function Page({ header, children }: { header: ReactNode; children: ReactNode }) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      {header}
      <div className="min-h-0 flex-1 overflow-y-auto p-4">{children}</div>
    </div>
  );
}
