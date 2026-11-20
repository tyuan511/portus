export interface SshieldTheme {
  id: string;
  name: string;
  mode: "dark" | "light";
  description: string;
  swatches: [string, string, string, string];
}

// Supabaze-inspired brand system (see DESIGN.md): a white canvas with a
// near-black ink ladder and a single emerald primary (#3ecf8e) as the only
// chromatic event. Light is the brand default; night inverts the surface
// ladder while keeping emerald untouched.
export const SSHIELD_THEMES: SshieldTheme[] = [
  {
    id: "supabase-light",
    name: "Daylight",
    mode: "light",
    description: "White canvas, near-black ink, emerald CTA",
    swatches: ["#ffffff", "#fafafa", "#171717", "#3ecf8e"],
  },
  {
    id: "supabase-night",
    name: "Night",
    mode: "dark",
    description: "Canvas-night surfaces, emerald untouched",
    swatches: ["#1c1c1c", "#202020", "#ededed", "#3ecf8e"],
  },
];

const STORAGE_KEY = "sshield.theme";

export function getSavedThemeId(): string {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && SSHIELD_THEMES.some((t) => t.id === saved)) {
      return saved;
    }
  } catch {
    // ignore storage errors
  }
  // Brand default follows the DESIGN.md white-canvas commitment.
  return "supabase-light";
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
