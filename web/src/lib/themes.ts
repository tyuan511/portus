export interface SshieldTheme {
  id: string;
  name: string;
  mode: "dark" | "light";
  description: string;
  swatches: [string, string, string, string];
}

export const SSHIELD_THEMES: SshieldTheme[] = [
  {
    id: "termius-dark",
    name: "Termius Dark",
    mode: "dark",
    description: "Signature Termius obsidian & emerald vault",
    swatches: ["#0D1017", "#141824", "#10B981", "#6366F1"],
  },
  {
    id: "termius-light",
    name: "Termius Light",
    mode: "light",
    description: "Clean daylight workspace for bright environments",
    swatches: ["#F6F8FB", "#FFFFFF", "#059669", "#4F46E5"],
  },
  {
    id: "dracula",
    name: "Dracula",
    mode: "dark",
    description: "Iconic dark purple & vibrant pink/cyan accents",
    swatches: ["#282A36", "#21222C", "#BD93F9", "#50FA7B"],
  },
  {
    id: "nord",
    name: "Nord",
    mode: "dark",
    description: "Arctic, north-bluish clean & calm palette",
    swatches: ["#2E3440", "#3B4252", "#88C0D0", "#81A1C1"],
  },
  {
    id: "tokyo-night",
    name: "Tokyo Night",
    mode: "dark",
    description: "Deep midnight indigo & neon sky accents",
    swatches: ["#1A1B26", "#24283B", "#7AA2F7", "#9ECE6A"],
  },
  {
    id: "one-dark",
    name: "One Dark",
    mode: "dark",
    description: "Balanced Atom-inspired slate & blue-green",
    swatches: ["#282C34", "#21252B", "#61AFEF", "#98C379"],
  },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    mode: "dark",
    description: "Precision Ethan Schoonover deep teal dark",
    swatches: ["#002B36", "#073642", "#268BD2", "#2AA198"],
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    mode: "light",
    description: "Warm paper-toned precision light theme",
    swatches: ["#FDF6E3", "#EEE8D5", "#268BD2", "#2AA198"],
  },
  {
    id: "monokai-pro",
    name: "Monokai Pro",
    mode: "dark",
    description: "Warm charcoal with crisp amber & lime",
    swatches: ["#2D2A2E", "#221F22", "#FFD866", "#A9DC76"],
  },
  {
    id: "gruvbox-dark",
    name: "Gruvbox Dark",
    mode: "dark",
    description: "Retro warm earthy dark with golden highlights",
    swatches: ["#282828", "#1D2021", "#FABD2F", "#B8BB26"],
  },
  {
    id: "catppuccin-mocha",
    name: "Catppuccin Mocha",
    mode: "dark",
    description: "Soothing pastel-on-espresso modern dark",
    swatches: ["#1E1E2E", "#181825", "#CBA6F7", "#A6E3A1"],
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
  return "termius-dark";
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
