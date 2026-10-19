<script lang="ts" module>
  export interface SyncState {
    running: boolean;
    error: string | null;
    detail: string;
    /** Local changes not yet on the server. */
    pending: number;
    pendingError: string | null;
    /** Mail waiting to send, mail the server refused, and why the last try failed. */
    queued: number;
    failed: number;
    outboxError: string | null;
  }
</script>

<script lang="ts">
  import type { Account, Folder } from "./api";
  import { errorText } from "./api";

  /** A dropped connection is expected and recovers by itself, so it is not shown as an error. */
  function isOffline(error: string): boolean {
    return /^network error/i.test(error);
  }
  import { folderLabel } from "./format";

  interface Props {
    accounts: Account[];
    folders: Record<number, Folder[]>;
    syncState: Record<number, SyncState>;
    /** The folder shown, or null for all inboxes or a search. */
    selected: Folder | null;
    allInboxes: boolean;
    onSelect: (folder: Folder) => void;
    onAllInboxes: () => void;
    /** The search box changed; an empty query ends the search. */
    onSearch: (query: string) => void;
    onSettings: () => void;
    onHelp: () => void;
    onAddAccount: () => void;
    onSync: (account: number) => void;
    onRemove: (account: Account) => void;
    onSignInAgain: (account: Account) => void;
    onCompose: () => void;
    onOutbox: (account: number) => void;
  }
  let {
    accounts,
    folders,
    syncState,
    selected,
    allInboxes,
    onSelect,
    onAllInboxes,
    onSearch,
    onSettings,
    onHelp,
    onAddAccount,
    onSync,
    onRemove,
    onSignInAgain,
    onCompose,
    onOutbox,
  }: Props = $props();

  /** A refused password or an expired sign-in: fixed by signing in again. */
  function isSignInError(error: string | null | undefined): boolean {
    return !!error && /^authentication failed/i.test(error);
  }

  function plural(n: number, one: string): string {
    return n === 1 ? `1 ${one}` : `${n.toLocaleString()} ${one}s`;
  }

  let menuFor = $state<number | null>(null);
  let collapsed = $state<Record<number, boolean>>({});

  const icons: Record<string, string> = {
    inbox: "📥",
    sent: "📤",
    drafts: "📝",
    trash: "🗑",
    junk: "⚠",
    archive: "🗄",
    other: "📁",
  };

  /** Searching starts after a pause in typing. */
  const SEARCH_AFTER = 250;
  let query = $state("");
  let searchInput = $state<HTMLInputElement>();
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  function typed() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => onSearch(query.trim()), SEARCH_AFTER);
  }

  function searchKey(event: KeyboardEvent) {
    if (event.key === "Enter") {
      clearTimeout(searchTimer);
      onSearch(query.trim());
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      clearSearch();
      searchInput?.blur();
    }
  }

  export function focusSearch() {
    searchInput?.focus();
    searchInput?.select();
  }

  export function clearSearch() {
    clearTimeout(searchTimer);
    if (!query) return;
    query = "";
    onSearch("");
  }

  const unreadInboxes = $derived(
    Object.values(folders)
      .flat()
      .filter((f) => f.role === "inbox")
      .reduce((n, f) => n + f.unread, 0),
  );

  function depth(folder: Folder): number {
    if (folder.role !== "other" || !folder.delimiter) return 0;
    return folder.name.split(folder.delimiter).length - 1;
  }
</script>

<nav class="sidebar" aria-label="Accounts and folders">
  <div class="brand">Novero Mail</div>
  <button class="primary compose" onclick={onCompose} title="New message (C)">New message</button>
  <div class="search">
    <input
      type="search"
      bind:this={searchInput}
      bind:value={query}
      oninput={typed}
      onkeydown={searchKey}
      placeholder="Search mail"
      aria-label="Search mail"
      title="Search (/). Try from:, to:, subject:, is:unread, has:attachment, in:"
      spellcheck="false"
    />
  </div>
  <div class="scroll">
    {#if accounts.length > 1}
      <button class="folder all" class:active={allInboxes} onclick={onAllInboxes}>
        <span class="icon" aria-hidden="true">📥</span>
        <span class="label">All inboxes</span>
        {#if unreadInboxes > 0}<span class="badge">{unreadInboxes.toLocaleString()}</span>{/if}
      </button>
    {/if}
    {#each accounts as account (account.id)}
      {@const sync = syncState[account.id]}
      <section>
        <div class="account">
          <button
            class="account-name"
            onclick={() => (collapsed[account.id] = !collapsed[account.id])}
            aria-expanded={!collapsed[account.id]}
            title={account.email}
          >
            <span class="chevron">{collapsed[account.id] ? "▸" : "▾"}</span>
            <span class="names">
              <strong>{account.displayName}</strong>
              {#if account.displayName !== account.email}<small class="quiet">{account.email}</small>{/if}
            </span>
          </button>
          {#if sync?.running}<span class="spinner small" title={sync.detail}></span>{/if}
          {#if sync?.error && !isOffline(sync.error)}<span class="warn" title={errorText(sync.error)}>!</span>{/if}
          <button class="more" aria-label="Account options" onclick={() => (menuFor = menuFor === account.id ? null : account.id)}>⋯</button>
          {#if menuFor === account.id}
            <div class="menu" role="menu">
              <button role="menuitem" onclick={() => { menuFor = null; onSync(account.id); }}>Check for mail</button>
              <button role="menuitem" onclick={() => { menuFor = null; onSignInAgain(account); }}>Sign in again</button>
              <button role="menuitem" class="danger" onclick={() => { menuFor = null; onRemove(account); }}>Remove account</button>
            </div>
          {/if}
        </div>
        {#if sync?.error && isOffline(sync.error)}
          <p class="pending quiet" title={errorText(sync.error)}>Offline. Showing saved mail.</p>
        {:else if sync?.error}
          <p class="sync-error">{errorText(sync.error)}</p>
        {/if}
        {#if isSignInError(sync?.error) || isSignInError(sync?.pendingError)}
          <button class="outbox-line sign-in" onclick={() => onSignInAgain(account)}>Sign in again</button>
        {/if}
        {#if sync?.pending}
          <p class="pending quiet" title={sync.pendingError ? errorText(sync.pendingError) : undefined}>
            {sync.pending === 1 ? "1 change" : `${sync.pending.toLocaleString()} changes`} waiting to sync
          </p>
        {:else if sync?.pendingError}
          <p class="sync-error">{errorText(sync.pendingError)}</p>
        {/if}
        {#if sync?.failed}
          <button class="outbox-line failed" onclick={() => onOutbox(account.id)}>
            {plural(sync.failed, "message")} could not be sent
          </button>
        {/if}
        {#if sync?.queued}
          <button
            class="outbox-line quiet"
            onclick={() => onOutbox(account.id)}
            title={sync.outboxError ? errorText(sync.outboxError) : undefined}
          >
            {sync.outboxError ? `${plural(sync.queued, "message")} waiting to send` : `Sending ${plural(sync.queued, "message")}…`}
          </button>
        {/if}
        {#if !collapsed[account.id]}
          <ul>
            {#each folders[account.id] ?? [] as folder (folder.id)}
              <li>
                <button
                  class="folder"
                  class:active={selected?.id === folder.id}
                  style:padding-left="{12 + depth(folder) * 14}px"
                  onclick={() => onSelect(folder)}
                >
                  <span class="icon" aria-hidden="true">{icons[folder.role]}</span>
                  <span class="label">{folderLabel(folder.name, folder.role, folder.delimiter)}</span>
                  {#if folder.unread > 0 && folder.role !== "sent" && folder.role !== "trash"}
                    <span class="badge">{folder.unread.toLocaleString()}</span>
                  {/if}
                </button>
              </li>
            {:else}
              <li class="quiet empty">{sync?.running ? "Loading folders…" : "No folders yet"}</li>
            {/each}
          </ul>
        {/if}
      </section>
    {/each}
  </div>
  <div class="footer">
    <button class="add" onclick={onAddAccount}>＋ Add account</button>
    <button class="tool" onclick={onHelp} aria-label="Keyboard shortcuts" title="Keyboard shortcuts (?)">⌨</button>
    <button class="tool" onclick={onSettings} aria-label="Settings" title="Settings">⚙</button>
  </div>
</nav>

<style>
  .sidebar {
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .brand {
    font-weight: 700;
    font-size: 15px;
    padding: 16px 16px 8px;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 8px 8px;
  }
  section + section {
    margin-top: 12px;
  }
  .account {
    display: flex;
    align-items: center;
    gap: 4px;
    position: relative;
  }
  .account-name {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 4px;
    align-items: flex-start;
    text-align: left;
    border: none;
    background: none;
    padding: 6px 4px;
  }
  .names {
    display: grid;
    min-width: 0;
  }
  .names strong,
  .names small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chevron {
    width: 12px;
    color: var(--text-quiet);
  }
  .more {
    border: none;
    background: none;
    padding: 2px 6px;
  }
  .menu {
    position: absolute;
    right: 0;
    top: 100%;
    z-index: 5;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px;
    display: grid;
    min-width: 170px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12);
  }
  .menu button {
    border: none;
    text-align: left;
  }
  .menu .danger {
    color: var(--danger);
  }
  .warn {
    color: var(--danger);
    font-weight: 700;
    padding: 0 4px;
  }
  .sync-error {
    color: var(--danger);
    font-size: 12px;
    margin: 0 4px 6px 20px;
  }
  .pending {
    font-size: 12px;
    margin: 0 4px 6px 20px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .folder {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    border: none;
    background: none;
    text-align: left;
    padding: 6px 10px;
  }
  .folder.active {
    background: var(--bg-selected);
    font-weight: 600;
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icon {
    width: 18px;
    text-align: center;
  }
  .badge {
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-ink);
  }
  .empty {
    padding: 6px 12px 6px 24px;
    font-size: 13px;
  }
  .footer {
    display: flex;
    gap: 6px;
    margin: 8px;
  }
  .add {
    flex: 1;
    border-style: dashed;
    white-space: nowrap;
    padding: 7px 8px;
  }
  .tool {
    padding: 4px 10px;
    font-size: 16px;
  }
  .search {
    padding: 0 12px 10px;
  }
  .search input {
    padding: 6px 10px;
  }
  .all {
    margin-bottom: 8px;
  }
  .compose {
    margin: 4px 12px 12px;
  }
  .outbox-line {
    display: block;
    border: none;
    background: none;
    padding: 0;
    text-align: left;
    font-size: 12px;
    margin: 0 4px 6px 20px;
  }
  .outbox-line.failed {
    color: var(--danger);
  }
  .outbox-line.sign-in {
    color: var(--accent-ink, var(--brand));
    font-weight: 600;
  }
  .spinner.small {
    width: 12px;
    height: 12px;
  }
</style>
