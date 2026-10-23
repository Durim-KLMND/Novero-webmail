<script lang="ts" module>
  import type { SyncStage } from "../api";

  const CLOSED_KEY = "novero.closedFolders";

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
    /** Scheduled mail waiting for its time. */
    scheduled: number;
    outboxError: string | null;
    /** When the last sync finished without an error (ms since 1970). */
    synced: number | null;
    /** The running sync's progress, while it has a lot to download. */
    progress: { stage: SyncStage; done: number; total: number } | null;
  }
</script>

<script lang="ts">
  import type { Account, Folder, UpdateStatus } from "../api";
  import { downloadPercent } from "../todo/notes";
  import { errorText, isOffline, isSignInError } from "../api";
  import Icon, { type IconName } from "../ui/Icon.svelte";
  import { items, openMenu, toggleMenuUnder, type MenuItem } from "../shell/menus.svelte";
  import { folderLabel, folderPath } from "../format";

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
    /** Make a folder at the top of an account. */
    onNewFolder: (account: number) => void;
    /** The folder under a message being dragged, which takes it on drop. */
    dropTarget: number | null;
    onAllInboxes: () => void;
    /** Snoozed messages, shown under All inboxes while there are any. */
    snoozedCount: number;
    snoozedActive: boolean;
    onSnoozed: () => void;
    /** The search box changed; an empty query ends the search. */
    onSearch: (query: string) => void;
    /** Open an account's rules. */
    onRules: (account: number) => void;
    onSync: (account: number) => void;
    onRemove: (account: Account) => void;
    onSignInAgain: (account: Account) => void;
    /** Write a new message, here or in a window of its own. */
    onCompose: (newWindow: boolean) => void;
    onOutbox: (account: number) => void;
    /** A newer version, offered at the bottom once found. */
    update: UpdateStatus | null;
    onInstallUpdate: () => void;
    onDownloadUpdate: () => void;
    onWhatsNew: () => void;
  }
  let {
    accounts,
    folders,
    syncState,
    selected,
    allInboxes,
    onSelect,
    onFolderMenu,
    onNewFolder,
    dropTarget,
    onAllInboxes,
    snoozedCount,
    snoozedActive,
    onSnoozed,
    onSearch,
    onRules,
    onSync,
    onRemove,
    onSignInAgain,
    onCompose,
    onOutbox,
    update,
    onInstallUpdate,
    onDownloadUpdate,
    onWhatsNew,
  }: Props = $props();

  /** A refused password or an expired sign-in: fixed by signing in again. */
  function plural(n: number, one: string): string {
    return n === 1 ? `1 ${one}` : `${n.toLocaleString()} ${one}s`;
  }

  function accountMenu(account: Account): MenuItem[] {
    return items(
      { label: "Check for mail", icon: "refresh", run: () => onSync(account.id) },
      { label: "New folder…", icon: "new-folder", run: () => onNewFolder(account.id) },
      { label: "Rules…", icon: "rules", run: () => onRules(account.id) },
      { label: "Sign in again", icon: "sign-in", run: () => onSignInAgain(account) },
      "separator",
      { label: "Remove account", icon: "remove-account", danger: true, run: () => onRemove(account) },
    );
  }

  function moreMenu(account: Account, event: MouseEvent) {
    toggleMenuUnder(event.currentTarget as HTMLElement, accountMenu(account), true);
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

  /** Folders with subfolders can be closed; which ones stays for next time. */
  let closed = $state<Record<number, boolean>>(loadClosed());

  function loadClosed(): Record<number, boolean> {
    try {
      return JSON.parse(localStorage.getItem(CLOSED_KEY) ?? "{}") ?? {};
    } catch {
      return {};
    }
  }

  function toggleOpen(folder: Folder) {
    closed[folder.id] = !closed[folder.id];
    if (!closed[folder.id]) delete closed[folder.id];
    try {
      localStorage.setItem(CLOSED_KEY, JSON.stringify(closed));
    } catch {
      // Not kept; every folder shows open next time.
    }
  }

  /** Whether `inner` is somewhere inside `outer`. */
  function isInside(inner: Folder, outer: Folder): boolean {
    return !!inner.delimiter && inner.role === "other" && inner.name.startsWith(outer.name + inner.delimiter);
  }

  /** An account's folders as shown: each with whether it has subfolders,
   *  leaving out those inside a closed folder. */
  function tree(list: Folder[]): { folder: Folder; parent: boolean }[] {
    const shut = list.filter((f) => closed[f.id]);
    return list
      .filter((f) => !shut.some((c) => isInside(f, c)))
      .map((folder) => ({ folder, parent: list.some((f) => isInside(f, folder)) }));
  }
</script>

<nav class="sidebar" aria-label="Accounts and folders">
  <div class="compose" role="group" aria-label="New message">
    <button class="primary main" onclick={() => onCompose(false)} title="New message (C)">New message</button>
    <button
      class="primary more-compose"
      aria-label="More ways to write a message"
      aria-haspopup="menu"
      title="More ways to write a message"
      onclick={(e) =>
        toggleMenuUnder(
          e.currentTarget.parentElement ?? e.currentTarget,
          [
            { label: "New message", icon: "drafts", shortcut: "C", run: () => onCompose(false) },
            { label: "New message in a new window", icon: "external-link", shortcut: "Shift+C", run: () => onCompose(true) },
          ],
        )}
    ><Icon name="chevron-down" size={14} /></button>
  </div>
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
    <span class="keys" aria-hidden="true"><kbd>Ctrl</kbd><kbd>K</kbd></span>
  </div>
  <div class="scroll">
    {#if accounts.length > 1}
      <button class="folder all" class:active={allInboxes} onclick={onAllInboxes}>
        <Icon name="all-inboxes" />
        <span class="label">All inboxes</span>
        {#if unreadInboxes > 0}<span class="badge">{unreadInboxes.toLocaleString()}</span>{/if}
      </button>
    {/if}
    {#if snoozedCount > 0 || snoozedActive}
      <button class="folder all" class:active={snoozedActive} onclick={onSnoozed}>
        <Icon name="clock" />
        <span class="label">Snoozed</span>
        {#if snoozedCount > 0}<span class="count quiet">{snoozedCount.toLocaleString()}</span>{/if}
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
          {#if sync?.error && !isOffline(sync.error)}<span class="warn" title={errorText(sync.error)}>!</span>{/if}
          <button class="more" aria-label="Account options" aria-haspopup="menu" onclick={(e) => moreMenu(account, e)}><Icon name="more" /></button>
        </div>
        <!-- Being offline and changes waiting to sync show in the status bar,
             so reading mail doesn't push the folders around. -->
        {#if sync?.error && !isOffline(sync.error)}
          <p class="sync-error">{errorText(sync.error)}</p>
        {/if}
        {#if isSignInError(sync?.error) || isSignInError(sync?.pendingError)}
          <button class="outbox-line sign-in" onclick={() => onSignInAgain(account)}>Sign in again</button>
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
        {#if sync?.scheduled}
          <button class="outbox-line quiet" onclick={() => onOutbox(account.id)}>
            {plural(sync.scheduled, "message")} scheduled
          </button>
        {/if}
        {#if !collapsed[account.id]}
          <ul>
            {#each tree(folders[account.id] ?? []) as { folder, parent } (folder.id)}
              <li style:--indent="{16 + depth(folder) * 16}px">
                {#if parent}
                  <button
                    class="twisty"
                    aria-label={closed[folder.id] ? "Show the folders inside" : "Hide the folders inside"}
                    aria-expanded={!closed[folder.id]}
                    onclick={() => toggleOpen(folder)}
                  ><Icon name={closed[folder.id] ? "chevron-right" : "chevron-down"} size={12} /></button>
                {/if}
                <button
                  class="folder"
                  class:active={selected?.id === folder.id}
                  class:drop={dropTarget === folder.id}
                  data-folder={folder.id}
                  onclick={() => onSelect(folder)}
                  oncontextmenu={(e) => onFolderMenu(folder, e)}
                >
                  <Icon name={icons[folder.role]} />
                  <span class="label" title={folder.role === "other" ? folderPath(folder.name, folder.delimiter) : undefined}>{folderLabel(folder.name, folder.role, folder.delimiter)}</span>
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
  {#if update?.update && update.phase !== "idle" && update.phase !== "checking"}
    {@const version = update.update.version}
    {@const percent = downloadPercent(update.downloaded, update.total)}
    <div class="update" role="status">
      {#if update.phase === "available"}
        <span>Version {version} is out</span>
        <button class="primary" onclick={onDownloadUpdate}>Download</button>
      {:else if update.phase === "downloading"}
        <span>Downloading the update{percent != null ? ` · ${percent}%` : "…"}</span>
        <progress max="100" value={percent} aria-label="Download progress"></progress>
      {:else}
        <span>Version {version} is ready</span>
        <button class="primary" onclick={onInstallUpdate} disabled={update.phase === "installing"}>
          {update.phase === "installing" ? "Restarting…" : "Restart to update"}
        </button>
      {/if}
      <button class="link" onclick={onWhatsNew}>What's new</button>
    </div>
  {/if}
</nav>

<style>
  .sidebar {
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    min-height: 0;
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
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    position: relative;
  }
  ul .folder {
    padding-left: var(--indent);
  }
  /* Opens and closes a folder's subfolders, just left of its icon. */
  .twisty {
    position: absolute;
    left: calc(var(--indent) - 16px);
    top: 50%;
    transform: translateY(-50%);
    z-index: 1;
    display: flex;
    border: none;
    background: none;
    padding: 2px;
    color: var(--text-quiet);
  }
  /* A dragged message is over this folder. */
  .folder.drop {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    box-shadow: inset 0 0 0 1px var(--accent);
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
  .count {
    font-size: 12px;
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
  .update .link {
    justify-self: start;
    font-size: 12px;
  }
  .search {
    position: relative;
    padding: 0 12px 10px;
  }
  .search input {
    padding: 6px 10px;
  }
  /* The shortcut, shown in the empty box until it is used. */
  .keys {
    position: absolute;
    right: 20px;
    top: 6px;
    display: flex;
    gap: 3px;
    pointer-events: none;
  }
  .search:has(input:focus) .keys,
  .search:has(input:not(:placeholder-shown)) .keys {
    display: none;
  }
  kbd {
    font: 11px/1 var(--font);
    color: var(--text-quiet);
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 4px;
    padding: 3px 5px;
    background: var(--bg);
  }
  .all {
    margin-bottom: 8px;
  }
  /* The title bar already names the app, so the list starts here. */
  .compose {
    display: flex;
    margin: 12px 12px 12px;
  }
  .compose .main {
    flex: 1;
    min-width: 0;
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .more-compose {
    padding: 7px 9px;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
    border-left: 1px solid color-mix(in srgb, var(--accent-text) 30%, transparent);
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
</style>
