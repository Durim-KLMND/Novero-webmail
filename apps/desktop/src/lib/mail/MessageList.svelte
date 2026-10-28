<script lang="ts">
  import { untrack } from "svelte";
  import Icon from "../ui/Icon.svelte";
  import { api, errorText, type Mailbox, type MessageSummary } from "../api";
  import { listDate, recipientLabel, senderName } from "../format";
  import { categoryOf } from "../cleanup/categories";
  import { groupReactions, reactionText } from "./reactions";

  interface Props {
    mailbox: Mailbox | null;
    title: string;
    /** Shown when the mailbox has no messages. */
    emptyText: string;
    /** What a row is, for the count: "message" or "conversation". */
    unit: string;
    /** A first sync is still downloading this list's mail. */
    arriving?: boolean;
    /** Bumped when the mailbox's contents may have changed. */
    version: number;
    /** A message of the selected row. */
    selected: number | null;
    /** Where a row's message is, for lists that mix folders or accounts. */
    place?: (item: MessageSummary) => string | null;
    onSelect: (item: MessageSummary) => void;
    /** The Delete key on the selected row. */
    onDelete: () => void;
    /** A right-click on a row. */
    onMenu?: (item: MessageSummary, event: MouseEvent) => void;
    /** A press on a row that may become a drag onto a folder. */
    onPress?: (item: MessageSummary, event: PointerEvent) => void;
  }
  let { mailbox, title, emptyText, unit, arriving = false, version, selected, place, onSelect, onDelete, onMenu, onPress }: Props =
    $props();

  const ROW = 64;
  const PAGE = 100;
  const OVERSCAN = 8;
  /** Search results stop here (MAX_RESULTS in mail-store). */
  const MAX_RESULTS = 5000;

  const key = $derived(mailbox ? JSON.stringify(mailbox) : "");
  let total = $state(0);
  let pages = $state<Record<number, MessageSummary[]>>({});
  let loading = new Set<number>();
  let scrollTop = $state(0);
  let viewport = $state(600);
  let error = $state("");
  let listEl = $state<HTMLDivElement>();
  let loadedFor = "";

  async function loadPage(page: number, box: Mailbox, force = false) {
    if (loading.has(page) || (!force && pages[page])) return;
    const forKey = JSON.stringify(box);
    loading.add(page);
    try {
      const result = await api.listMessages(box, page * PAGE, PAGE);
      if (key !== forKey) return;
      total = result.total;
      pages[page] = result.items;
      error = "";
    } catch (err) {
      if (key === forKey) error = errorText(err);
    } finally {
      loading.delete(page);
    }
  }

  // Another mailbox starts from the top.
  $effect(() => {
    if (key === loadedFor) return;
    loadedFor = key;
    pages = {};
    total = 0;
    error = "";
    loading = new Set();
    scrollTop = 0;
    if (listEl) listEl.scrollTop = 0;
    if (mailbox) loadPage(0, mailbox);
  });

  // New mail or changed flags: reload the pages already shown. Only `version`
  // and the mailbox start a reload; the pages it writes must not start another.
  $effect(() => {
    void version;
    const box = mailbox;
    if (!box) return;
    untrack(() => {
      const loaded = Object.keys(pages).map(Number);
      for (const page of loaded.length ? loaded : [0]) loadPage(page, box, true);
    });
  });

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(total, Math.ceil((scrollTop + viewport) / ROW) + OVERSCAN));

  $effect(() => {
    const box = mailbox;
    if (!box || total === 0) return;
    for (let page = Math.floor(first / PAGE); page <= Math.floor(Math.max(first, last - 1) / PAGE); page++) {
      loadPage(page, box);
    }
  });

  function itemAt(index: number): MessageSummary | undefined {
    return pages[Math.floor(index / PAGE)]?.[index % PAGE];
  }

  const rows = $derived.by(() => {
    const out: { index: number; item: MessageSummary | undefined }[] = [];
    for (let i = first; i < last; i++) out.push({ index: i, item: itemAt(i) });
    return out;
  });

  /** The row holding message `id`: a conversation keeps its row as replies arrive. */
  function indexOf(id: number | null): number {
    if (id == null) return -1;
    for (const [page, items] of Object.entries(pages)) {
      const i = items.findIndex((m) => m.ids.includes(id));
      if (i >= 0) return Number(page) * PAGE + i;
    }
    return -1;
  }

  /** The current version of the row holding message `id`. */
  export function find(id: number): MessageSummary | null {
    const index = indexOf(id);
    return index < 0 ? null : (itemAt(index) ?? null);
  }

  /** The row to select once the row holding `id` leaves the list: the next one, else the previous. */
  export function neighbour(id: number): MessageSummary | null {
    const index = indexOf(id);
    if (index < 0) return null;
    return itemAt(index + 1) ?? itemAt(index - 1) ?? null;
  }

  /** Selects the row `delta` rows from the selected one and scrolls it into view. */
  export function move(delta: number) {
    if (total === 0) return;
    const current = indexOf(selected);
    const next = current < 0 ? 0 : Math.min(total - 1, Math.max(0, current + delta));
    const item = itemAt(next);
    if (!item || !listEl) return;
    onSelect(item);
    const top = next * ROW;
    if (top < listEl.scrollTop) listEl.scrollTop = top;
    else if (top + ROW > listEl.scrollTop + listEl.clientHeight) listEl.scrollTop = top + ROW - listEl.clientHeight;
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Delete" && selected != null) {
      event.preventDefault();
      event.stopPropagation();
      onDelete();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    move(event.key === "ArrowDown" ? 1 : -1);
  }

  const outgoing = (item: MessageSummary) => item.folderRole === "sent" || item.folderRole === "drafts";

  const countText = $derived.by(() => {
    if (mailbox?.kind === "search") {
      if (total >= MAX_RESULTS) return `${MAX_RESULTS.toLocaleString()}+ results`;
      return total === 1 ? "1 result" : `${total.toLocaleString()} results`;
    }
    return total === 1 ? `1 ${unit}` : `${total.toLocaleString()} ${unit}s`;
  });
</script>

<section class="list-pane" aria-label="Messages">
  <header>
    {#if mailbox}
      <h2>{title}</h2>
      {#if pages[0]}<span class="quiet">{countText}</span>{/if}
    {/if}
  </header>
  {#if error}<p class="error">{error}</p>{/if}
  <div
    class="scroller"
    bind:this={listEl}
    bind:clientHeight={viewport}
    onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
    onkeydown={onKey}
    tabindex="0"
    role="listbox"
    aria-label="Message list"
  >
    {#if !mailbox}
      <p class="empty quiet">Choose a folder.</p>
    {:else if total === 0 && pages[0]}
      <p class="empty quiet">{arriving ? "Your mail is on its way." : emptyText}</p>
    {:else}
      <div class="spacer" style:height="{total * ROW}px">
        {#each rows as { index, item } (index)}
          <div class="row-slot" style:top="{index * ROW}px">
            {#if item}
              {@const active = selected != null && item.ids.includes(selected)}
              {@const where = place?.(item)}
              <button
                class="row"
                class:unread={!item.flags.seen}
                class:active
                role="option"
                aria-selected={active}
                onclick={() => onSelect(item)}
                oncontextmenu={(e) => onMenu?.(item, e)}
                onpointerdown={(e) => onPress?.(item, e)}
                tabindex="-1"
              >
                <span class="line1">
                  <span class="from">{outgoing(item) ? recipientLabel(item.to) : senderName(item.from)}</span>
                  {#if item.count > 1}<span class="count" title="{item.count} messages in this conversation">{item.count}</span>{/if}
                  {#if where}<span class="place" title={where}>{where}</span>{/if}
                  {#if item.category}
                    <span class="label cat-{item.category}" title="Category: {categoryOf[item.category].label}">
                      {categoryOf[item.category].label}
                    </span>
                  {/if}
                  <span class="date">{listDate(item.date)}</span>
                </span>
                <span class="line2">
                  {#if !item.flags.seen}<span class="dot" aria-label="Unread"></span>{/if}
                  <span class="subject">{item.subject || "(no subject)"}</span>
                  {#if item.reactions?.length}
                    {@const groups = groupReactions(item.reactions)}
                    <span class="reactions" title={reactionText(groups)}>
                      {#each groups.slice(0, 3) as g (g.emoji)}<span>{g.emoji}</span>{/each}
                    </span>
                  {/if}
                  {#if item.hasAttachments}<span class="clip"><Icon name="attachment" size={14} label="Has attachments" /></span>{/if}
                  {#if item.flags.answered}<span class="answered" title="Replied"><Icon name="reply" size={14} label="Replied" /></span>{/if}
                  {#if item.flags.flagged}<span class="flag"><Icon name="flag" size={14} filled label="Flagged" /></span>{/if}
                </span>
              </button>
            {:else}
              <div class="row placeholder"></div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</section>

<style>
  .list-pane {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--border);
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 14px 16px 10px;
    border-bottom: 1px solid var(--border);
    min-height: 50px;
  }
  h2 {
    font-size: 17px;
    margin: 0;
  }
  .scroller {
    flex: 1;
    overflow-y: auto;
    position: relative;
    outline: none;
  }
  .spacer {
    position: relative;
  }
  .row-slot {
    position: absolute;
    left: 0;
    right: 0;
    height: 64px;
  }
  .row {
    width: 100%;
    height: 100%;
    border: none;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    background: none;
    text-align: left;
    padding: 9px 16px;
    display: grid;
    gap: 3px;
  }
  .row.active {
    background: var(--bg-selected);
  }
  .placeholder {
    background: linear-gradient(90deg, var(--bg) 0%, var(--bg-hover) 50%, var(--bg) 100%);
    opacity: 0.5;
  }
  .line1,
  .line2 {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .from,
  .subject {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .subject {
    color: var(--text-quiet);
  }
  .unread .from,
  .unread .subject {
    font-weight: 600;
    color: var(--text);
  }
  .reactions {
    flex: none;
    font-size: 13px;
    line-height: 16px;
    letter-spacing: 1px;
  }
  .count {
    font-size: 12px;
    color: var(--text-quiet);
    border: 1px solid var(--border);
    border-radius: 9px;
    padding: 0 6px;
    line-height: 16px;
    flex: none;
  }
  .place {
    font-size: 11px;
    color: var(--text-quiet);
    background: var(--bg-hover);
    border-radius: 4px;
    padding: 0 5px;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: none;
  }
  .label {
    font-size: 11px;
    font-weight: 500;
    color: var(--cat);
    background: color-mix(in srgb, var(--cat) 12%, transparent);
    border-radius: 4px;
    padding: 0 5px;
    line-height: 16px;
    white-space: nowrap;
    flex: none;
  }
  .date {
    font-size: 12px;
    color: var(--text-quiet);
    white-space: nowrap;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent-ink);
    flex: none;
  }
  .clip,
  .answered {
    display: flex;
    color: var(--text-quiet);
  }
  .flag {
    display: flex;
    color: var(--accent-ink);
  }
  .empty {
    padding: 24px 16px;
  }
  .error {
    margin: 8px;
  }
</style>
