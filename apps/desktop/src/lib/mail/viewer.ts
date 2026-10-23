// Which received attachments the app shows itself, and how.
import type { Attachment } from "../api";

export type ViewKind = "image" | "pdf" | "text";

/** Text files larger than this are offered for saving instead. */
export const MAX_TEXT = 2 * 1024 * 1024;

const imageTypes: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  jfif: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  avif: "image/avif",
  ico: "image/x-icon",
  svg: "image/svg+xml",
};

const textExtensions = ["txt", "log", "csv", "tsv", "md", "json", "xml", "yaml", "yml", "ini", "cfg"];

function extension(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/** The type to show an attachment as. Senders often label everything
 * application/octet-stream, so the file name counts too. */
export function mediaType(a: Attachment): string {
  const ext = extension(a.name);
  const type = a.mimeType.toLowerCase();
  if (type === "application/octet-stream" || type === "application/x-download") {
    if (imageTypes[ext]) return imageTypes[ext];
    if (ext === "pdf") return "application/pdf";
    if (textExtensions.includes(ext)) return "text/plain";
  }
  if (type === "image/jpg" || type === "image/pjpeg") return "image/jpeg";
  return type;
}

/** How the app shows an attachment, or null when it can only be saved. */
export function viewKind(a: Attachment): ViewKind | null {
  const type = mediaType(a);
  if (Object.values(imageTypes).includes(type)) return "image";
  if (type === "application/pdf" || type === "application/x-pdf") return "pdf";
  if (a.size > MAX_TEXT) return null;
  if (type.startsWith("text/") && type !== "text/html" && type !== "text/calendar") return "text";
  if (["application/json", "application/xml", "application/x-yaml"].includes(type)) return "text";
  return null;
}

/** Back or forward, as a browser takes them: the mouse's side buttons,
 * Alt+Left/Right, the keyboard's Back/Forward keys, and Backspace for back. */
export type Step = "back" | "forward";

export function stepOfKey(e: KeyboardEvent): Step | null {
  if (e.ctrlKey || e.metaKey || e.shiftKey) return null;
  if (e.key === "BrowserBack" || (e.altKey && e.key === "ArrowLeft") || (!e.altKey && e.key === "Backspace"))
    return "back";
  if (e.key === "BrowserForward" || (e.altKey && e.key === "ArrowRight")) return "forward";
  return null;
}

export function stepOfButton(e: MouseEvent): Step | null {
  return e.button === 3 ? "back" : e.button === 4 ? "forward" : null;
}

/** Whether a key is meant for something else: a field being typed in, or a dialog. */
export function typingOrDialog(e: Event): boolean {
  const target = e.target as HTMLElement | null;
  if (target?.closest?.("input, textarea, select, [contenteditable='true']")) return true;
  return !!document.querySelector('[aria-modal="true"]');
}
