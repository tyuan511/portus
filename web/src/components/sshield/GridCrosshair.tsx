import { cn } from "cn";

export function GridCrosshair({
  position,
  className,
}: {
  position: "top-left" | "top-right" | "bottom-left" | "bottom-right";
  className?: string;
}) {
  const posClass = {
    "top-left": "-top-2 -left-2",
    "top-right": "-top-2 -right-2",
    "bottom-left": "-bottom-2 -left-2",
    "bottom-right": "-bottom-2 -right-2",
  }[position];

  return (
    <span
      aria-hidden="true"
      className={cn(
        "pointer-events-none absolute z-10 flex size-4 items-center justify-center text-muted-foreground/50 select-none",
        posClass,
        className
      )}
    >
      <svg width="15" height="15" viewBox="0 0 15 15" fill="none">
        <path d="M7.5 1V14M1 7.5H14" stroke="currentColor" strokeWidth="1" />
      </svg>
    </span>
  );
}
