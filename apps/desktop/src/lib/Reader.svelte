<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, errorText, type FlagKind, type Folder, type MessageSummary, type ParsedMessage } from "./api";
  import { fileSize, folderLabel, fullDate } from "./format";

  interface Props {
    summary: MessageSummary | null;
    /** The folder the message is in, and the other folders of its account. */
    folder: Folder | null;
    folders: Folder[];
    onOpened: () => void;
    onArchive: () => void;
    onDelete: () => void;
    onToggle: (flag: FlagKind, value: boolean) => void;
    onMove: (folder: number) => void;
  }
  let { summary, folder, folders, onOpened, onArchive, onDelete, onToggle, onMove }: Props = $props();

  const messageId = $derived(summary?.id ?? null);
  const canArchive = $derived(folder?.role !== "archive" && folders.some((f) => f.role === "archive"));
  const moveTargets = $derived(folders.filter((f) => f.id !== folder?.id));

  let message = $state<ParsedMessage | null>(null);
  let loading = $state(false);
  let error = $state("");
  const offline = $derived(error.startsWith("Network error"));
  let allowRemote = $state(false);
  let request = 0;

  async function load(id: number, remote: boolean) {
    const mine = ++request;
    loading = true;
    error = "";
    try {
      const result = await api.openMessage(id, remote);
      if (mine !== request) return;
      message = result;
      onOpened();
    } catch (err) {
      if (mine === request) error = errorText(err);
    } finally {
      if (mine === request) loading = false;
    }
  }

  $effect(() => {
    const id = messageId;
    allowRemote = false;
    message = null;
    if (id != null) load(id, false);
  });

  function loadImages() {
    allowRemote = true;
    if (messageId != null) load(messageId, true);
  }

  /** The message HTML in its own document: no scripts, no network unless images are allowed. */
  function frameDoc(html: string, remote: boolean): string {
    const images = remote ? "data: https: http:" : "data:";
    return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${images}; style-src 'unsafe-inline'; font-src data:">
<base target="_blank">
<style>body{margin:0;padding:20px;font:14px/1.5 "Anthropic Sans","Segoe UI",system-ui,sans-serif;color:#1a1a19;background:#fff;word-wrap:break-word}img{max-width:100%;height:auto}</style>
</head><body>${html}</body></html>`;
  }

  /** Links open in the default browser instead of inside the app. */
  function wireFrame(frame: HTMLIFrameElement) {
    const doc = frame.contentDocument;
    if (!doc) return;
    doc.addEventListener("click", (event) => {
      const link = (event.target as Element | null)?.closest?.("a[href]");
      if (!link) return;
      event.preventDefault();
      const href = link.getAttribute("href") ?? "";
      if (/^(https?:|mailto:)/i.test(href)) openUrl(href).catch(() => {});
    });
    frame.style.height = `${Math.max(doc.documentElement.scrollHeight, 200)}px`;
  }
</script>

<article class="reader" aria-label="Message">
  {#if messageId == null}
    <div class="empty quiet">Select a message to read it.</div>
  {:else if !message && !error}
    <div class="empty"><span class="spinner"></span></div>
  {:else}
    <div class="actions" role="toolbar" aria-label="Message actions">
      {#if canArchive}<button onclick={onArchive}>Archive</button>{/if}
      <button onclick={onDelete}>{folder?.role === "trash" ? "Delete forever" : "Delete"}</button>
      {#if summary}
        <button onclick={() => onToggle("seen", !summary.flags.seen)}>
          {summary.flags.seen ? "Mark unread" : "Mark read"}
        </button>
        <button aria-pressed={summary.flags.flagged} onclick={() => onToggle("flagged", !summary.flags.flagged)}>
          {summary.flags.flagged ? "⚑ Flagged" : "⚐ Flag"}
        </button>
      {/if}
      {#if moveTargets.length}
        <select
          aria-label="Move to folder"
          onchange={(e) => {
            const target = Number(e.currentTarget.value);
            e.currentTarget.value = "";
            if (target) onMove(target);
          }}
        >
          <option value="">Move to…</option>
          {#each moveTargets as f (f.id)}
            <option value={f.id}>{folderLabel(f.name, f.role, f.delimiter)}</option>
          {/each}
        </select>
      {/if}
    </div>
    {#if error || !message}
      <div class="pad">
        <p class="error">
          {#if offline}
            This message hasn't been downloaded yet and the server can't be reached. It will open once you're back online.
          {:else}
            Could not open this message: {error}
          {/if}
        </p>
        <button onclick={() => messageId != null && load(messageId, allowRemote)} disabled={loading}>Try again</button>
      </div>
    {:else}
    <header>
      <h1>{message.subject || "(no subject)"}</h1>
      <dl>
        <dt>From</dt><dd>{message.from ?? "(unknown)"}</dd>
        {#if message.to}<dt>To</dt><dd>{message.to}</dd>{/if}
        {#if message.cc}<dt>Cc</dt><dd>{message.cc}</dd>{/if}
        {#if message.date}<dt>Date</dt><dd>{fullDate(message.date)}</dd>{/if}
      </dl>
      {#if message.attachments.length}
        <ul class="attachments">
          {#each message.attachments as a}
            <li title={a.mimeType}>📎 {a.name} <span class="quiet">{fileSize(a.size)}</span></li>
          {/each}
        </ul>
      {/if}
    </header>
    {#if message.blockedRemote > 0 && !allowRemote}
      <div class="blocked">
        <span>Remote images are blocked to protect your privacy ({message.blockedRemote}).</span>
        <button onclick={loadImages} disabled={loading}>Load images</button>
      </div>
    {/if}
    <div class="body">
      {#if message.html}
        {#key message}
          <iframe
            title="Message content"
            sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
            srcdoc={frameDoc(message.html, allowRemote)}
            onload={(e) => wireFrame(e.currentTarget as HTMLIFrameElement)}
          ></iframe>
        {/key}
      {:else if message.text}
        <pre>{message.text}</pre>
      {:else}
        <p class="quiet pad">This message has no text.</p>
      {/if}
    </div>
    {/if}
  {/if}
</article>

<style>
  .reader {
    overflow-y: auto;
    min-width: 0;
  }
  .empty {
    height: 100%;
    display: grid;
    place-items: center;
  }
  .pad {
    padding: 16px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 10px 24px;
    border-bottom: 1px solid var(--border);
    position: sticky;
    top: 0;
    background: var(--bg);
    z-index: 1;
  }
  .actions button,
  .actions select {
    width: auto;
    padding: 5px 10px;
    font-size: 13px;
  }
  .actions button[aria-pressed="true"] {
    color: var(--accent-ink);
    border-color: var(--accent-ink);
  }
  header {
    padding: 18px 24px 12px;
    border-bottom: 1px solid var(--border);
  }
  h1 {
    font-size: 20px;
    margin: 0 0 10px;
    word-break: break-word;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 12px;
    margin: 0;
    font-size: 13px;
  }
  dt {
    color: var(--text-quiet);
  }
  dd {
    margin: 0;
    word-break: break-word;
  }
  .attachments {
    list-style: none;
    padding: 0;
    margin: 10px 0 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .attachments li {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 8px;
    font-size: 13px;
  }
  .blocked {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    background: var(--note-bg);
    padding: 8px 24px;
    font-size: 13px;
  }
  .body {
    padding: 0;
  }
  iframe {
    width: 100%;
    border: none;
    display: block;
    background: #fff;
    min-height: 200px;
  }
  pre {
    white-space: pre-wrap;
    word-wrap: break-word;
    font: 14px/1.5 var(--font);
    margin: 0;
    padding: 20px 24px;
  }
</style>
