// Release notes as GitHub writes them ("Generate release notes"), made
// readable in the app: the pull request links, authors and the changelog
// link go, and so does the list of new contributors.
//
//   ## What's Changed
//   * Calendar invites by @someone in https://github.com/.../pull/15
//   **Full Changelog**: https://github.com/.../compare/v0.1.0...v0.2.0

import { fileSize } from "./format";

export type NoteLine = { kind: "heading" | "item" | "text"; text: string };

const SKIP_SECTIONS = /^new contributors$/i;
const SKIP_HEADINGS = /^what'?s changed$/i;

function plain(text: string): string {
  return text
    .replace(/\s+by @[\w-]+(\[bot\])? in https?:\/\/\S+$/, "")
    .replace(/\s+in https?:\/\/\S+\/pull\/\d+$/, "")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/\*\*|__|`/g, "")
    .trim();
}

export function releaseNotes(notes: string | null): NoteLine[] {
  const lines: NoteLine[] = [];
  let skipping = false;
  for (const raw of (notes ?? "").split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || /^\*\*full changelog\*\*/i.test(line) || line.startsWith("<!--")) continue;
    const heading = line.match(/^#{1,6}\s+(.*)$/);
    if (heading) {
      const text = plain(heading[1]);
      skipping = SKIP_SECTIONS.test(text);
      if (!skipping && !SKIP_HEADINGS.test(text)) lines.push({ kind: "heading", text });
      continue;
    }
    if (skipping) continue;
    const item = line.match(/^[-*+]\s+(.*)$/);
    const text = plain(item ? item[1] : line);
    if (text) lines.push({ kind: item ? "item" : "text", text });
  }
  return lines;
}

/** "4.2 MB of 12.0 MB" while downloading. */
export function downloadText(downloaded: number, total: number | null): string {
  return total ? `${fileSize(downloaded)} of ${fileSize(total)}` : fileSize(downloaded);
}

/** 0 to 100, or null when the size isn't known. */
export function downloadPercent(downloaded: number, total: number | null): number | null {
  return total ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
}
