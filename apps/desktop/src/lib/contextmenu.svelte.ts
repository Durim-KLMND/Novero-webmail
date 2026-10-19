// The app's own right-click menu, in place of the browser's (Back, Reload,
// Inspect…). Components open it with the actions that fit what was clicked;
// anything else gets the text menu, or nothing (see `fallbackMenu`).
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { IconName } from "./Icon.svelte";

export interface MenuAction {
  label: string;
  icon?: IconName;
  /** Shown on the right, e.g. "R". */
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  run?: () => void;
  /** A submenu, such as the folders for "Move to". */
  items?: MenuItem[];
}

export type MenuItem = MenuAction | "separator";

/** Entries left out of a menu, from `condition && item`. */
type Skipped = null | false | undefined | "";

export const menu = $state<{ x: number; y: number; items: MenuItem[] } | { items: null }>({ items: null });

/** Opens the menu at the pointer. `offset` moves it, for clicks inside a frame. */
export function openMenu(event: MouseEvent, items: MenuItem[], offset = { x: 0, y: 0 }) {
  event.preventDefault();
  event.stopPropagation();
  const shown = tidy(items);
  if (!shown.length) {
    closeMenu();
    return;
  }
  Object.assign(menu, { x: event.clientX + offset.x, y: event.clientY + offset.y, items: shown });
}

/** Opens the menu at a point, e.g. under a "⋯" button. */
export function openMenuAt(x: number, y: number, items: MenuItem[]) {
  const shown = tidy(items);
  if (shown.length) Object.assign(menu, { x, y, items: shown });
}

export function closeMenu() {
  Object.assign(menu, { items: null });
}

/** Drops separators at the ends and doubled ones, left by optional items. */
function tidy(items: (MenuItem | Skipped)[]): MenuItem[] {
  const out: MenuItem[] = [];
  for (const item of items) {
    if (!item) continue;
    if (item === "separator" && (out.length === 0 || out[out.length - 1] === "separator")) continue;
    out.push(item);
  }
  if (out[out.length - 1] === "separator") out.pop();
  return out;
}

/** Builds a menu with optional entries: `false`/`null` entries are left out. */
export function items(...entries: (MenuItem | Skipped)[]): MenuItem[] {
  return tidy(entries);
}

/** The element clicked. Not `instanceof Element`: a message's frame has its own `Element`. */
function element(target: EventTarget | null): Element | null {
  return target && typeof (target as Element).closest === "function" ? (target as Element) : null;
}

function editable(target: EventTarget | null): HTMLElement | null {
  const el = element(target);
  const field = el?.closest<HTMLElement>("input, textarea, [contenteditable='true']");
  if (!field) return null;
  if (field instanceof HTMLInputElement && !["text", "search", "email", "url", "tel", "password", ""].includes(field.type))
    return null;
  return field;
}

function selectedIn(field: HTMLElement | null, doc: Document): string {
  if (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) {
    return field.value.slice(field.selectionStart ?? 0, field.selectionEnd ?? 0);
  }
  return doc.getSelection()?.toString() ?? "";
}

function linkAt(target: EventTarget | null): string | null {
  const a = element(target)?.closest("a[href]");
  const href = a?.getAttribute("href") ?? "";
  return /^(https?:|mailto:)/i.test(href) ? href : null;
}

let writeTo: ((link: string) => void) | null = null;

/** What "Write to this address" on an email link does: start a new message. */
export function setMailtoHandler(handler: ((link: string) => void) | null) {
  writeTo = handler;
}

/** Copies text. Errors are rare and there is nowhere useful to show them. */
export function copy(text: string) {
  writeText(text).catch(() => navigator.clipboard?.writeText(text));
}

/** The menu for places without their own: fields get Cut, Copy and Paste;
 *  selected text Copy; links Open and Copy address. `doc` is the document the
 *  click happened in (a message's frame has its own). */
export function fallbackMenu(event: MouseEvent, doc: Document = document, offset = { x: 0, y: 0 }) {
  if (event.defaultPrevented) return;
  const field = editable(event.target);
  const selection = selectedIn(field, doc);
  const link = linkAt(event.target);
  const password = field instanceof HTMLInputElement && field.type === "password";
  const readOnly = field && ((field as HTMLInputElement).readOnly || (field as HTMLInputElement).disabled);
  const entries = items(
    link && !link.startsWith("mailto:") && { label: "Open link", icon: "external-link", run: () => openUrl(link) },
    link && link.startsWith("mailto:") && writeTo && {
      label: "Write to this address",
      icon: "unread",
      run: () => writeTo?.(link),
    },
    link && {
      label: link.startsWith("mailto:") ? "Copy address" : "Copy link address",
      icon: "link",
      run: () => copy(link.replace(/^mailto:/i, "").split("?")[0]),
    },
    link && "separator",
    field && !readOnly && {
      label: "Cut",
      icon: "cut",
      shortcut: "Ctrl+X",
      disabled: !selection || password,
      run: () => {
        copy(selection);
        field.focus();
        doc.execCommand("delete");
      },
    },
    (field || selection) && {
      label: "Copy",
      icon: "copy",
      shortcut: "Ctrl+C",
      disabled: !selection || password,
      run: () => copy(selection),
    },
    field && !readOnly && {
      label: "Paste",
      icon: "paste",
      shortcut: "Ctrl+V",
      run: async () => {
        const text = await readText().catch(() => "");
        field.focus();
        if (text) doc.execCommand("insertText", false, text);
      },
    },
    (field || selection) && "separator",
    (field || doc !== document) && {
      label: "Select all",
      icon: "select-all",
      shortcut: "Ctrl+A",
      run: () => {
        if (field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) field.select();
        else if (field) {
          field.focus();
          doc.execCommand("selectAll");
        } else {
          doc.getSelection()?.selectAllChildren(doc.body);
        }
      },
    },
  );
  if (entries.length) openMenu(event, entries, offset);
  else event.preventDefault();
}
