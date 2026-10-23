import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { Theme } from "../api";

const KEY = "novero.theme";

/** Applies a theme. "system" follows Windows; the others override it. The
 *  choice is also kept in local storage so the next start paints in the right
 *  colours before the settings have loaded. */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
  paintWindow();
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // Storage can be unavailable; the theme still applies for this session.
  }
}

export function cachedTheme(): Theme {
  try {
    const value = localStorage.getItem(KEY);
    if (value === "light" || value === "dark") return value;
  } catch {
    // Fall back to the system theme.
  }
  return "system";
}

/** Gives the window the page's background, so a fast resize shows that and
 *  not white while the page catches up (window.rs paints it at first). */
function paintWindow() {
  const color = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim();
  if (color) getCurrentWebviewWindow().setBackgroundColor(color).catch(() => {});
}

// "system" follows Windows switching between light and dark.
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", paintWindow);
