// Whether this window draws its own title bar (lib/TitleBar.svelte). The
// window itself knows: it opens without the Windows frame when the setting is
// on, and Settings switches every open window at once (window.rs).
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

const KEY = "novero.titlebar";

export const titlebar = $state({ on: cached() });

/** The last known answer, so the first paint already has the right layout. */
function cached(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

function set(on: boolean) {
  titlebar.on = on;
  try {
    localStorage.setItem(KEY, on ? "on" : "off");
  } catch {
    // Storage can be unavailable; the bar still follows the window.
  }
}

/** Follows the window's frame from now on. */
export function watchTitlebar() {
  getCurrentWindow()
    .isDecorated()
    .then((framed) => set(!framed))
    .catch(() => {});
  listen<boolean>("titlebar", (event) => set(event.payload)).catch(() => {});
}
