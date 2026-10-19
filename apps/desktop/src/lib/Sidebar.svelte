<script lang="ts" module>
  import type { SyncStage } from "./api";

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
    /** The running sync's progress, while it has a lot to download. */
    progress: { stage: SyncStage; done: number; total: number } | null;
  }
</script>

<script lang="ts">
  import type { Account, Folder, UpdateInfo } from "./api";
  import { errorText } from "./api";
  import Icon, { type IconName } from "./Icon.svelte";
  import { items, openMenu, openMenuAt, type MenuItem } from "./menus.svelte";

  /** A dropped connection is expected and recovers by itself, so it is not shown as an error. */
  function isOffline(error: string): boolean {
    return /^network error/i.test(error);
  }
  import { folderLabel, progressLabel } from "./format";

  interface Props {
    accounts: Account[];
    folders: Record<number, Folder[]>;
    syncState: Record<number, SyncState>;
    /** The folder shown, or null for all inboxes or a search. */
    selected: Folder | null;
    allInboxes: boolean;
    onSelect: (folder: Folder) => void;
    /** A right-click on a folder. */
    onFolderMenu: (folder: Folder, event: MouseEvent) => void;
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
    /** A downloaded update, offered at the bottom. */
    update: UpdateInfo | null;
    onInstallUpdate: () => void;
  }
  let {
    accounts,
    folders,
    syncState,
    selected,
    allInboxes,
    onSelect,
    onFolderMenu,
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
    update,
    onInstallUpdate,
  }: Props = $props();

  /** A refused password or an expired sign-in: fixed by signing in again. */
  function isSignInError(error: string | null | undefined): boolean {
    return !!error && /^authentication failed/i.test(error);
  }

  function plural(n: number, one: string): string {
    return n === 1 ? `1 ${one}` : `${n.toLocaleString()} ${one}s`;
  }

  function accountMenu(account: Account): MenuItem[] {
    return items(
      { label: "Check for mail", icon: "refresh", run: () => onSync(account.id) },
      { label: "Sign in again", icon: "sign-in", run: () => onSignInAgain(account) },
      "separator",
      { label: "Remove account", icon: "remove-account", danger: true, run: () => onRemove(account) },
    );
  }

  function moreMenu(account: Account, event: MouseEvent) {
    const at = (event.currentTarget as HTMLElement).getBoundingClientRect();
    openMenuAt(at.left, at.bottom + 2, accountMenu(account));
  }
  let collapsed = $state<Record<number, boolean>>({});

  const icons: Record<string, IconName> = {
    inbox: "inbox",
    sent: "sent",
    drafts: "drafts",
    trash: "trash",
    junk: "junk",
    archive: "archive",
    other: "folder",
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
        <Icon name="all-inboxes" />
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
            oncontextmenu={(e) => openMenu(e, accountMenu(account))}
            aria-expanded={!collapsed[account.id]}
            title={account.email}
          >
            <span class="chevron"><Icon name={collapsed[account.id] ? "chevron-right" : "chevron-down"} size={14} /></span>
            <span class="names">
              <strong>{account.displayName}</strong>
              {#if account.displayName !== account.email}<small class="quiet">{account.email}</small>{/if}
            </span>
          </button>
          {#if sync?.running}<span class="spinner small" title={sync.detail}></span>{/if}
          {#if sync?.error && !isOffline(sync.error)}<span class="warn" title={errorText(sync.error)}>!</span>{/if}
          <button class="more" aria-label="Account options" aria-haspopup="menu" onclick={(e) => moreMenu(account, e)}><Icon name="more" /></button>
        </div>
        {#if sync?.running && sync.progress}
          {@const p = sync.progress}
          <div class="progress" title={sync.detail}>
            <progress max={p.total} value={p.done} aria-label="Sync progress"></progress>
            <small class="quiet">{progressLabel(p.stage, p.done, p.total, true)}</small>
          </div>
        {/if}
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
                  oncontextmenu={(e) => onFolderMenu(folder, e)}
                >
                  <Icon name={icons[folder.role]} />
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
  {#if update}
    <div class="update" title={update.notes ?? ""}>
      <span>Version {update.version} is ready</span>
      <button class="primary" onclick={onInstallUpdate}>Restart to update</button>
    </div>
  {/if}
  <div class="footer">
    <button class="add" onclick={onAddAccount}><Icon name="plus" /> Add account</button>
    <button class="tool" onclick={onHelp} aria-label="Keyboard shortcuts" title="Keyboard shortcuts (?)"><Icon name="keyboard" size={18} /></button>
    <button class="tool" onclick={onSettings} aria-label="Settings" title="Settings"><Icon name="settings" size={18} /></button>
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
    display: flex;
    color: var(--text-quiet);
  }
  .more {
    border: none;
    background: none;
    padding: 3px 5px;
    color: var(--text-quiet);
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
  .progress {
    display: grid;
    gap: 2px;
    margin: 0 4px 6px 20px;
  }
  .progress small {
    font-size: 12px;
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
  .folder :global(.icon) {
    color: var(--text-quiet);
  }
  .folder.active :global(.icon) {
    color: var(--accent-ink);
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
  .update {
    display: grid;
    gap: 6px;
    margin: 8px 8px 0;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
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
    padding: 6px 9px;
    color: var(--text-quiet);
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
