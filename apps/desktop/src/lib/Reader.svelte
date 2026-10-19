<script lang="ts">
  import Icon from "./Icon.svelte";
  import { api, errorText, type FlagKind, type Folder, type MessageSummary } from "./api";
  import { folderLabel } from "./format";
  import MessageCard from "./MessageCard.svelte";
  import { openMenuAt } from "./contextmenu.svelte";

  interface Props {
    /** The selected row of the message list: a message or a conversation. */
    row: MessageSummary | null;
    /** The folders of the row's account. */
    folders: Folder[];
    /** Bumped when messages may have changed. */
    version: number;
    /** A message was read. */
    onOpened: () => void;
    onArchive: () => void;
    onDelete: () => void;
    onToggle: (flag: FlagKind, value: boolean) => void;
    onMove: (folder: number) => void;
    onReply: (message: number, all: boolean) => void;
    onForward: (message: number) => void;
  }
  let { row, folders, version, onOpened, onArchive, onDelete, onToggle, onMove, onReply, onForward }: Props =
    $props();

  const rowId = $derived(row?.id ?? null);
  const canArchive = $derived(row?.folderRole !== "archive" && folders.some((f) => f.role === "archive"));
  const moveTargets = $derived(folders.filter((f) => f.id !== row?.folderId));

  function moveMenu(event: MouseEvent) {
    const at = (event.currentTarget as HTMLElement).getBoundingClientRect();
    openMenuAt(
      at.left,
      at.bottom + 2,
      moveTargets.map((f) => ({
        label: folderLabel(f.name, f.role, f.delimiter),
        icon: f.role === "other" ? "folder" : f.role,
        run: () => onMove(f.id),
      })),
    );
  }

  /** The conversation, oldest first. */
  let messages = $state<MessageSummary[]>([]);
  let expanded = $state<Record<number, boolean>>({});
  let error = $state("");
  let loadedFor: number | null = null;
  let request = 0;

  /** The message the toolbar's Reply and Forward answer: the newest one that isn't a draft. */
  const latest = $derived(
    [...messages].reverse().find((m) => m.folderRole !== "drafts" && m.folderRole !== "sent") ??
      [...messages].reverse().find((m) => m.folderRole !== "drafts") ??
      null,
  );

  /** The row's messages as last loaded: their flags are newer than the row's. */
  const own = $derived(row ? messages.filter((m) => row.ids.includes(m.id)) : []);
  const seen = $derived(own.length ? own.every((m) => m.flags.seen) : !!row?.flags.seen);
  const flagged = $derived(own.length ? own.some((m) => m.flags.flagged) : !!row?.flags.flagged);

  /** Whether the row is read and flagged, for the S and Shift+U shortcuts. */
  export function rowFlags(): { seen: boolean; flagged: boolean } {
    return { seen, flagged };
  }

  /** What R, A and F answer. */
  export function replyTarget(): number | null {
    return latest?.id ?? null;
  }

  async function load(id: number) {
    const mine = ++request;
    const fresh = loadedFor !== id;
    try {
      const result = await api.conversation(id);
      if (mine !== request) return;
      if (fresh) expanded = {};
      // The newest message and anything unread start open; read ones fold away.
      for (const m of result) {
        if (!m.flags.seen && !(m.id in expanded)) expanded[m.id] = true;
      }
      const newest = result[result.length - 1];
      if (newest && fresh) expanded[newest.id] = true;
      if (!result.some((m) => expanded[m.id]) && newest) expanded[newest.id] = true;
      messages = result;
      loadedFor = id;
      error = "";
    } catch (err) {
      if (mine === request) error = errorText(err);
    }
  }

  $effect(() => {
    const id = rowId;
    if (id == null) {
      messages = [];
      loadedFor = null;
      return;
    }
    if (id !== loadedFor) messages = [];
    load(id);
  });

  // Replies arriving or flags changing elsewhere.
  $effect(() => {
    void version;
    if (loadedFor != null) load(loadedFor);
  });

  function opened(summary: MessageSummary) {
    const m = messages.find((x) => x.id === summary.id);
    if (m && !m.flags.seen) {
      m.flags.seen = true;
      onOpened();
    }
  }

</script>

<article class="reader" aria-label="Conversation">
  {#if !row}
    <div class="empty quiet">Select a message to read it.</div>
  {:else}
    <div class="actions" role="toolbar" aria-label="Message actions">
      {#if latest}
        <button onclick={() => onReply(latest.id, false)} title="Reply (R)"><Icon name="reply" />Reply</button>
        <button onclick={() => onReply(latest.id, true)} title="Reply all (A)"><Icon name="reply-all" />Reply all</button>
        <button onclick={() => onForward(latest.id)} title="Forward (F)"><Icon name="forward" />Forward</button>
        <span class="gap"></span>
      {/if}
      {#if canArchive}<button onclick={onArchive} title="Archive (E)" aria-label="Archive"><Icon name="archive" /><span class="text">Archive</span></button>{/if}
      <button onclick={onDelete} title="Delete (#)" aria-label={row.folderRole === "trash" ? "Delete forever" : "Delete"}>
        <Icon name="trash" /><span class="text">{row.folderRole === "trash" ? "Delete forever" : "Delete"}</span>
      </button>
      <button
        onclick={() => onToggle("seen", !seen)}
        title={seen ? "Mark unread (Shift+U)" : "Mark read (Shift+I)"}
        aria-label={seen ? "Mark unread" : "Mark read"}
      >
        <Icon name={seen ? "unread" : "read"} /><span class="text">{seen ? "Mark unread" : "Mark read"}</span>
      </button>
      <button aria-pressed={flagged} onclick={() => onToggle("flagged", !flagged)} title="Flag (S)" aria-label="Flag">
        <Icon name="flag" filled={flagged} /><span class="text">{flagged ? "Flagged" : "Flag"}</span>
      </button>
      {#if moveTargets.length}
        <button aria-haspopup="menu" title="Move to folder" aria-label="Move to folder" onclick={moveMenu}>
          <Icon name="move" /><span class="text">Move to</span><Icon name="chevron-down" size={14} />
        </button>
      {/if}
    </div>
    <header>
      <h1>{row.subject || "(no subject)"}</h1>
      {#if messages.length > 1}<span class="quiet">{messages.length} messages</span>{/if}
    </header>
    {#if error}
      <div class="pad">
        <p class="error">Could not open this conversation: {error}</p>
        <button onclick={() => rowId != null && load(rowId)}>Try again</button>
      </div>
    {:else if !messages.length}
      <div class="empty"><span class="spinner"></span></div>
    {:else}
      <div class="thread">
        {#each messages as m (m.id)}
          <MessageCard
            summary={m}
            expanded={!!expanded[m.id]}
            showFolder={m.folderId !== row.folderId}
            onExpand={(open) => (expanded[m.id] = open)}
            onOpened={opened}
            onReply={(all) => onReply(m.id, all)}
            onForward={() => onForward(m.id)}
          />
        {/each}
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
  .actions button {
    width: auto;
    padding: 5px 10px;
    font-size: 13px;
  }
  /* In a narrow pane the actions after Forward show only their icons. */
  .reader {
    container-type: inline-size;
  }
  @container (max-width: 760px) {
    .actions .text {
      display: none;
    }
  }
  .gap {
    width: 8px;
  }
  .actions button[aria-pressed="true"] {
    color: var(--accent-ink);
    border-color: var(--accent-ink);
  }
  header {
    padding: 18px 24px 4px;
    display: flex;
    align-items: baseline;
    gap: 12px;
  }
  h1 {
    font-size: 20px;
    margin: 0;
    word-break: break-word;
  }
  .thread {
    display: grid;
    gap: 8px;
    padding: 12px 24px 24px;
  }
</style>
