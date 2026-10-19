<script lang="ts">
  import { onDestroy } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api,
    errorText,
    onMailEvent,
    type Account,
    type FlagKind,
    type Folder,
    type MailEvent,
    type MessageSummary,
  } from "./api";
  import Sidebar, { type SyncState } from "./Sidebar.svelte";
  import MessageList from "./MessageList.svelte";
  import Reader from "./Reader.svelte";

  interface Props {
    accounts: Account[];
    onAddAccount: () => void;
    onAccountsChanged: () => void;
  }
  let { accounts, onAddAccount, onAccountsChanged }: Props = $props();

  let folders = $state<Record<number, Folder[]>>({});
  let syncState = $state<Record<number, SyncState>>({});
  let selectedFolder = $state<Folder | null>(null);
  let selected = $state<MessageSummary | null>(null);
  let listVersion = $state(0);
  let notice = $state("");
  let list = $state<ReturnType<typeof MessageList>>();

  const accountFolders = $derived(selectedFolder ? (folders[selectedFolder.accountId] ?? []) : []);

  function stateOf(account: number): SyncState {
    return syncState[account] ?? { running: false, error: null, detail: "", pending: 0, pendingError: null };
  }

  async function loadFolders(accountId: number) {
    folders[accountId] = await api.listFolders(accountId);
    if (!selectedFolder) {
      selectedFolder = folders[accountId].find((f) => f.role === "inbox") ?? folders[accountId][0] ?? null;
    } else {
      const fresh = folders[selectedFolder.accountId]?.find((f) => f.id === selectedFolder!.id);
      if (fresh) selectedFolder = fresh;
    }
  }

  $effect(() => {
    for (const account of accounts) {
      if (!(account.id in folders)) {
        loadFolders(account.id);
        api.pendingChanges(account.id).then((pending) => {
          syncState[account.id] = { ...stateOf(account.id), pending };
        });
      }
    }
  });

  // Refresh at most every 700 ms while a sync streams in.
  const pending = new Set<number>();
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  function scheduleRefresh(accountId: number) {
    pending.add(accountId);
    if (refreshTimer) return;
    refreshTimer = setTimeout(async () => {
      refreshTimer = undefined;
      const ids = [...pending];
      pending.clear();
      await Promise.all(ids.map(loadFolders));
      if (selectedFolder && ids.includes(selectedFolder.accountId)) listVersion += 1;
    }, 700);
  }

  const unlisten = onMailEvent((e: MailEvent) => {
    const current = stateOf(e.account);
    if (e.type === "syncStarted") {
      syncState[e.account] = { ...current, running: true, error: null, detail: "Checking for mail…" };
    } else if (e.type === "syncProgress") {
      const detail = e.total > 0 ? `${e.folder}: ${e.done.toLocaleString()} of ${e.total.toLocaleString()}` : `Checking ${e.folder}…`;
      syncState[e.account] = { ...current, running: true, detail };
      if (e.done > 0) scheduleRefresh(e.account);
    } else if (e.type === "syncFinished") {
      syncState[e.account] = { ...current, running: false, error: e.error, detail: "" };
      scheduleRefresh(e.account);
    } else if (e.type === "pendingChanges") {
      syncState[e.account] = { ...current, pending: e.pending, pendingError: e.error };
    } else {
      scheduleRefresh(e.account);
    }
  });
  onDestroy(() => {
    unlisten.then((fn) => fn());
    clearTimeout(refreshTimer);
  });

  function selectFolder(folder: Folder) {
    if (selectedFolder?.id !== folder.id) selected = null;
    selectedFolder = folder;
  }

  /** Runs a change on the open message. Changes that take it out of the
   *  folder move the selection to its neighbour. */
  async function act(run: (id: number) => Promise<void>, leaves: boolean) {
    const current = selected;
    if (!current || !selectedFolder) return;
    const next = leaves ? (list?.neighbour(current.id) ?? null) : null;
    try {
      await run(current.id);
      notice = "";
      if (leaves) selected = next;
    } catch (err) {
      notice = errorText(err);
    }
    scheduleRefresh(selectedFolder.accountId);
  }

  function toggle(flag: FlagKind, value: boolean) {
    const current = selected;
    act((id) => api.setFlag([id], flag, value), false).then(() => {
      if (selected && current && selected.id === current.id) {
        selected = { ...selected, flags: { ...selected.flags, [flag]: value } };
      }
    });
  }

  async function remove() {
    if (!selected || !selectedFolder) return;
    const forever = selectedFolder.role === "trash" || !accountFolders.some((f) => f.role === "trash");
    if (forever) {
      const confirmed = await ask("Delete this message for good? This cannot be undone.", {
        title: "Delete message",
        kind: "warning",
        okLabel: "Delete",
        cancelLabel: "Cancel",
      });
      if (!confirmed) return;
    }
    act((id) => api.deleteMessages([id]), true);
  }

  function opened() {
    if (selected && !selected.flags.seen) selected = { ...selected, flags: { ...selected.flags, seen: true } };
    if (selectedFolder) scheduleRefresh(selectedFolder.accountId);
  }

  async function removeAccount(account: Account) {
    const confirmed = await ask(`Remove ${account.email} from Novero Mail? Mail on the server is not touched.`, {
      title: "Remove account",
      kind: "warning",
      okLabel: "Remove",
      cancelLabel: "Cancel",
    });
    if (!confirmed) return;
    await api.removeAccount(account.id);
    delete folders[account.id];
    if (selectedFolder?.accountId === account.id) {
      selected = null;
      const next = accounts.find((a) => a.id !== account.id);
      const list = next ? (folders[next.id] ?? []) : [];
      selectedFolder = list.find((f) => f.role === "inbox") ?? list[0] ?? null;
    }
    onAccountsChanged();
  }
</script>

<div class="layout">
  <Sidebar
    {accounts}
    {folders}
    {syncState}
    selected={selectedFolder}
    onSelect={selectFolder}
    {onAddAccount}
    onSync={(id) => api.syncAccount(id)}
    onRemove={removeAccount}
  />
  <MessageList
    bind:this={list}
    folder={selectedFolder}
    version={listVersion}
    selected={selected?.id ?? null}
    onSelect={(item) => (selected = item)}
    onDelete={remove}
  />
  <Reader
    summary={selected}
    folder={selectedFolder}
    folders={accountFolders}
    onOpened={opened}
    onArchive={() => act((id) => api.archiveMessages([id]), true)}
    onDelete={remove}
    onToggle={toggle}
    onMove={(folder) => act((id) => api.moveMessages([id], folder), true)}
  />
  {#if notice}
    <div class="notice error" role="alert">
      <span>{notice}</span>
      <button class="link" onclick={() => (notice = "")}>Dismiss</button>
    </div>
  {/if}
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 240px minmax(300px, 380px) 1fr;
    height: 100%;
    position: relative;
  }
  .notice {
    position: absolute;
    right: 16px;
    bottom: 16px;
    max-width: 420px;
    display: flex;
    gap: 12px;
    align-items: center;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.18);
  }
</style>
