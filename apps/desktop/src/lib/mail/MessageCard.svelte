<script lang="ts">
  import Icon from "../ui/Icon.svelte";
  import InviteCard from "./InviteCard.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { closeMenu, fallbackMenu, items, openMenu } from "../shell/menus.svelte";
  import { api, errorText, type Attachment, type MessageSummary, type ParsedMessage } from "../api";
  import { fileSize, folderLabel, fullDate, listDate, senderName } from "../format";
  import { toast } from "../shell/toast.svelte";
  import { viewKind } from "./viewer";
  import { forgetLinks, watchLinks } from "./hoverlink.svelte";

  interface Props {
    summary: MessageSummary;
    expanded: boolean;
    /** Shown next to the sender when the message is in another folder than the list. */
    showFolder: boolean;
    onExpand: (expanded: boolean) => void;
    /** The message was downloaded and shown; it is read now. */
    onOpened: (summary: MessageSummary) => void;
    onReply: (all: boolean) => void;
    onForward: () => void;
    /** Shows one of the message's attachments in the reading pane. */
    onOpenAttachment: (attachments: Attachment[], index: number) => void;
  }
  let { summary, expanded, showFolder, onExpand, onOpened, onReply, onForward, onOpenAttachment }: Props = $props();

  let message = $state<ParsedMessage | null>(null);
  let loading = $state(false);
  let error = $state("");
  const offline = $derived(error.startsWith("Network error"));
  let allowRemote = $state(false);
  let request = 0;
  const isDraft = $derived(summary.folderRole === "drafts");
  /** The shown message's document, whose hovered link must not outlive it. */
  let shownDoc: Document | null = null;

  $effect(() => () => forgetLinks(shownDoc));

  async function saveAttachment(index: number) {
    if (!message) return;
    try {
      if (await api.saveAttachment(summary.id, index)) toast(`Saved ${message.attachments[index].name}`);
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  function attachmentMenu(event: MouseEvent, index: number) {
    event.preventDefault();
    event.stopPropagation();
    openMenu(
      event,
      items(
        !!viewKind(message!.attachments[index]) && { label: "Open", icon: "template", run: () => onOpenAttachment(message!.attachments, index) },
        { label: "Save as…", icon: "save", run: () => saveAttachment(index) },
      ),
    );
  }

  async function load(remote: boolean) {
    const mine = ++request;
    loading = true;
    error = "";
    try {
      const result = await api.openMessage(summary.id, remote);
      if (mine !== request) return;
      message = result;
      onOpened(summary);
    } catch (err) {
      if (mine === request) error = errorText(err);
    } finally {
      if (mine === request) loading = false;
    }
  }

  $effect(() => {
    if (expanded && !message && !loading && !error) load(allowRemote);
  });

  function loadImages() {
    allowRemote = true;
    load(true);
  }

  /** The message HTML in its own document: no scripts, no network unless images are allowed. */
  function frameDoc(html: string, remote: boolean): string {
    const images = remote ? "data: https: http:" : "data:";
    return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${images}; style-src 'unsafe-inline'; font-src data:">
<base target="_blank">
<style>body{margin:0;padding:16px 20px;font:14px/1.5 "Anthropic Sans","Segoe UI",system-ui,sans-serif;color:#1a1a19;background:#fff;word-wrap:break-word}img{max-width:100%;height:auto}::-webkit-scrollbar{width:10px;height:10px}::-webkit-scrollbar-track,::-webkit-scrollbar-corner{background:transparent}::-webkit-scrollbar-thumb{background:rgba(26,26,25,.22);background-clip:padding-box;border:2px solid transparent;border-radius:999px}::-webkit-scrollbar-thumb:hover{background-color:rgba(26,26,25,.4)}</style>
</head><body>${html}</body></html>`;
  }

  /** Links open in the default browser instead of inside the app. */
  function wireFrame(frame: HTMLIFrameElement) {
    const doc = frame.contentDocument;
    if (!doc) return;
    forgetLinks(shownDoc);
    shownDoc = doc;
    watchLinks(doc, () => {
      const at = frame.getBoundingClientRect();
      return { x: at.left, y: at.top };
    });
    doc.addEventListener("click", (event) => {
      const link = (event.target as Element | null)?.closest?.("a[href]");
      if (!link) return;
      event.preventDefault();
      const href = link.getAttribute("href") ?? "";
      if (/^(https?:|mailto:)/i.test(href)) openUrl(href).catch(() => {});
    });
    // The frame is a document of its own, so right-clicks and clicks in it
    // don't reach the app's menu by themselves.
    doc.addEventListener("contextmenu", (event) => {
      const at = frame.getBoundingClientRect();
      fallbackMenu(event, doc, { x: at.left, y: at.top });
    });
    doc.addEventListener("pointerdown", (event) => {
      if (event.button !== 2) closeMenu();
    });
    frame.style.height = `${Math.max(doc.documentElement.scrollHeight, 120)}px`;
  }
</script>

<div class="card" class:expanded>
  <button
    class="head"
    onclick={() => onExpand(!expanded)}
    oncontextmenu={(e) =>
      openMenu(
        e,
        items(
          { label: expanded ? "Collapse" : "Expand", icon: expanded ? "chevron-down" : "chevron-right", run: () => onExpand(!expanded) },
          "separator",
          !isDraft && { label: "Reply", icon: "reply", run: () => onReply(false) },
          !isDraft && { label: "Reply all", icon: "reply-all", run: () => onReply(true) },
          !isDraft && { label: "Forward", icon: "forward", run: onForward },
        ),
      )}
    aria-expanded={expanded}
  >
    {#if !summary.flags.seen}<span class="dot" aria-label="Unread"></span>{/if}
    <span class="from">{senderName(summary.from)}</span>
    {#if isDraft}<span class="tag draft">Draft</span>
    {:else if showFolder}<span class="tag">{folderLabel(summary.folderName, summary.folderRole, null)}</span>{/if}
    {#if summary.hasAttachments}<span class="quiet clip"><Icon name="attachment" size={14} label="Has attachments" /></span>{/if}
    <span class="date quiet" title={fullDate(summary.date)}>{expanded ? fullDate(summary.date) : listDate(summary.date)}</span>
  </button>

  {#if expanded}
    {#if error}
      <div class="pad">
        <p class="error">
          {#if offline}
            This message hasn't been downloaded yet and the server can't be reached. It will open once you're back online.
          {:else}
            Could not open this message: {error}
          {/if}
        </p>
        <button onclick={() => load(allowRemote)} disabled={loading}>Try again</button>
      </div>
    {:else if !message}
      <div class="pad"><span class="spinner"></span></div>
    {:else}
      <dl class="selectable">
        <dt>From</dt><dd>{message.from ?? "(unknown)"}</dd>
        {#if message.to}<dt>To</dt><dd>{message.to}</dd>{/if}
        {#if message.cc}<dt>Cc</dt><dd>{message.cc}</dd>{/if}
      </dl>
      {#if message.invite}
        <InviteCard id={summary.id} />
      {/if}
      {#if message.attachments.length}
        <ul class="attachments">
          {#each message.attachments as a, i}
            <li>
              <button
                class="open"
                title={viewKind(a) ? `Open ${a.name}` : `${a.name} (${a.mimeType})`}
                onclick={() => onOpenAttachment(message!.attachments, i)}
                oncontextmenu={(e) => attachmentMenu(e, i)}
              >
                <Icon name={viewKind(a) === "image" ? "image" : viewKind(a) ? "template" : "attachment"} size={14} />
                <span class="file">{a.name}</span>
                <span class="quiet">{fileSize(a.size)}</span>
              </button>
              <button class="save" title="Save {a.name}" aria-label="Save {a.name}" onclick={() => saveAttachment(i)}>
                <Icon name="save" size={14} />
              </button>
            </li>
          {/each}
        </ul>
      {/if}
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
          <pre class="selectable">{message.text}</pre>
        {:else}
          <p class="quiet pad">This message has no text.</p>
        {/if}
      </div>
      {#if !isDraft}
        <div class="replies">
          <button onclick={() => onReply(false)}><Icon name="reply" />Reply</button>
          <button onclick={() => onReply(true)}><Icon name="reply-all" />Reply all</button>
          <button onclick={onForward}><Icon name="forward" />Forward</button>
        </div>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .card {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--bg);
  }
  .head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    border: none;
    border-radius: 0;
    background: none;
    text-align: left;
    padding: 10px 16px;
  }
  .from {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .date {
    font-size: 12px;
    white-space: nowrap;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent-ink);
    flex: none;
  }
  .tag {
    font-size: 11px;
    color: var(--text-quiet);
    background: var(--bg-hover);
    border-radius: 4px;
    padding: 0 5px;
  }
  .tag.draft {
    color: var(--danger);
  }
  .pad {
    padding: 12px 16px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 12px;
    margin: 0;
    padding: 0 16px 10px;
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
    padding: 0 16px 10px;
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .attachments li {
    display: flex;
    align-items: stretch;
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 13px;
    max-width: 100%;
    overflow: hidden;
  }
  .attachments button {
    border: none;
    border-radius: 0;
    background: none;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }
  .attachments button:hover {
    background: var(--bg-hover);
  }
  .attachments .open {
    padding: 4px 8px;
    min-width: 0;
  }
  .attachments .file {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .attachments .save {
    padding: 4px 7px;
    border-left: 1px solid var(--border);
    color: var(--text-quiet);
  }
  .blocked {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    background: var(--note-bg);
    padding: 8px 16px;
    font-size: 13px;
  }
  .body {
    border-top: 1px solid var(--border);
  }
  iframe {
    width: 100%;
    border: none;
    display: block;
    /* Mail is written for a white page, so HTML keeps one in dark mode too. */
    background: #fff;
    min-height: 120px;
  }
  pre {
    white-space: pre-wrap;
    word-wrap: break-word;
    font: 14px/1.5 var(--font);
    margin: 0;
    padding: 16px;
  }
  .replies {
    display: flex;
    gap: 6px;
    padding: 10px 16px;
    border-top: 1px solid var(--border);
  }
  .replies button {
    padding: 4px 10px;
    font-size: 13px;
  }
</style>
