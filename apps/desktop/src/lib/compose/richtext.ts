// Converting between the composer's formatted text and the plain text sent
// alongside it. Quotes are "> " lines in plain text and blockquotes in HTML.

function escape(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

/** Plain text as the editor shows it: a line per <div>, as the editor itself
 *  writes them, and "> " lines as quotes. */
export function textToHtml(text: string): string {
  const lines = text.split("\n");
  let html = "";
  for (let i = 0; i < lines.length; ) {
    if (lines[i].startsWith(">")) {
      const quoted: string[] = [];
      while (i < lines.length && lines[i].startsWith(">"))
        quoted.push(lines[i++].replace(/^> ?/, ""));
      html += `<blockquote>${textToHtml(quoted.join("\n"))}</blockquote>`;
    } else {
      html += `<div>${escape(lines[i++]) || "<br>"}</div>`;
    }
  }
  return html;
}

const BLOCKS = new Set([
  ..."DIV P UL OL LI BLOCKQUOTE H1 H2 H3 H4 H5 H6 PRE HR TABLE THEAD TBODY TFOOT TR".split(
    " ",
  ),
]);

/** The plain-text copy of formatted text: lists get "- " or "1. ", links
 *  their address in angle brackets, quotes "> ". */
export function htmlToText(root: Node): string {
  let out = "";
  const newline = () => {
    if (out && !out.endsWith("\n")) out += "\n";
  };
  function walk(node: Node, list: { ordered: boolean; n: number } | null) {
    for (const child of Array.from(node.childNodes)) {
      if (child.nodeType === Node.TEXT_NODE) {
        out += (child.textContent ?? "").replace(/ /g, " ");
        continue;
      }
      if (child.nodeType !== Node.ELEMENT_NODE) continue;
      const el = child as Element;
      const tag = el.tagName;
      if (tag === "BR") {
        out += "\n";
      } else if (tag === "IMG") {
        const alt = el.getAttribute("alt")?.trim();
        out += alt ? `[${alt}]` : "[picture]";
      } else if (tag === "BLOCKQUOTE") {
        newline();
        const inner = htmlToText(el);
        out += inner
          .split("\n")
          .map((line) => (line ? `> ${line}` : ">"))
          .join("\n");
        out += "\n";
      } else if (tag === "UL" || tag === "OL") {
        newline();
        walk(el, { ordered: tag === "OL", n: 0 });
        newline();
      } else if (tag === "LI") {
        newline();
        out += list?.ordered ? `${++list.n}. ` : "- ";
        walk(el, null);
        newline();
      } else if (tag === "A") {
        const start = out.length;
        walk(el, list);
        const label = out.slice(start).trim();
        const href = el.getAttribute("href") ?? "";
        if (href && href !== label && href !== `mailto:${label}`)
          out += ` <${href}>`;
      } else if (tag === "TD" || tag === "TH") {
        walk(el, list);
        if (el.nextElementSibling) out += " ";
      } else {
        const block = BLOCKS.has(tag);
        if (block) newline();
        walk(el, list);
        if (block) newline();
      }
    }
  }
  walk(root, null);
  return out.endsWith("\n") ? out.slice(0, -1) : out;
}

/** Whether the text uses anything plain text can't carry. Quotes count: in
 *  plain text they are "> " lines, which other mail programs show as such. */
export function hasFormatting(root: Element): boolean {
  return (
    root.querySelector(
      "b, strong, i, em, u, s, strike, a, ul, ol, img, blockquote, table, h1, h2, h3, h4, h5, h6",
    ) !== null
  );
}

/** Pictures wider than this, or heavier than `KEEP_BYTES`, are made smaller
 *  before they go into a message. */
const MAX_WIDTH = 1600;
const KEEP_BYTES = 1_000_000;

/** A picture (`data:` URL) ready for a message: large photos are scaled down
 *  and saved as JPEG; small ones are kept as they are. */
export async function fitPicture(url: string): Promise<string> {
  const img = new Image();
  img.src = url;
  await img.decode();
  const bytes = (url.length - url.indexOf(",") - 1) * 0.75;
  if (img.naturalWidth <= MAX_WIDTH && bytes <= KEEP_BYTES) return url;
  const scale = Math.min(1, MAX_WIDTH / img.naturalWidth);
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(img.naturalWidth * scale);
  canvas.height = Math.round(img.naturalHeight * scale);
  const ctx = canvas.getContext("2d");
  if (!ctx) return url;
  // PNGs may be see-through; JPEG has no transparency, so they get white.
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL("image/jpeg", 0.85);
}

/** Reads a pasted or dropped file as a `data:` URL. */
export function readFile(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}
