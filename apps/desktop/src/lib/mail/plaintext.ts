// Plain-text mail marks quoted lines with "> ". The reader shows those runs
// as quotes with a bar beside them, as formatted mail shows its quotes.

/** Plain text in order: runs of ordinary lines, and quotes, which may hold quotes. */
export type TextBlock =
  | { kind: "text"; text: string }
  | { kind: "quote"; blocks: TextBlock[] };

export function quoteBlocks(text: string): TextBlock[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const blocks: TextBlock[] = [];
  for (let i = 0; i < lines.length; ) {
    if (lines[i].startsWith(">")) {
      const quoted: string[] = [];
      while (i < lines.length && lines[i].startsWith(">"))
        quoted.push(lines[i++].replace(/^> ?/, ""));
      blocks.push({ kind: "quote", blocks: quoteBlocks(quoted.join("\n")) });
    } else {
      const plain: string[] = [];
      while (i < lines.length && !lines[i].startsWith(">"))
        plain.push(lines[i++]);
      blocks.push({ kind: "text", text: plain.join("\n") });
    }
  }
  return blocks;
}
