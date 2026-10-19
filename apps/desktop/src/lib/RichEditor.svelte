<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";
  import { onMount } from "svelte";
  import { fitPicture, hasFormatting, htmlToText, readFile, textToHtml } from "./richtext";
  import { api, errorText } from "./api";
  import { toast } from "./toast.svelte";
  import { openMenu, type MenuItem } from "./menus.svelte";

  interface Props {
    /** The text to start with, as HTML. */
    html: string;
    placeholder?: string;
    /** Called on every change with the plain text and, when the text uses
     *  formatting plain text can't carry, the HTML ("" otherwise). */
    onChange: (text: string, html: string) => void;
    /** What the text is, for screen readers. */
    label?: string;
    /** A few lines high, for a signature, instead of filling its pane. */
    compact?: boolean;
  }
  let { html, placeholder = "", onChange, label = "Message", compact = false }: Props = $props();

  let editor = $state<HTMLDivElement>();
  let linkInput = $state<HTMLInputElement>();
  let linking = $state(false);
  let linkUrl = $state("");
  let active = $state<Record<string, boolean>>({});
  /** Where the link goes: the selection when the link row was opened. */
  let savedRange: Range | null = null;

  onMount(() => {
    if (editor) editor.innerHTML = html;
    document.addEventListener("selectionchange", updateActive);
    return () => document.removeEventListener("selectionchange", updateActive);
  });

  /** Replaces the whole text, e.g. after the signature changed. */
  export function setContent(next: string) {
    if (editor) editor.innerHTML = next;
    changed();
  }

  /** Swaps the signature block ("-- " and the signature) for another one,
   *  formatted (`toHtml`) or plain (`to`); an empty one removes the block.
   *  The block is found by the mark it was given when put in, or else by
   *  the lines of the plain-text signature `from`, if they are unchanged. */
  export function replaceSignature(from: string, to: string, toHtml = "") {
    if (!editor) return;
    const text = (el: Element | undefined) => (el?.textContent ?? "").replace(/\u00a0/g, " ");
    let block: Element[] = [];
    let before: Element | null = null;
    const marked = editor.querySelector(":scope > [data-signature]");
    if (marked) {
      block = [marked];
      before = marked.previousElementSibling;
    } else {
      const old = from.replace(/\s+$/, "").split("\n");
      const children = Array.from(editor.children);
      for (let at = children.length - 1; at >= 0 && !block.length; at--) {
        if (text(children[at]).trimEnd() !== "--") continue;
        if (!old.every((line, i) => text(children[at + 1 + i]) === line)) continue;
        block = children.slice(at, at + 1 + old.length);
        before = children[at - 1] ?? null;
      }
    }
    if (!block.length) return;
    if (to.trim() || toHtml.trim()) {
      const sig = document.createElement("div");
      sig.dataset.signature = "";
      sig.innerHTML = `<div>-- </div>${toHtml.trim() || textToHtml(to.replace(/\s+$/, ""))}`;
      block[0].before(sig);
    } else if (before && text(before) === "") {
      // The blank line above the block goes with it.
      before.remove();
    }
    block.forEach((el) => el.remove());
    changed();
  }

  /** The current HTML, formatted or not. */
  export function content(): string {
    return editor?.innerHTML ?? "";
  }

  /** Puts the cursor at the very start, above any quoted text. */
  export function focusStart() {
    if (!editor) return;
    editor.focus();
    const range = document.createRange();
    range.setStart(editor, 0);
    range.collapse(true);
    const selection = getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    editor.scrollTop = 0;
  }

  function changed() {
    if (!editor) return;
    onChange(htmlToText(editor), hasFormatting(editor) ? editor.innerHTML : "");
  }

  function inside(): boolean {
    const node = getSelection()?.anchorNode;
    return Boolean(editor && node && editor.contains(node));
  }

  function updateActive() {
    if (!inside()) return;
    active = {
      bold: document.queryCommandState("bold"),
      italic: document.queryCommandState("italic"),
      underline: document.queryCommandState("underline"),
      insertUnorderedList: document.queryCommandState("insertUnorderedList"),
      insertOrderedList: document.queryCommandState("insertOrderedList"),
    };
  }

  function run(command: string, value?: string) {
    editor?.focus();
    document.execCommand(command, false, value);
    changed();
    updateActive();
  }

  function clearFormatting() {
    run("removeFormat");
    run("unlink");
  }

  /** Keeps the editor's selection when a toolbar button is pressed. */
  function keepSelection(event: MouseEvent) {
    event.preventDefault();
  }

  function openLink() {
    const selection = getSelection();
    savedRange = inside() && selection?.rangeCount ? selection.getRangeAt(0).cloneRange() : null;
    const link = savedRange?.startContainer.parentElement?.closest("a");
    linkUrl = link?.getAttribute("href")?.replace(/^mailto:/, "") ?? "";
    linking = true;
    queueMicrotask(() => linkInput?.focus());
  }

  /** "example.com" means a web page, "ana@example.com" an address. */
  function address(url: string): string {
    const u = url.trim();
    if (/^(https?:|mailto:)/i.test(u)) return u;
    if (/^[^\s@/]+@[^\s@/]+\.[^\s@/]+$/.test(u)) return `mailto:${u}`;
    return `https://${u}`;
  }

  function escape(text: string): string {
    return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }

  function addLink() {
    const url = linkUrl.trim();
    closeLink();
    if (!url || !editor) return;
    editor.focus();
    const selection = getSelection();
    if (savedRange) {
      selection?.removeAllRanges();
      selection?.addRange(savedRange);
    }
    const href = address(url);
    if (!savedRange || savedRange.collapsed) {
      run("insertHTML", `<a href="${escape(href)}">${escape(url)}</a>&nbsp;`);
    } else {
      run("createLink", href);
    }
  }

  function closeLink() {
    linking = false;
    linkUrl = "";
  }

  function linkKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      addLink();
    } else if (event.key === "Escape") {
      event.preventDefault();
      closeLink();
      editor?.focus();
    }
  }

  function keydown(event: KeyboardEvent) {
    if (event.key.toLowerCase() === "k" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      openLink();
    }
  }

  /** Pasted text arrives without its formatting, so mail from other programs
   *  doesn't bring their fonts and colours. */
  function paste(event: ClipboardEvent) {
    const pictures = Array.from(event.clipboardData?.files ?? []).filter((f) => PICTURE_TYPES.includes(f.type));
    if (pictures.length) {
      event.preventDefault();
      const at = selection();
      (async () => {
        for (const file of pictures) await insertPicture(await readFile(file), at);
      })().catch(() => toast("The picture could not be pasted.", "error"));
      return;
    }
    const text = event.clipboardData?.getData("text/plain");
    if (text === undefined) return;
    event.preventDefault();
    run("insertHTML", textToHtml(text.replace(/\r\n?/g, "\n")));
  }

  /** Picture types every mail client shows. */
  const PICTURE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

  function selection(): Range | null {
    const sel = getSelection();
    return inside() && sel?.rangeCount ? sel.getRangeAt(0).cloneRange() : null;
  }

  /** The cursor position under a point of the window, if it is in the text. */
  export function rangeAt(x: number, y: number): Range | null {
    const range = document.caretRangeFromPoint?.(x, y) ?? null;
    return range && editor?.contains(range.startContainer) ? range : null;
  }

  /** Puts a picture (`data:` URL) into the text at `at`, else at the cursor,
   *  else at the end. Large photos are made smaller first. */
  export async function insertPicture(url: string, at: Range | null = selection()) {
    if (!editor) return;
    const fitted = await fitPicture(url);
    editor.focus();
    const sel = getSelection();
    sel?.removeAllRanges();
    if (at) {
      sel?.addRange(at);
    } else {
      const end = document.createRange();
      end.selectNodeContents(editor);
      end.collapse(false);
      sel?.addRange(end);
    }
    run("insertHTML", `<img src="${fitted}" alt="">`);
  }

  /** Right-clicking a picture offers sizes; anything else gets the text menu. */
  function pictureMenu(event: MouseEvent) {
    const img = event.target instanceof HTMLImageElement ? event.target : null;
    if (!img) return;
    const size = (label: string, width: number | null): MenuItem => ({
      label,
      icon: img.getAttribute("width") === (width == null ? null : String(width)) ? "check" : undefined,
      run: () => {
        img.removeAttribute("height");
        if (width == null) img.removeAttribute("width");
        else img.setAttribute("width", String(Math.min(width, img.naturalWidth)));
        changed();
      },
    });
    openMenu(event, [
      size("Small", 200),
      size("Medium", 400),
      size("Large", 640),
      size("Original size", null),
      "separator",
      {
        label: "Remove picture",
        icon: "trash",
        danger: true,
        run: () => {
          img.remove();
          changed();
        },
      },
    ]);
  }

  async function pickPicture() {
    const at = selection();
    try {
      const url = await api.pickPicture();
      if (url) await insertPicture(url, at);
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  const tools: { command: string; icon: IconName; title: string }[] = [
    { command: "bold", icon: "bold", title: "Bold (Ctrl+B)" },
    { command: "italic", icon: "italic", title: "Italic (Ctrl+I)" },
    { command: "underline", icon: "underline", title: "Underline (Ctrl+U)" },
    { command: "insertUnorderedList", icon: "list", title: "Bulleted list" },
    { command: "insertOrderedList", icon: "list-ordered", title: "Numbered list" },
  ];
</script>

<div class="rich" class:compact>
  <div class="tools" role="toolbar" aria-label="Formatting">
    {#each tools as t (t.command)}
      <button
        class:on={active[t.command]}
        title={t.title}
        aria-label={t.title}
        aria-pressed={Boolean(active[t.command])}
        onmousedown={keepSelection}
        onclick={() => run(t.command)}><Icon name={t.icon} /></button
      >
    {/each}
    <button title="Link (Ctrl+K)" aria-label="Link (Ctrl+K)" onmousedown={keepSelection} onclick={openLink}><Icon name="link" /></button>
    <button title="Insert picture" aria-label="Insert picture" onmousedown={keepSelection} onclick={pickPicture}
      ><Icon name="image" /></button
    >
    <button title="Clear formatting" aria-label="Clear formatting" onmousedown={keepSelection} onclick={clearFormatting}
      ><Icon name="clear-formatting" /></button
    >
    {#if linking}
      <span class="link-row">
        <input
          bind:this={linkInput}
          bind:value={linkUrl}
          onkeydown={linkKeydown}
          placeholder="https://… or name@example.com"
          aria-label="Link address"
          spellcheck="false"
        />
        <button onclick={addLink}>Add</button>
        <button onclick={closeLink}>Cancel</button>
      </span>
    {/if}
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="editor"
    bind:this={editor}
    contenteditable="true"
    role="textbox"
    tabindex="0"
    aria-multiline="true"
    aria-label={label}
    data-placeholder={placeholder}
    oninput={changed}
    onkeydown={keydown}
    onpaste={paste}
    oncontextmenu={pictureMenu}
  ></div>
</div>

<style>
  .rich {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .compact {
    flex: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .compact .tools {
    padding: 4px 6px;
  }
  .compact .editor {
    min-height: 72px;
    max-height: 220px;
    padding: 8px 10px;
  }
  .tools {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 2px;
    padding: 6px 20px;
    border-bottom: 1px solid var(--border);
  }
  .tools button {
    width: auto;
    min-width: 30px;
    padding: 3px 8px;
    font-size: 13px;
    border: 1px solid transparent;
    background: none;
    color: var(--text);
  }
  .tools button:hover {
    border-color: var(--border);
  }
  .tools button.on {
    background: var(--bg-selected);
    border-color: var(--border);
  }
  .link-row {
    display: flex;
    gap: 4px;
    margin-left: 8px;
    flex: 1;
    min-width: 240px;
  }
  .link-row input {
    flex: 1;
    padding: 3px 8px;
    font-size: 13px;
  }
  .link-row button {
    border-color: var(--border);
  }
  .editor {
    flex: 1;
    min-height: 120px;
    overflow-y: auto;
    padding: 16px 24px;
    font: 14px/1.5 var(--font);
    color: var(--text);
    background: var(--bg);
    word-wrap: break-word;
    white-space: pre-wrap;
  }
  .editor:focus {
    outline: none;
  }
  .editor:empty::before {
    content: attr(data-placeholder);
    color: var(--text-quiet);
    pointer-events: none;
  }
  .editor :global(blockquote) {
    margin: 4px 0;
    padding-left: 12px;
    border-left: 3px solid var(--border);
    color: var(--text-quiet);
  }
  .editor :global(ul),
  .editor :global(ol) {
    margin: 4px 0;
    padding-left: 24px;
  }
  .editor :global(img) {
    max-width: 100%;
    height: auto;
  }
  .editor :global(a) {
    color: var(--accent-ink);
  }
</style>
