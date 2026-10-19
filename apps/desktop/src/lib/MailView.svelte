<script lang="ts">
  import { onDestroy } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, onMailEvent, type Account, type Folder, type MailEvent } from "./api";
  import Sidebar from "./Sidebar.svelte";
  import MessageList from "./MessageList.svelte";
  import Reader from "./Reader.svelte";

  interface Props {
    accounts: Account[];
    onAddAccount: () => void;
    onAccountsChanged: () => void;
  }
  let { accounts, onAddAccount, onAccountsChanged }: Props = $props();

  let folders = $state<Record<number, Folder[]>>({});
  let syncState = $state<Record<number, { running: boolean; error: string | null; detail: string }>>({});
  let selectedFolder = $state<Folder | null>(null);
  let selectedMessage = $state<number | null>(null);
  let listVersion = $state(0);

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
      if (!(account.id in folders)) loadFolders(account.id);
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
    const current = syncState[e.account] ?? { running: false, error: null, detail: "" };
    if (e.type === "syncStarted") {
      syncState[e.account] = { running: true, error: null, detail: "Checking for mail…" };
    } else if (e.type === "syncProgress") {
      const detail = e.total > 0 ? `${e.folder}: ${e.done.toLocaleString()} of ${e.total.toLocaleString()}` : `Checking ${e.folder}…`;
      syncState[e.account] = { ...current, running: true, detail };
      if (e.done > 0) scheduleRefresh(e.account);
    } else {
      syncState[e.account] = { running: false, error: e.error, detail: "" };
      scheduleRefresh(e.account);
    }
  });
  onDestroy(() => {
    unlisten.then((fn) => fn());
    clearTimeout(refreshTimer);
  });

  function selectFolder(folder: Folder) {
    if (selectedFolder?.id !== folder.id) selectedMessage = null;
    selectedFolder = folder;
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
      selectedMessage = null;
      const next = accounts.find((a) => a.id !== account.id);
      const list = next ? folders[next.id] ?? [] : [];
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
  <MessageList folder={selectedFolder} version={listVersion} selected={selectedMessage} onSelect={(id) => (selectedMessage = id)} />
  <Reader
    messageId={selectedMessage}
    onOpened={() => selectedFolder && scheduleRefresh(selectedFolder.accountId)}
  />
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 240px minmax(300px, 380px) 1fr;
    height: 100%;
  }
</style>
