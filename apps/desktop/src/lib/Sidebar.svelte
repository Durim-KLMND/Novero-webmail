<script lang="ts">
  import type { Account, Folder } from "./api";
  import { errorText } from "./api";
  import { folderLabel } from "./format";

  interface Props {
    accounts: Account[];
    folders: Record<number, Folder[]>;
    syncState: Record<number, { running: boolean; error: string | null; detail: string }>;
    selected: Folder | null;
    onSelect: (folder: Folder) => void;
    onAddAccount: () => void;
    onSync: (account: number) => void;
    onRemove: (account: Account) => void;
  }
  let { accounts, folders, syncState, selected, onSelect, onAddAccount, onSync, onRemove }: Props = $props();

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

  function depth(folder: Folder): number {
    if (folder.role !== "other" || !folder.delimiter) return 0;
    return folder.name.split(folder.delimiter).length - 1;
  }
</script>

<nav class="sidebar" aria-label="Accounts and folders">
  <div class="brand">Novero Mail</div>
  <div class="scroll">
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
          {#if sync?.error}<span class="warn" title={errorText(sync.error)}>!</span>{/if}
          <button class="more" aria-label="Account options" onclick={() => (menuFor = menuFor === account.id ? null : account.id)}>⋯</button>
          {#if menuFor === account.id}
            <div class="menu" role="menu">
              <button role="menuitem" onclick={() => { menuFor = null; onSync(account.id); }}>Check for mail</button>
              <button role="menuitem" class="danger" onclick={() => { menuFor = null; onRemove(account); }}>Remove account</button>
            </div>
          {/if}
        </div>
        {#if sync?.error}<p class="sync-error">{errorText(sync.error)}</p>{/if}
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
  <button class="add" onclick={onAddAccount}>＋ Add account</button>
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
    color: var(--accent);
  }
  .empty {
    padding: 6px 12px 6px 24px;
    font-size: 13px;
  }
  .add {
    margin: 8px;
    border-style: dashed;
  }
  .spinner.small {
    width: 12px;
    height: 12px;
  }
</style>
