import logo from "@/assets/logo.png";

// Brand mark from the original client icon: an emerald shield with the `>_`
// terminal glyph. Renders at any size; the artwork carries its own rounded
// dark-green tile, so it sits cleanly on both light and night surfaces.
export function BrandLogo({ size = 24, className }: { size?: number; className?: string }) {
  return (
    <img
      src={logo}
      alt="Bulwark"
      width={size}
      height={size}
      className={`shrink-0 rounded-[22%] ${className ?? ""}`}
      style={{ width: size, height: size }}
    />
  );
}
