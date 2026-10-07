import type { Settings } from "./types";

/** Apply Settings → Appearance to this window's page ("system" follows the OS). */
export function applyTheme(theme: Settings["theme"]) {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") root.dataset.theme = theme;
  else delete root.dataset.theme;
}
