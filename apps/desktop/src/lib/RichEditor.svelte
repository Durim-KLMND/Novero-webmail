<script lang="ts">
  import { onMount } from "svelte";
  import { hasFormatting, htmlToText, textToHtml } from "./richtext";

  interface Props {
    /** The text to start with, as HTML. */
    html: string;
    placeholder?: string;
    /** Called on every change with the plain text and, when the text uses
     *  formatting plain text can't carry, the HTML ("" otherwise). */
    onChange: (text: string, html: string) => void;
  }
  let { html, placeholder = "", onChange }: Props = $props();

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

  /** Swaps the signature block ("-- " and the lines of `from`) for `to`, if
   *  it is still there unchanged. An empty `to` removes the block. */
  export function replaceSignature(from: string, to: string) {
    if (!editor) return;
    const lines = (sig: string) => sig.replace(/\s+$/, "").split("\n");
    const old = lines(from);
    const children = Array.from(editor.children);
    const text = (el: Element | undefined) => (el?.textContent ?? "").replace(/\u00a0/g, " ");
    for (let at = children.length - 1; at >= 0; at--) {
      if (text(children[at]).trimEnd() !== "--") continue;
      if (!old.every((line, i) => text(children[at + 1 + i]) === line)) continue;
      const dash = children[at];
      const oldLines = children.slice(at + 1, at + 1 + old.length);
      if (to.trim()) {
        const sig = document.createElement("template");
        sig.innerHTML = textToHtml(lines(to).join("\n"));
        dash.after(...Array.from(sig.content.childNodes));
        oldLines.forEach((el) => el.remove());
      } else {
        const blank = at > 0 && text(children[at - 1]) === "" ? [children[at - 1]] : [];
        [...blank, dash, ...oldLines].forEach((el) => el.remove());
      }
      changed();
      return;
    }
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
    const text = event.clipboardData?.getData("text/plain");
    if (text === undefined) return;
    event.preventDefault();
    run("insertHTML", textToHtml(text.replace(/\r\n?/g, "\n")));
  }

  const tools = [
    { command: "bold", label: "B", title: "Bold (Ctrl+B)", class: "bold" },
    { command: "italic", label: "I", title: "Italic (Ctrl+I)", class: "italic" },
    { command: "underline", label: "U", title: "Underline (Ctrl+U)", class: "underline" },
    { command: "insertUnorderedList", label: "•", title: "Bulleted list", class: "" },
    { command: "insertOrderedList", label: "1.", title: "Numbered list", class: "" },
  ];
</script>

<div class="rich">
  <div class="tools" role="toolbar" aria-label="Formatting">
    {#each tools as t (t.command)}
      <button
        class={t.class}
        class:on={active[t.command]}
        title={t.title}
        aria-label={t.title}
        aria-pressed={Boolean(active[t.command])}
        onmousedown={keepSelection}
        onclick={() => run(t.command)}>{t.label}</button
      >
    {/each}
    <button title="Link (Ctrl+K)" aria-label="Link (Ctrl+K)" onmousedown={keepSelection} onclick={openLink}>🔗</button>
    <button title="Clear formatting" aria-label="Clear formatting" onmousedown={keepSelection} onclick={clearFormatting}
      >Tx</button
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
    aria-label="Message"
    data-placeholder={placeholder}
    oninput={changed}
    onkeydown={keydown}
    onpaste={paste}
  ></div>
</div>

<style>
  .rich {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
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
  .bold {
    font-weight: 700;
  }
  .italic {
    font-style: italic;
  }
  .underline {
    text-decoration: underline;
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
  .editor :global(a) {
    color: var(--accent-ink);
  }
</style>
