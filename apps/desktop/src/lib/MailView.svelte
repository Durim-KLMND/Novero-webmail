<script lang="ts">
  import { onDestroy } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api,
    errorText,
    onMailEvent,
    type Account,
    type Draft,
    type FlagKind,
    type Folder,
    type MailEvent,
    type MessageSummary,
  } from "./api";
  import Sidebar, { type SyncState } from "./Sidebar.svelte";
  import MessageList from "./MessageList.svelte";
  import Reader from "./Reader.svelte";
  import Composer from "./Composer.svelte";
  import Outbox from "./Outbox.svelte";

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
  let notice = $state<{ text: string; kind: "error" | "info" } | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  let list = $state<ReturnType<typeof MessageList>>();
  /** The right pane shows the composer, an account's outbox, or the reader. */
  let composing = $state<Draft | null>(null);
  let composer = $state<ReturnType<typeof Composer>>();
  let outboxAccount = $state<number | null>(null);
  let outboxVersion = $state(0);
  /** Accounts with a message just sent, so the outcome can be announced. */
  const sending = new Set<number>();

  const accountFolders = $derived(selectedFolder ? (folders[selectedFolder.accountId] ?? []) : []);

  function stateOf(account: number): SyncState {
    return (
      syncState[account] ?? {
        running: false,
        error: null,
        detail: "",
        pending: 0,
        pendingError: null,
        queued: 0,
        failed: 0,
        outboxError: null,
      }
    );
  }

  function say(text: string, kind: "error" | "info" = "error") {
    clearTimeout(noticeTimer);
    notice = { text, kind };
    if (kind === "info") noticeTimer = setTimeout(() => (notice = null), 4000);
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
        api.outboxStatus(account.id).then(([queued, failed]) => {
          syncState[account.id] = { ...stateOf(account.id), queued, failed };
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
    } else if (e.type === "outboxChanged") {
      syncState[e.account] = { ...current, queued: e.queued, failed: e.failed, outboxError: e.error };
      outboxVersion += 1;
      if (sending.has(e.account) && e.queued === 0) {
        sending.delete(e.account);
        if (e.failed > current.failed) say("A message could not be sent. It is waiting in the outbox.");
        else say("Message sent", "info");
      }
    } else {
      scheduleRefresh(e.account);
    }
  });
  onDestroy(() => {
    unlisten.then((fn) => fn());
    clearTimeout(refreshTimer);
    clearTimeout(noticeTimer);
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
      notice = null;
      if (leaves) selected = next;
    } catch (err) {
      say(errorText(err));
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

  /** Saves and closes an open composer before the pane shows something else. */
  async function leaveComposer() {
    if (!composing) return;
    await composer?.flush();
    composing = null;
  }

  async function openComposer(make: () => Promise<Draft>) {
    try {
      await leaveComposer();
      composing = await make();
      outboxAccount = null;
    } catch (err) {
      say(errorText(err));
    }
  }

  function compose() {
    const account = selectedFolder?.accountId ?? accounts[0]?.id;
    if (account != null) openComposer(() => api.newDraft(account));
  }

  async function selectMessage(item: MessageSummary) {
    outboxAccount = null;
    if (selectedFolder?.role === "drafts") {
      selected = item;
      await openComposer(() => api.openDraft(item.id));
      return;
    }
    await leaveComposer();
    selected = item;
  }

  function composerClosed(outcome: "sent" | "saved" | "discarded") {
    const draft = composing;
    composing = null;
    if (selectedFolder?.role === "drafts") selected = null;
    if (!draft) return;
    if (outcome === "sent") {
      sending.add(draft.account);
      say("Sending…", "info");
    } else if (outcome === "discarded") {
      say("Draft deleted", "info");
    }
    scheduleRefresh(draft.account);
  }

  async function showOutbox(account: number) {
    await leaveComposer();
    outboxAccount = account;
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
    onCompose={compose}
    onOutbox={showOutbox}
  />
  <MessageList
    bind:this={list}
    folder={selectedFolder}
    version={listVersion}
    selected={selected?.id ?? null}
    onSelect={selectMessage}
    onDelete={remove}
  />
  {#if composing}
    {#key composing.id}
      <Composer
        bind:this={composer}
        draft={composing}
        {accounts}
        onClose={composerClosed}
        onReplace={(draft) => (composing = draft)}
      />
    {/key}
  {:else if outboxAccount != null && accounts.some((a) => a.id === outboxAccount)}
    <Outbox
      account={accounts.find((a) => a.id === outboxAccount)!}
      version={outboxVersion}
      onEdit={(draft) => openComposer(async () => draft)}
      onClose={() => (outboxAccount = null)}
    />
  {:else}
  <Reader
    summary={selected}
    folder={selectedFolder}
    folders={accountFolders}
    onOpened={opened}
    onArchive={() => act((id) => api.archiveMessages([id]), true)}
    onDelete={remove}
    onToggle={toggle}
    onMove={(folder) => act((id) => api.moveMessages([id], folder), true)}
    onReply={(all) => selected && openComposer(() => api.replyDraft(selected!.id, all))}
    onForward={() => selected && openComposer(() => api.forwardDraft(selected!.id))}
  />
  {/if}
  {#if notice}
    <div class="notice" class:error={notice.kind === "error"} class:info={notice.kind === "info"} role="alert">
      <span>{notice.text}</span>
      <button class="link" onclick={() => (notice = null)}>Dismiss</button>
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
  .notice.info {
    background: var(--bg-side);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 14px;
  }
</style>
