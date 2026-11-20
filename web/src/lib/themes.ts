export interface SshieldTheme {
  id: string;
  name: string;
  mode: "dark" | "light";
  description: string;
  swatches: [string, string, string, string];
}

// Vercel-style monochrome brand: white canvas with near-black ink and a
// solid near-black CTA (inverted on Night). The UI stays strictly greyscale;
// chroma lives only in the emerald shield logo.
export const SSHIELD_THEMES: SshieldTheme[] = [
  {
    id: "daylight",
    name: "Daylight",
    mode: "light",
    description: "White canvas, black ink, high-contrast greys",
    swatches: ["#ffffff", "#fafafa", "#171717", "#171717"],
  },
  {
    id: "night",
    name: "Night",
    mode: "dark",
    description: "Near-black canvas, inverted light CTA",
    swatches: ["#0a0a0a", "#1a1a1a", "#a1a1a1", "#ededed"],
  },
];

// Themes were renamed when the Supabase-style spec was dropped; keep saved
// preferences working across the rename.
const LEGACY_THEME_IDS: Record<string, string> = {
  "supabase-light": "daylight",
  "supabase-night": "night",
};

const STORAGE_KEY = "sshield.theme";

export function getSavedThemeId(): string {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    const resolved = saved ? LEGACY_THEME_IDS[saved] ?? saved : undefined;
    if (resolved && SSHIELD_THEMES.some((t) => t.id === resolved)) {
      return resolved;
    }
  } catch {
    // ignore storage errors
  }
  return "daylight";
}

export function applyTheme(themeId: string): SshieldTheme {
  const theme = SSHIELD_THEMES.find((t) => t.id === themeId) ?? SSHIELD_THEMES[0];
  const root = document.documentElement;
  root.setAttribute("data-theme", theme.id);
  if (theme.mode === "dark") {
    root.classList.add("dark");
  } else {
    root.classList.remove("dark");
  }
  try {
    localStorage.setItem(STORAGE_KEY, theme.id);
  } catch {
    // ignore
  }
  return theme;
}
