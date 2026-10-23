// The browser-style status bubble that shows where a link goes while the
// pointer is over it (lib/LinkStatus.svelte, at the bottom of the reading
// pane). Every message frame reports into the same state.

export const linkStatus = $state({ url: "", x: 0, y: 0 });

/** The document whose link is shown now, so a frame that goes away can clear it. */
let source: Document | null = null;

/** Shows the target of the link at or around `target`, or hides the bubble. */
function report(doc: Document, target: EventTarget | null, x: number, y: number) {
  const link = (target as Element | null)?.closest?.("a[href]");
  const href = link?.getAttribute("href")?.trim() ?? "";
  source = href ? doc : null;
  linkStatus.url = href ? readable(href) : "";
  linkStatus.x = x;
  linkStatus.y = y;
}

/** Percent escapes decoded where that gives readable text, as browsers show it. */
function readable(href: string): string {
  try {
    return decodeURI(href);
  } catch {
    return href;
  }
}

/**
 * Follows the pointer over links in `doc`. `offset` places a frame's document
 * in the window, so the bubble can move out of the pointer's way.
 */
export function watchLinks(doc: Document, offset: () => { x: number; y: number } = () => ({ x: 0, y: 0 })) {
  doc.addEventListener("mouseover", (event) => {
    const at = offset();
    report(doc, event.target, event.clientX + at.x, event.clientY + at.y);
  });
  // Leaving the frame: the app's own document doesn't report links.
  doc.addEventListener("mouseout", (event) => {
    if (!event.relatedTarget) forgetLinks(doc);
  });
}

/** Hides the bubble if it shows a link from `doc`. */
export function forgetLinks(doc: Document | null) {
  if (!doc || doc !== source) return;
  source = null;
  linkStatus.url = "";
}
