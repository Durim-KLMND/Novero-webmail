<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api,
    defaultSettings,
    errorText,
    onMailEvent,
    onMailto,
    onUpdateReady,
    type Account,
    type Draft,
    type FlagKind,
    type Folder,
    type Mailbox,
    type MailEvent,
    type MessageSummary,
    type Settings,
    type UpdateInfo,
  } from "./api";
  import { folderLabel } from "./format";
  import { toast } from "./toast.svelte";
  import { applyTheme } from "./theme";
  import Sidebar, { type SyncState } from "./Sidebar.svelte";
  import MessageList from "./MessageList.svelte";
  import Reader from "./Reader.svelte";
  import Composer from "./Composer.svelte";
  import Outbox from "./Outbox.svelte";
  import SignInAgain from "./SignInAgain.svelte";
  import SettingsDialog from "./SettingsDialog.svelte";
  import ShortcutHelp from "./ShortcutHelp.svelte";
  import Toasts from "./Toasts.svelte";
  import CrashReport from "./CrashReport.svelte";

  interface Props {
    accounts: Account[];
    onAddAccount: () => void;
    onAccountsChanged: () => void;
  }
  let { accounts, onAddAccount, onAccountsChanged }: Props = $props();

  let folders = $state<Record<number, Folder[]>>({});
  let syncState = $state<Record<number, SyncState>>({});
  /** The folder shown, unless all inboxes or a search are. */
  let selectedFolder = $state<Folder | null>(null);
  let allInboxes = $state(false);
  let searchQuery = $state("");
  /** The selected row of the list: a message or a conversation. */
  let selected = $state<MessageSummary | null>(null);
  let listVersion = $state(0);
  let list = $state<ReturnType<typeof MessageList>>();
  let reader = $state<ReturnType<typeof Reader>>();
  let sidebar = $state<ReturnType<typeof Sidebar>>();
  let settings = $state<Settings>({ ...defaultSettings });
  /** A downloaded update, and a crash report waiting for an answer. */
  let update = $state<UpdateInfo | null>(null);
  let crashReport = $state<string | null>(null);
  let showSettings = $state(false);
  let showHelp = $state(false);
  /** The right pane shows the composer, an account's outbox, or the reader. */
  let composing = $state<Draft | null>(null);
  let composer = $state<ReturnType<typeof Composer>>();
  let outboxAccount = $state<number | null>(null);
  let outboxVersion = $state(0);
  /** Accounts with a message just sent, so the outcome can be announced. */
  const sending = new Set<number>();

  /** The folders of the selected row's account, for Move and Archive. */
  const rowFolders = $derived(selected ? (folders[selected.accountId] ?? []) : []);

  const mailbox = $derived<Mailbox | null>(
    searchQuery
      ? { kind: "search", query: searchQuery }
      : allInboxes
        ? { kind: "allInboxes" }
        : selectedFolder
          ? { kind: "folder", id: selectedFolder.id }
          : null,
  );

  const listTitle = $derived(
    searchQuery
      ? "Search results"
      : allInboxes
        ? "All inboxes"
        : selectedFolder
          ? folderLabel(selectedFolder.name, selectedFolder.role, selectedFolder.delimiter)
          : "",
  );

  const emptyText = $derived(
    searchQuery ? "No messages match your search." : allInboxes ? "No mail yet." : "This folder is empty.",
  );

  function accountEmail(id: number): string {
    return accounts.find((a) => a.id === id)?.email ?? "";
  }

  /** Rows from several accounts or folders say where they are. */
  function place(item: MessageSummary): string | null {
    const several = accounts.length > 1;
    if (searchQuery) {
      const folder = folderLabel(item.folderName, item.folderRole, null);
      return several ? `${folder} · ${accountEmail(item.accountId)}` : folder;
    }
    if (allInboxes && several) return accountEmail(item.accountId);
    return null;
  }

  onMount(async () => {
    try {
      settings = await api.getSettings();
      applyTheme(settings.theme);
      update = await api.readyUpdate();
      crashReport = await api.crashReport();
    } catch (err) {
      say(errorText(err));
    }
    // The app may have been started by clicking an email link.
    openMailto();
  });

  const unlistenUpdate = onUpdateReady((found) => (update = found));
  const unlistenMailto = onMailto(openMailto);

  async function openMailto() {
    const link = await api.takeMailto().catch(() => null);
    const account = selectedFolder?.accountId ?? accounts[0]?.id;
    if (link && account != null) openComposer(() => api.mailtoDraft(account, link));
  }

  async function installUpdate() {
    // Nothing typed is lost: the draft is saved before the app closes.
    await leaveComposer();
    try {
      await api.installUpdate();
    } catch (err) {
      say(`The update could not be installed. ${errorText(err)}`);
      update = null;
    }
  }

  async function crashClosed(never: boolean) {
    crashReport = null;
    if (never) {
      try {
        settings = await api.saveSettings({ ...settings, crashReports: false });
      } catch (err) {
        say(errorText(err));
      }
    }
  }

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
        progress: null,
      }
    );
  }

  function say(text: string, kind: "error" | "info" = "error") {
    toast(text, kind);
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

  /** Downloads of fewer items than this show no progress bar. */
  const PROGRESS_FROM = 50;

  /** The progress bar over the message list: the shown account's sync, or
   *  any account's in lists that mix them. */
  const listProgress = $derived.by(() => {
    const ids = mailbox?.kind === "folder" && selectedFolder ? [selectedFolder.accountId] : accounts.map((a) => a.id);
    for (const id of ids) {
      const s = syncState[id];
      if (s?.running && s.progress) return s.progress;
    }
    return null;
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
      // Lists that mix accounts refresh for any of them.
      if (mailbox?.kind !== "folder" || (selectedFolder && ids.includes(selectedFolder.accountId))) listVersion += 1;
    }, 700);
  }

  const unlisten = onMailEvent((e: MailEvent) => {
    const current = stateOf(e.account);
    if (e.type === "syncStarted") {
      syncState[e.account] = { ...current, running: true, error: null, detail: "Checking for mail…", progress: null };
    } else if (e.type === "syncProgress") {
      const detail = e.total > 0 ? `${e.folder}: ${e.done.toLocaleString()} of ${e.total.toLocaleString()}` : `Checking ${e.folder}…`;
      syncState[e.account] = { ...current, running: true, detail };
      if (e.done > 0) scheduleRefresh(e.account);
    } else if (e.type === "accountProgress") {
      // A handful of new messages is quick; a bar is for real downloads.
      const progress = e.total >= PROGRESS_FROM ? { stage: e.stage, done: e.done, total: e.total } : null;
      syncState[e.account] = { ...current, progress };
    } else if (e.type === "syncFinished") {
      syncState[e.account] = { ...current, running: false, error: e.error, detail: "", progress: null };
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
    unlistenUpdate.then((fn) => fn());
    unlistenMailto.then((fn) => fn());
    clearTimeout(refreshTimer);
  });

  function selectFolder(folder: Folder) {
    if (selectedFolder?.id !== folder.id || allInboxes || searchQuery) selected = null;
    selectedFolder = folder;
    allInboxes = false;
    sidebar?.clearSearch();
  }

  function selectAllInboxes() {
    if (!allInboxes || searchQuery) selected = null;
    allInboxes = true;
    sidebar?.clearSearch();
  }

  function search(query: string) {
    if (query === searchQuery) return;
    selected = null;
    searchQuery = query;
  }

  /** Runs a change on every message of the selected row. Changes that take
   *  the row out of the list move the selection to its neighbour, and are
   *  confirmed with `done`. */
  async function act(run: (ids: number[]) => Promise<void>, leaves: boolean, done?: (many: boolean) => string) {
    const shown = selected;
    if (!shown) return;
    // The list may have picked up replies since the row was selected.
    const current = list?.find(shown.id) ?? shown;
    const next = leaves ? (list?.neighbour(current.id) ?? null) : null;
    try {
      await run(current.ids);
      if (leaves) selected = next;
      if (done) say(done(current.ids.length > 1), "info");
    } catch (err) {
      say(errorText(err));
    }
    scheduleRefresh(current.accountId);
  }

  const what = (many: boolean) => (many ? "Conversation" : "Message");

  function toggle(flag: FlagKind, value: boolean) {
    const current = selected;
    if (!current) return;
    act((ids) => api.setFlag(ids, flag, value), false).then(() => {
      if (selected && selected.id === current.id) {
        selected = { ...selected, flags: { ...selected.flags, [flag]: value } };
      }
    });
  }

  const canArchive = $derived(
    !!selected && selected.folderRole !== "archive" && rowFolders.some((f) => f.role === "archive"),
  );

  function archive() {
    if (canArchive) act((ids) => api.archiveMessages(ids), true, (many) => `${what(many)} archived`);
  }

  function move(folder: number) {
    const target = rowFolders.find((f) => f.id === folder);
    const label = target ? folderLabel(target.name, target.role, target.delimiter) : "the folder";
    act((ids) => api.moveMessages(ids, folder), true, (many) => `${what(many)} moved to ${label}`);
  }

  async function remove() {
    if (!selected) return;
    const forever = selected.folderRole === "trash" || !rowFolders.some((f) => f.role === "trash");
    const many = selected.ids.length > 1;
    if (forever) {
      const confirmed = await ask(
        `Delete ${many ? "these messages" : "this message"} for good? This cannot be undone.`,
        {
          title: many ? "Delete conversation" : "Delete message",
          kind: "warning",
          okLabel: "Delete",
          cancelLabel: "Cancel",
        },
      );
      if (!confirmed) return;
    }
    act((ids) => api.deleteMessages(ids), true, (many) => (forever ? `${what(many)} deleted` : `${what(many)} moved to Trash`));
  }

  function opened() {
    if (selected) scheduleRefresh(selected.accountId);
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
    const account = (allInboxes || searchQuery ? selected?.accountId : selectedFolder?.accountId) ?? selectedFolder?.accountId ?? accounts[0]?.id;
    if (account != null) openComposer(() => api.newDraft(account));
  }

  async function selectMessage(item: MessageSummary) {
    outboxAccount = null;
    if (item.folderRole === "drafts") {
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
    if (selected?.folderRole === "drafts") selected = null;
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

  /** The account whose sign-in is being renewed. */
  let signingIn = $state<Account | null>(null);

  function signInDone(signedIn: boolean) {
    if (signedIn && signingIn) say(`Signed in to ${signingIn.email} again`, "info");
    signingIn = null;
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
    if (selected?.accountId === account.id) selected = null;
    if (selectedFolder?.accountId === account.id) {
      const next = accounts.find((a) => a.id !== account.id);
      const list = next ? (folders[next.id] ?? []) : [];
      selectedFolder = list.find((f) => f.role === "inbox") ?? list[0] ?? null;
    }
    onAccountsChanged();
  }

  function settingsClosed(saved: Settings | null, signaturesChanged: boolean) {
    showSettings = false;
    if (signaturesChanged) onAccountsChanged();
    if (!saved) return;
    const regroup = saved.conversations !== settings.conversations;
    settings = saved;
    if (regroup) {
      selected = null;
      listVersion += 1;
    }
    say("Settings saved", "info");
  }

  /** Typing in a field, or a dialog is open: keys belong to it. */
  function typing(event: KeyboardEvent): boolean {
    const target = event.target as HTMLElement | null;
    if (!target) return false;
    return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
  }

  function shortcut(event: KeyboardEvent) {
    if (event.defaultPrevented || event.ctrlKey || event.altKey || event.metaKey) return;
    if (showHelp) {
      if (event.key === "Escape" || event.key === "?") {
        event.preventDefault();
        showHelp = false;
      }
      return;
    }
    if (showSettings || signingIn || typing(event)) return;
    const key = event.key;
    if (key === "?") showHelp = true;
    else if (key === "/") sidebar?.focusSearch();
    else if (key === "Escape" && searchQuery) sidebar?.clearSearch();
    else if (composing || outboxAccount != null) return;
    else if (key === "j" || key === "ArrowDown") list?.move(1);
    else if (key === "k" || key === "ArrowUp") list?.move(-1);
    else if (key === "c") compose();
    else if (key === "r" || key === "a" || key === "f") {
      const target = reader?.replyTarget();
      if (target == null) return;
      if (key === "f") openComposer(() => api.forwardDraft(target));
      else openComposer(() => api.replyDraft(target, key === "a"));
    } else if (key === "e") archive();
    else if (key === "#" || key === "Delete") remove();
    else if (key === "s" && selected) toggle("flagged", !(reader?.rowFlags().flagged ?? selected.flags.flagged));
    else if (key === "U" && selected) toggle("seen", false);
    else if (key === "I" && selected) toggle("seen", true);
    else return;
    event.preventDefault();
  }
</script>

<svelte:window onkeydown={shortcut} />

<div class="layout">
  <Sidebar
    {accounts}
    {folders}
    {syncState}
    bind:this={sidebar}
    selected={allInboxes || searchQuery ? null : selectedFolder}
    allInboxes={allInboxes && !searchQuery}
    onSelect={selectFolder}
    onAllInboxes={selectAllInboxes}
    onSearch={search}
    onSettings={() => (showSettings = true)}
    onHelp={() => (showHelp = true)}
    {onAddAccount}
    onSync={(id) => api.syncAccount(id)}
    onRemove={removeAccount}
    onSignInAgain={(account) => (signingIn = account)}
    onCompose={compose}
    onOutbox={showOutbox}
    {update}
    onInstallUpdate={installUpdate}
  />
  <MessageList
    bind:this={list}
    {mailbox}
    title={listTitle}
    {emptyText}
    unit={settings.conversations ? "conversation" : "message"}
    progress={listProgress}
    version={listVersion}
    {place}
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
    bind:this={reader}
    row={selected}
    folders={rowFolders}
    version={listVersion}
    onOpened={opened}
    onArchive={archive}
    onDelete={remove}
    onToggle={toggle}
    onMove={move}
    onReply={(message, all) => openComposer(() => api.replyDraft(message, all))}
    onForward={(message) => openComposer(() => api.forwardDraft(message))}
  />
  {/if}
  {#if signingIn}
    <SignInAgain account={signingIn} onDone={signInDone} />
  {/if}
  {#if showSettings}
    <SettingsDialog {accounts} {settings} onClose={settingsClosed} onUpdate={(found) => (update = found)} />
  {/if}
  {#if crashReport}
    <CrashReport report={crashReport} onClose={crashClosed} />
  {/if}
  {#if showHelp}
    <ShortcutHelp onClose={() => (showHelp = false)} />
  {/if}
  <Toasts />
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 240px minmax(300px, 380px) 1fr;
    height: 100%;
    position: relative;
  }
</style>
