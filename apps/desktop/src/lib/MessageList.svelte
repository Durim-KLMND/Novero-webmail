<script lang="ts">
  import { api, errorText, type Folder, type MessageSummary } from "./api";
  import { folderLabel, listDate, senderName } from "./format";

  interface Props {
    folder: Folder | null;
    /** Bumped when the folder's contents may have changed. */
    version: number;
    selected: number | null;
    onSelect: (id: number) => void;
  }
  let { folder, version, selected, onSelect }: Props = $props();

  const ROW = 64;
  const PAGE = 100;
  const OVERSCAN = 8;

  let total = $state(0);
  let pages = $state<Record<number, MessageSummary[]>>({});
  let loading = new Set<number>();
  let scrollTop = $state(0);
  let viewport = $state(600);
  let error = $state("");
  let listEl = $state<HTMLDivElement>();
  let loadedFor = -1;

  async function loadPage(page: number, folderId: number, force = false) {
    if (loading.has(page) || (!force && pages[page])) return;
    loading.add(page);
    try {
      const result = await api.listMessages(folderId, page * PAGE, PAGE);
      if (folder?.id !== folderId) return;
      total = result.total;
      pages[page] = result.items;
      error = "";
    } catch (err) {
      error = errorText(err);
    } finally {
      loading.delete(page);
    }
  }

  // A new folder starts from the top.
  $effect(() => {
    const id = folder?.id ?? -1;
    if (id === loadedFor) return;
    loadedFor = id;
    pages = {};
    total = 0;
    loading = new Set();
    scrollTop = 0;
    if (listEl) listEl.scrollTop = 0;
    if (id >= 0) loadPage(0, id);
  });

  // New mail or changed flags: reload the pages already shown.
  $effect(() => {
    void version;
    const id = folder?.id;
    if (id == null) return;
    const loaded = Object.keys(pages).map(Number);
    for (const page of loaded.length ? loaded : [0]) loadPage(page, id, true);
  });

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(total, Math.ceil((scrollTop + viewport) / ROW) + OVERSCAN));

  $effect(() => {
    const id = folder?.id;
    if (id == null || total === 0) return;
    for (let page = Math.floor(first / PAGE); page <= Math.floor(Math.max(first, last - 1) / PAGE); page++) {
      loadPage(page, id);
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

  function indexOfSelected(): number {
    for (const [page, items] of Object.entries(pages)) {
      const i = items.findIndex((m) => m.id === selected);
      if (i >= 0) return Number(page) * PAGE + i;
    }
    return -1;
  }

  function onKey(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const current = indexOfSelected();
    const next = Math.min(total - 1, Math.max(0, current + (event.key === "ArrowDown" ? 1 : -1)));
    const item = itemAt(next);
    if (!item || !listEl) return;
    onSelect(item.id);
    const top = next * ROW;
    if (top < listEl.scrollTop) listEl.scrollTop = top;
    else if (top + ROW > listEl.scrollTop + listEl.clientHeight) listEl.scrollTop = top + ROW - listEl.clientHeight;
  }
</script>

<section class="list-pane" aria-label="Messages">
  <header>
    {#if folder}
      <h2>{folderLabel(folder.name, folder.role, folder.delimiter)}</h2>
      <span class="quiet">{total.toLocaleString()} {total === 1 ? "message" : "messages"}</span>
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
    {#if !folder}
      <p class="empty quiet">Choose a folder.</p>
    {:else if total === 0 && pages[0]}
      <p class="empty quiet">This folder is empty.</p>
    {:else}
      <div class="spacer" style:height="{total * ROW}px">
        {#each rows as { index, item } (index)}
          <div class="row-slot" style:top="{index * ROW}px">
            {#if item}
              <button
                class="row"
                class:unread={!item.flags.seen}
                class:active={item.id === selected}
                role="option"
                aria-selected={item.id === selected}
                onclick={() => onSelect(item.id)}
                tabindex="-1"
              >
                <span class="line1">
                  <span class="from">{senderName(item.from)}</span>
                  <span class="date">{listDate(item.date)}</span>
                </span>
                <span class="line2">
                  {#if !item.flags.seen}<span class="dot" aria-label="Unread"></span>{/if}
                  <span class="subject">{item.subject || "(no subject)"}</span>
                  {#if item.hasAttachments}<span class="clip" aria-label="Has attachments">📎</span>{/if}
                  {#if item.flags.flagged}<span class="flag" aria-label="Flagged">⚑</span>{/if}
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
  .empty {
    padding: 24px 16px;
  }
  .error {
    margin: 8px;
  }
</style>
