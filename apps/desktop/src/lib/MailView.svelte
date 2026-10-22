<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api,
    defaultSettings,
    errorText,
    onMailEvent,
    onMailto,
    onUpdateStatus,
    type Account,
    type ComposerOutcome,
    type Draft,
    type FlagKind,
    type Folder,
    type Mailbox,
    type MailEvent,
    type MessageSummary,
    type Settings,
    type UpdateStatus,
  } from "./api";
  import { folderLabel, folderPath } from "./format";
  import { toast } from "./toast.svelte";
  import { applyTheme } from "./theme";
  import Sidebar, { type SyncState } from "./Sidebar.svelte";
  import MessageList from "./MessageList.svelte";
  import Reader from "./Reader.svelte";
  import Composer from "./Composer.svelte";
  import Outbox from "./Outbox.svelte";
  import SignInAgain from "./SignInAgain.svelte";
  import SettingsDialog from "./SettingsDialog.svelte";
  import RulesDialog from "./RulesDialog.svelte";
  import ShortcutHelp from "./ShortcutHelp.svelte";
  import Toasts from "./Toasts.svelte";
  import AppRail, { type AppView } from "./AppRail.svelte";
  import CalendarView from "./CalendarView.svelte";
  import CrashReport from "./CrashReport.svelte";
  import UpdateDialog from "./UpdateDialog.svelte";
  import { needPro } from "./pro.svelte";
  import { items, openMenu, setMailtoHandler, type MenuItem } from "./menus.svelte";
  import { laterChoices, laterText } from "./later";
  import PickTime from "./PickTime.svelte";
  import NameDialog from "./NameDialog.svelte";
  import Icon from "./Icon.svelte";

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
  /** The Snoozed list is shown. */
  let snoozedView = $state(false);
  let snoozedCount = $state(0);
  /** "Pick a date and time" for Snooze is open. */
  let pickingSnooze = $state(false);
  let searchQuery = $state("");
  /** The selected row of the list: a message or a conversation. */
  let selected = $state<MessageSummary | null>(null);
  let listVersion = $state(0);
  let list = $state<ReturnType<typeof MessageList>>();
  let reader = $state<ReturnType<typeof Reader>>();
  let sidebar = $state<ReturnType<typeof Sidebar>>();
  let settings = $state<Settings>({ ...defaultSettings });
  /** A downloaded update, and a crash report waiting for an answer. */
  let update = $state<UpdateStatus | null>(null);
  let showWhatsNew = $state(false);
  let crashReport = $state<string | null>(null);
  let showSettings = $state(false);
  /** The rules dialog: the account it shows, and a sender to make a rule for. */
  let rulesFor = $state<{ account: number; from?: string } | null>(null);
  function openRules(start: { account: number; from?: string }) {
    if (needPro("rules")) rulesFor = start;
  }
  let showHelp = $state(false);
  /** Mail or the calendar, picked on the rail. */
  let view = $state<AppView>("mail");
  /** The calendar stays loaded once opened, to come back to where it was. */
  let calendarOpened = $state(false);
  $effect(() => {
    if (view === "calendar") calendarOpened = true;
  });
  const unreadInboxes = $derived(
    Object.values(folders)
      .flat()
      .filter((f) => f.role === "inbox")
      .reduce((n, f) => n + f.unread, 0),
  );
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
        : snoozedView
          ? { kind: "snoozed" }
          : selectedFolder
          ? { kind: "folder", id: selectedFolder.id }
          : null,
  );

  const listTitle = $derived(
    searchQuery
      ? "Search results"
      : allInboxes
        ? "All inboxes"
        : snoozedView
          ? "Snoozed"
          : selectedFolder
          ? folderLabel(selectedFolder.name, selectedFolder.role, selectedFolder.delimiter)
          : "",
  );

  const emptyText = $derived(
    searchQuery
      ? "No messages match your search."
      : allInboxes
        ? "No mail yet."
        : snoozedView
          ? "Nothing is snoozed. Snoozed mail comes back to its folder at the time you pick."
          : "This folder is empty.",
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
    if ((allInboxes || snoozedView) && several) return accountEmail(item.accountId);
    return null;
  }

  onMount(async () => {
    refreshSnoozed();
    try {
      settings = await api.getSettings();
      applyTheme(settings.theme);
      update = await api.updateStatus();
      crashReport = await api.crashReport();
    } catch (err) {
      say(errorText(err));
    }
    // The app may have been started by clicking an email link.
    openMailto();
  });

  setMailtoHandler((link) => {
    const account = selected?.accountId ?? selectedFolder?.accountId ?? accounts[0]?.id;
    if (account != null) openComposer(() => api.mailtoDraft(account, link));
  });
  onDestroy(() => setMailtoHandler(null));

  const unlistenUpdate = onUpdateStatus((status) => {
    update = status;
    if (status.phase === "idle") showWhatsNew = false;
  });
  const unlistenMailto = onMailto(openMailto);
  // A message window closed (see ComposerWindow).
  const unlistenWindows = listen<{ account: number; outcome: ComposerOutcome }>(
    "composer-closed",
    (e) => ended(e.payload.account, e.payload.outcome),
  );

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
    }
  }

  async function downloadUpdate() {
    try {
      await api.downloadUpdate();
    } catch (err) {
      say(`The update could not be downloaded. ${errorText(err)}`);
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
        scheduled: 0,
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
        api.outboxStatus(account.id).then(([queued, failed, scheduled]) => {
          syncState[account.id] = { ...stateOf(account.id), queued, failed, scheduled };
        });
      }
    }
  });

  /** Syncing runs quietly: mail shows up as it arrives, with no spinner or
   *  progress bar. Only an empty list says a first download is under way. */
  const arriving = $derived.by(() => {
    const ids = mailbox?.kind === "folder" && selectedFolder ? [selectedFolder.accountId] : accounts.map((a) => a.id);
    return ids.some((id) => syncState[id]?.running && syncState[id]?.progress?.stage === "messages");
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
      syncState[e.account] = { ...current, running: true, error: null, detail: "", progress: null };
    } else if (e.type === "syncProgress") {
      const detail = e.total > 0 ? `${e.folder}: ${e.done.toLocaleString()} of ${e.total.toLocaleString()}` : `Checking ${e.folder}…`;
      syncState[e.account] = { ...current, detail };
      if (e.done > 0) scheduleRefresh(e.account);
    } else if (e.type === "accountProgress") {
      const progress = e.done < e.total ? { stage: e.stage, done: e.done, total: e.total } : null;
      syncState[e.account] = { ...current, progress };
    } else if (e.type === "syncFinished") {
      syncState[e.account] = { ...current, running: false, error: e.error, detail: "", progress: null };
      scheduleRefresh(e.account);
    } else if (e.type === "pendingChanges") {
      syncState[e.account] = { ...current, pending: e.pending, pendingError: e.error };
    } else if (e.type === "outboxChanged") {
      syncState[e.account] = { ...current, queued: e.queued, failed: e.failed, scheduled: e.scheduled, outboxError: e.error };
      outboxVersion += 1;
      if (sending.has(e.account) && e.queued === 0) {
        sending.delete(e.account);
        if (e.failed > current.failed) say("A message could not be sent. It is waiting in the outbox.");
        else say("Message sent", "info");
      }
    } else {
      scheduleRefresh(e.account);
      // Snoozed mail may have come back.
      refreshSnoozed();
    }
  });
  onDestroy(() => {
    unlisten.then((fn) => fn());
    unlistenUpdate.then((fn) => fn());
    unlistenMailto.then((fn) => fn());
    unlistenWindows.then((fn) => fn());
    clearTimeout(refreshTimer);
  });

  function selectFolder(folder: Folder) {
    if (selectedFolder?.id !== folder.id || allInboxes || snoozedView || searchQuery) selected = null;
    selectedFolder = folder;
    allInboxes = false;
    snoozedView = false;
    sidebar?.clearSearch();
  }

  function selectAllInboxes() {
    if (!allInboxes || searchQuery) selected = null;
    allInboxes = true;
    snoozedView = false;
    sidebar?.clearSearch();
  }

  function selectSnoozed() {
    if (!snoozedView || searchQuery) selected = null;
    snoozedView = true;
    allInboxes = false;
    sidebar?.clearSearch();
  }

  async function refreshSnoozed() {
    snoozedCount = await api.snoozedCount().catch(() => snoozedCount);
  }

  /** Hides the selected row until `at`. */
  function snooze(at: Date) {
    if (!needPro("snooze")) return;
    act(
      (ids) => api.snoozeMessages(ids, Math.floor(at.getTime() / 1000)),
      true,
      (many) => `${what(many)} snoozed until ${laterText(at)}`,
    ).then(refreshSnoozed);
  }

  function unsnooze() {
    act((ids) => api.unsnoozeMessages(ids), true, (many) => `${what(many)} is back`).then(refreshSnoozed);
  }

  /** The Snooze menu: the usual times, or one picked. */
  function snoozeChoices(): MenuItem[] {
    return [
      ...laterChoices(new Date(), true).map((c) => ({
        label: c.label,
        shortcut: c.when,
        icon: "clock" as const,
        run: () => snooze(c.at),
      })),
      "separator",
      { label: "Pick a date and time…", icon: "calendar", run: () => needPro("snooze") && (pickingSnooze = true) },
    ];
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
    const target = selected ? folders[selected.accountId]?.find((f) => f.id === folder) : undefined;
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

  /** The right-click menu of a message row. The row is selected first, so the
   *  actions are the toolbar's. */
  async function rowMenu(item: MessageSummary, event: MouseEvent) {
    event.preventDefault();
    const draft = item.folderRole === "drafts";
    if (selected?.id !== item.id) {
      if (draft) {
        await leaveComposer();
        selected = item;
      } else {
        await selectMessage(item);
      }
    }
    const target = () => (selected?.id === item.id ? reader?.replyTarget() : null) ?? item.id;
    const seen = item.flags.seen;
    const flagged = item.flags.flagged;
    const sender = item.from?.match(/<([^<>]+@[^<>]+)>/)?.[1] ?? item.from?.match(/[^\s<>"]+@[^\s<>"]+/)?.[0];
    const targets = (folders[item.accountId] ?? []).filter((f) => f.id !== item.folderId);
    const moveTo: MenuItem[] = [
      ...targets.map((f): MenuItem => ({
        label: f.role === "other" ? folderPath(f.name, f.delimiter) : folderLabel(f.name, f.role, f.delimiter),
        icon: f.role === "other" ? "folder" : f.role,
        run: () => move(f.id),
      })),
      "separator",
      { label: "New folder…", icon: "new-folder", run: () => newFolder(item.accountId, null, (f) => move(f.id)) },
    ];
    openMenu(
      event,
      items(
        draft && { label: "Open draft", icon: "drafts", run: () => selectMessage(item) },
        !draft && { label: "Reply", icon: "reply", shortcut: "R", run: () => openComposer(() => api.replyDraft(target(), false)) },
        !draft && { label: "Reply all", icon: "reply-all", shortcut: "A", run: () => openComposer(() => api.replyDraft(target(), true)) },
        !draft && { label: "Forward", icon: "forward", shortcut: "F", run: () => openComposer(() => api.forwardDraft(target())) },
        "separator",
        !draft && {
          label: seen ? "Mark unread" : "Mark read",
          icon: seen ? "unread" : "read",
          shortcut: seen ? "Shift+U" : "Shift+I",
          run: () => toggle("seen", !seen),
        },
        !draft && { label: flagged ? "Remove flag" : "Flag", icon: flagged ? "unflag" : "flag", shortcut: "S", run: () => toggle("flagged", !flagged) },
        !draft && snoozedView && { label: "Unsnooze", icon: "clock", shortcut: "B", run: unsnooze },
        !draft && !snoozedView && { label: "Snooze", icon: "clock", items: snoozeChoices() },
        "separator",
        !draft && { label: "Move to", icon: "move", items: moveTo },
        !draft && item.folderRole === "inbox" && sender && {
          label: "Make a rule for this sender…",
          icon: "rules",
          run: () => openRules({ account: item.accountId, from: sender }),
        },
        canArchive && { label: "Archive", icon: "archive", shortcut: "E", run: archive },
        {
          label: item.folderRole === "trash" ? "Delete forever" : "Delete",
          icon: "trash",
          shortcut: "Del",
          danger: true,
          run: remove,
        },
      ),
    );
  }

  /** The right-click menu of a folder in the sidebar. */
  function folderMenu(folder: Folder, event: MouseEvent) {
    const own = folder.role === "other";
    const bin = folder.role === "trash" || folder.role === "junk";
    openMenu(
      event,
      items(
        { label: "Open", icon: folder.role === "other" ? "folder" : folder.role, run: () => selectFolder(folder) },
        { label: "Check for mail", icon: "refresh", run: () => api.syncAccount(folder.accountId) },
        folder.unread > 0 && { label: "Mark all as read", icon: "read", run: () => markFolderRead(folder) },
        bin && folder.total > 0 && {
          label: folder.role === "trash" ? "Empty Trash" : "Empty Junk",
          icon: "trash",
          danger: true,
          run: () => emptyFolder(folder),
        },
        "separator",
        { label: "New folder…", icon: "new-folder", run: () => newFolder(folder.accountId) },
        (own || folder.role === "inbox") && !!folder.delimiter && {
          label: "New folder inside…",
          icon: "new-folder",
          run: () => newFolder(folder.accountId, folder),
        },
        own && { label: "Rename…", icon: "rename-folder", run: () => renameFolder(folder) },
        own && { label: "Delete folder…", icon: "trash", danger: true, run: () => deleteFolder(folder) },
        "separator",
        { label: "New message", icon: "drafts", shortcut: "C", run: () => openComposer(() => api.newDraft(folder.accountId)) },
      ),
    );
  }

  /** The name dialog for a new folder (inside `parent`, or at the top) or for
   *  renaming one. `then` gets a folder just made, e.g. to move mail into it. */
  let naming = $state<{
    account: number;
    parent: Folder | null;
    renaming: Folder | null;
    then?: (folder: Folder) => void;
  } | null>(null);
  let namingError = $state("");
  let namingBusy = $state(false);

  function newFolder(account: number, parent: Folder | null = null, then?: (folder: Folder) => void) {
    naming = { account, parent, renaming: null, then };
    namingError = "";
  }

  function renameFolder(folder: Folder) {
    naming = { account: folder.accountId, parent: null, renaming: folder };
    namingError = "";
  }

  async function named(name: string) {
    const asked = naming;
    if (!asked) return;
    namingBusy = true;
    namingError = "";
    try {
      if (asked.renaming) {
        await api.renameFolder(asked.renaming.id, name);
        await loadFolders(asked.account);
        say(`Folder renamed to ${name}`, "info");
      } else {
        const made = await api.createFolder(asked.account, asked.parent?.id ?? null, name);
        await loadFolders(asked.account);
        if (asked.then) asked.then(made);
        else say(`Folder ${name} made`, "info");
      }
      naming = null;
    } catch (err) {
      namingError = errorText(err);
    } finally {
      namingBusy = false;
    }
  }

  /** Whether `inner` is `outer` or a folder inside it. */
  function within(inner: Folder, outer: Folder): boolean {
    return inner.id === outer.id || (!!outer.delimiter && inner.name.startsWith(outer.name + outer.delimiter));
  }

  async function deleteFolder(folder: Folder) {
    const doomed = (folders[folder.accountId] ?? []).filter((f) => within(f, folder));
    const inside = doomed.length - 1;
    const mail = doomed.reduce((n, f) => n + f.total, 0);
    const parts = [
      `Delete “${folderPath(folder.name, folder.delimiter)}”`,
      inside > 0 ? ` and the ${inside === 1 ? "folder" : `${inside} folders`} inside it` : "",
      "?",
      mail > 0 ? ` The ${mail === 1 ? "message" : `${mail.toLocaleString()} messages`} in it ${mail === 1 ? "is" : "are"} deleted too.` : "",
      " This changes the mail server and cannot be undone.",
    ];
    const confirmed = await ask(parts.join(""), {
      title: "Delete folder",
      kind: "warning",
      okLabel: "Delete",
      cancelLabel: "Cancel",
    });
    if (!confirmed) return;
    try {
      await api.deleteFolder(folder.id);
      if (selectedFolder && doomed.some((f) => f.id === selectedFolder!.id)) {
        selected = null;
        selectedFolder = (folders[folder.accountId] ?? []).find((f) => f.role === "inbox") ?? null;
      }
      await loadFolders(folder.accountId);
      say("Folder deleted", "info");
    } catch (err) {
      say(errorText(err));
    }
  }

  async function markFolderRead(folder: Folder) {
    try {
      await api.markFolderRead(folder.id);
      say(`Everything in ${folderLabel(folder.name, folder.role, folder.delimiter)} is marked read`, "info");
    } catch (err) {
      say(errorText(err));
    }
    scheduleRefresh(folder.accountId);
  }

  async function emptyFolder(folder: Folder) {
    const label = folderLabel(folder.name, folder.role, folder.delimiter);
    const confirmed = await ask(
      `Delete the ${folder.total === 1 ? "message" : `${folder.total.toLocaleString()} messages`} in ${label} for good? This cannot be undone.`,
      { title: `Empty ${label}`, kind: "warning", okLabel: "Empty", cancelLabel: "Cancel" },
    );
    if (!confirmed) return;
    try {
      await api.emptyFolder(folder.id);
      if (selected?.folderId === folder.id) selected = null;
      say(`${label} is empty`, "info");
    } catch (err) {
      say(errorText(err));
    }
    scheduleRefresh(folder.accountId);
  }

  /** A message row being dragged onto a folder in the sidebar, where the
   *  pointer is, and the folder under it that would take it. HTML drag and
   *  drop is not used: the window's file drop handling takes it over on Windows. */
  let carrying = $state<{ item: MessageSummary; x: number; y: number } | null>(null);
  let dropTarget = $state<number | null>(null);
  /** Pixels the pointer moves before a press becomes a drag. */
  const DRAG_FROM = 6;

  function pressRow(item: MessageSummary, event: PointerEvent) {
    if (event.button !== 0 || item.folderRole === "drafts") return;
    const startX = event.clientX;
    const startY = event.clientY;
    const moved = (e: PointerEvent) => {
      if (!carrying && Math.hypot(e.clientX - startX, e.clientY - startY) < DRAG_FROM) return;
      carrying = { item, x: e.clientX, y: e.clientY };
      dropTarget = folderAt(e.clientX, e.clientY, item);
    };
    const ended = (e: PointerEvent) => {
      window.removeEventListener("pointermove", moved);
      window.removeEventListener("pointerup", ended);
      window.removeEventListener("pointercancel", ended);
      const target = carrying && e.type === "pointerup" ? dropTarget : null;
      carrying = null;
      dropTarget = null;
      if (target != null) dropOn(item, target);
    };
    window.addEventListener("pointermove", moved);
    window.addEventListener("pointerup", ended);
    window.addEventListener("pointercancel", ended);
  }

  /** The folder at a point that could take `item`: one of its own account's, not its own. */
  function folderAt(x: number, y: number, item: MessageSummary): number | null {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-folder]");
    const id = Number(el?.dataset.folder);
    const folder = (folders[item.accountId] ?? []).find((f) => f.id === id);
    return folder && folder.id !== item.folderId ? folder.id : null;
  }

  async function dropOn(item: MessageSummary, folder: number) {
    // The selected row moves like Move to, so the selection steps on.
    if (selected && item.ids.includes(selected.id)) {
      move(folder);
      return;
    }
    const target = folders[item.accountId]?.find((f) => f.id === folder);
    const label = target ? folderLabel(target.name, target.role, target.delimiter) : "the folder";
    try {
      await api.moveMessages(item.ids, folder);
      say(`${what(item.ids.length > 1)} moved to ${label}`, "info");
    } catch (err) {
      say(errorText(err));
    }
    scheduleRefresh(item.accountId);
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
      view = "mail";
      await leaveComposer();
      composing = await make();
      outboxAccount = null;
    } catch (err) {
      say(errorText(err));
    }
  }

  /** A new message from the account in view, here or in a window of its own. */
  async function compose(newWindow = false) {
    const account = (allInboxes || searchQuery ? selected?.accountId : selectedFolder?.accountId) ?? selectedFolder?.accountId ?? accounts[0]?.id;
    if (account == null) return;
    if (!newWindow) {
      openComposer(() => api.newDraft(account));
      return;
    }
    try {
      const draft = await api.newDraft(account);
      await api.openComposerWindow(draft.id);
    } catch (err) {
      say(errorText(err));
    }
  }

  /** Moves the open draft to a window of its own. */
  async function popOut() {
    const draft = composing;
    if (!draft) return;
    composing = null;
    if (selected?.folderRole === "drafts") selected = null;
    try {
      await api.openComposerWindow(draft.id);
    } catch (err) {
      say(errorText(err));
    }
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

  function composerClosed(outcome: ComposerOutcome) {
    const draft = composing;
    composing = null;
    if (selected?.folderRole === "drafts") selected = null;
    if (draft) ended(draft.account, outcome);
  }

  /** A composer here or in its own window is done. */
  function ended(account: number, outcome: ComposerOutcome) {
    if (outcome === "sent") {
      sending.add(account);
      say("Sending…", "info");
    } else if (outcome === "scheduled") {
      say("Scheduled. It waits in the Outbox until then.", "info");
    } else if (outcome === "discarded") {
      say("Draft deleted", "info");
    }
    scheduleRefresh(account);
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

  // The folder list and message list can be made wider or narrower; the
  // widths are kept on this computer for next time.
  type Pane = "side" | "list";
  const PANES_KEY = "novero.panes";
  const PANE_DEFAULTS = { side: 240, list: 360 };
  const PANE_LIMITS: Record<Pane, [number, number]> = { side: [180, 440], list: [260, 640] };

  function clampPane(pane: Pane, width: number): number {
    const [min, max] = PANE_LIMITS[pane];
    return Math.round(Math.min(max, Math.max(min, width)));
  }

  function loadPanes(): { side: number; list: number } {
    try {
      const saved = JSON.parse(localStorage.getItem(PANES_KEY) ?? "{}");
      return {
        side: clampPane("side", Number(saved.side) || PANE_DEFAULTS.side),
        list: clampPane("list", Number(saved.list) || PANE_DEFAULTS.list),
      };
    } catch {
      return { ...PANE_DEFAULTS };
    }
  }

  let panes = $state(loadPanes());
  let dragging = $state<Pane | null>(null);

  function savePanes() {
    try {
      localStorage.setItem(PANES_KEY, JSON.stringify(panes));
    } catch {
      // Not kept; the default widths come back next time.
    }
  }

  function startDrag(pane: Pane, event: PointerEvent) {
    if (event.button !== 0) return;
    event.preventDefault();
    const handle = event.currentTarget as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const startWidth = panes[pane];
    dragging = pane;
    const move = (e: PointerEvent) => (panes[pane] = clampPane(pane, startWidth + e.clientX - startX));
    const end = () => {
      dragging = null;
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", end);
      handle.removeEventListener("pointercancel", end);
      savePanes();
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", end);
    handle.addEventListener("pointercancel", end);
  }

  function resetPane(pane: Pane) {
    panes[pane] = PANE_DEFAULTS[pane];
    savePanes();
  }

  function paneKey(pane: Pane, event: KeyboardEvent) {
    const step = event.key === "ArrowLeft" ? -16 : event.key === "ArrowRight" ? 16 : 0;
    if (!step) return;
    event.preventDefault();
    event.stopPropagation();
    panes[pane] = clampPane(pane, panes[pane] + step);
    savePanes();
  }

  /** Typing in a field, or a dialog is open: keys belong to it. */
  function typing(event: KeyboardEvent): boolean {
    const target = event.target as HTMLElement | null;
    if (!target) return false;
    return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
  }

  function shortcut(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    const dialog = showHelp || showSettings || rulesFor || signingIn || pickingSnooze || showWhatsNew;
    // Ctrl+1 and Ctrl+2 switch between mail and the calendar.
    if (event.ctrlKey && !event.altKey && !event.shiftKey && (event.key === "1" || event.key === "2")) {
      if (dialog) return;
      event.preventDefault();
      view = event.key === "1" ? "mail" : "calendar";
      return;
    }
    // Ctrl+K searches mail from anywhere, even while typing a message.
    if (event.ctrlKey && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "k") {
      if (dialog) return;
      event.preventDefault();
      view = "mail";
      tick().then(() => sidebar?.focusSearch());
      return;
    }
    if (event.ctrlKey || event.altKey || event.metaKey) return;
    if (showHelp) {
      if (event.key === "Escape" || event.key === "?") {
        event.preventDefault();
        showHelp = false;
      }
      return;
    }
    if (showSettings || rulesFor || signingIn || pickingSnooze || showWhatsNew || typing(event)) return;
    const key = event.key;
    if (key === "?") showHelp = true;
    else if (view !== "mail") return;
    else if (key === "/") sidebar?.focusSearch();
    else if (key === "Escape" && searchQuery) sidebar?.clearSearch();
    else if (composing || outboxAccount != null) return;
    else if (key === "j" || key === "ArrowDown") list?.move(1);
    else if (key === "k" || key === "ArrowUp") list?.move(-1);
    else if (key === "c") compose();
    else if (key === "C") compose(true);
    else if (key === "r" || key === "a" || key === "f") {
      const target = reader?.replyTarget();
      if (target == null) return;
      if (key === "f") openComposer(() => api.forwardDraft(target));
      else openComposer(() => api.replyDraft(target, key === "a"));
    } else if (key === "e") archive();
    else if (key === "#" || key === "Delete") remove();
    else if (key === "s" && selected) toggle("flagged", !(reader?.rowFlags().flagged ?? selected.flags.flagged));
    else if (key === "b" && selected && selected.folderRole !== "drafts") reader?.openSnooze();
    else if (key === "U" && selected) toggle("seen", false);
    else if (key === "I" && selected) toggle("seen", true);
    else return;
    event.preventDefault();
  }
</script>

<svelte:window onkeydown={shortcut} />

<div class="shell">
<AppRail
  {view}
  unread={unreadInboxes}
  onView={(v) => (view = v)}
  onSettings={() => (showSettings = true)}
  onHelp={() => (showHelp = true)}
/>
<div class="layout" class:hidden={view !== "mail"} style:--side="{panes.side}px" style:--list="{panes.list}px" class:resizing={dragging != null} class:carrying={carrying != null}>
  <Sidebar
    {accounts}
    {folders}
    {syncState}
    bind:this={sidebar}
    selected={allInboxes || snoozedView || searchQuery ? null : selectedFolder}
    allInboxes={allInboxes && !searchQuery}
    onSelect={selectFolder}
    onFolderMenu={folderMenu}
    onNewFolder={(account) => newFolder(account)}
    {dropTarget}
    onAllInboxes={selectAllInboxes}
    {snoozedCount}
    snoozedActive={snoozedView && !searchQuery}
    onSnoozed={selectSnoozed}
    onSearch={search}
    onRules={(account) => openRules({ account })}
    onSync={(id) => api.syncAccount(id)}
    onRemove={removeAccount}
    onSignInAgain={(account) => (signingIn = account)}
    onCompose={compose}
    onOutbox={showOutbox}
    {update}
    onInstallUpdate={installUpdate}
    onDownloadUpdate={downloadUpdate}
    onWhatsNew={() => (showWhatsNew = true)}
  />
  <!-- A focusable separator is the ARIA window splitter: arrows resize it. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="splitter"
    style:left="{panes.side - 3}px"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize the folder list"
    aria-valuenow={panes.side}
    aria-valuemin={PANE_LIMITS.side[0]}
    aria-valuemax={PANE_LIMITS.side[1]}
    tabindex="0"
    title="Drag to resize. Double-click to reset."
    onpointerdown={(e) => startDrag("side", e)}
    ondblclick={() => resetPane("side")}
    onkeydown={(e) => paneKey("side", e)}
  ></div>
  <!-- A focusable separator is the ARIA window splitter: arrows resize it. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="splitter"
    style:left="{panes.side + panes.list - 3}px"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize the message list"
    aria-valuenow={panes.list}
    aria-valuemin={PANE_LIMITS.list[0]}
    aria-valuemax={PANE_LIMITS.list[1]}
    tabindex="0"
    title="Drag to resize. Double-click to reset."
    onpointerdown={(e) => startDrag("list", e)}
    ondblclick={() => resetPane("list")}
    onkeydown={(e) => paneKey("list", e)}
  ></div>
  <MessageList
    bind:this={list}
    {mailbox}
    title={listTitle}
    {emptyText}
    unit={settings.conversations ? "conversation" : "message"}
    {arriving}
    version={listVersion}
    {place}
    selected={selected?.id ?? null}
    onSelect={selectMessage}
    onDelete={remove}
    onMenu={rowMenu}
    onPress={pressRow}
  />
  {#if composing}
    {#key composing.id}
      <Composer
        bind:this={composer}
        draft={composing}
        {accounts}
        onClose={composerClosed}
        onReplace={(draft) => (composing = draft)}
        onPopOut={popOut}
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
    onMoveToNew={() => selected && newFolder(selected.accountId, null, (f) => move(f.id))}
    onReply={(message, all) => openComposer(() => api.replyDraft(message, all))}
    onForward={(message) => openComposer(() => api.forwardDraft(message))}
    {snoozeChoices}
    snoozed={snoozedView && !searchQuery}
    onUnsnooze={unsnooze}
  />
  {/if}
</div>
{#if calendarOpened}
  <div class="pane" class:hidden={view !== "calendar"}>
    <CalendarView active={view === "calendar"} blocked={!!(showHelp || showSettings || rulesFor || signingIn || showWhatsNew)} />
  </div>
{/if}
  {#if signingIn}
    <SignInAgain account={signingIn} onDone={signInDone} />
  {/if}
  {#if showSettings}
    <SettingsDialog
      {accounts}
      {settings}
      onClose={settingsClosed}
      {update}
      onInstallUpdate={installUpdate}
      onWhatsNew={() => (showWhatsNew = true)}
      onRules={() => openRules({ account: selectedFolder?.accountId ?? accounts[0].id })}
      {onAddAccount}
      onSignInAgain={(account) => (signingIn = account)}
      onRemoveAccount={removeAccount}
      onAccountChanged={onAccountsChanged}
    />
  {/if}
  {#if rulesFor && accounts.length}
    <RulesDialog {accounts} {folders} start={rulesFor} onClose={() => (rulesFor = null)} />
  {/if}
  {#if showWhatsNew && update?.update}
    <UpdateDialog
      status={update}
      onInstall={installUpdate}
      onDownload={downloadUpdate}
      onClose={() => (showWhatsNew = false)}
    />
  {/if}
  {#if crashReport}
    <CrashReport report={crashReport} onClose={crashClosed} />
  {/if}
  {#if pickingSnooze}
    <PickTime
      title="Snooze until"
      action="Snooze"
      start={laterChoices(new Date(), true)[0].at}
      onPick={(at) => {
        pickingSnooze = false;
        snooze(at);
      }}
      onCancel={() => (pickingSnooze = false)}
    />
  {/if}
  {#if showHelp}
    <ShortcutHelp onClose={() => (showHelp = false)} />
  {/if}
  {#if naming}
    {@const parent = naming.parent}
    <NameDialog
      title={naming.renaming ? "Rename folder" : "New folder"}
      action={naming.renaming ? "Rename" : "Make folder"}
      start={naming.renaming ? folderLabel(naming.renaming.name, naming.renaming.role, naming.renaming.delimiter) : ""}
      hint={parent
        ? `Inside ${parent.role === "other" ? folderPath(parent.name, parent.delimiter) : folderLabel(parent.name, parent.role, parent.delimiter)}`
        : naming.renaming
          ? ""
          : accounts.length > 1
            ? `In ${accountEmail(naming.account)}`
            : ""}
      error={namingError}
      busy={namingBusy}
      onName={named}
      onCancel={() => (naming = null)}
    />
  {/if}
  {#if carrying}
    {@const n = carrying.item.ids.length}
    <div class="drag-ghost" class:ready={dropTarget != null} style:left="{carrying.x + 14}px" style:top="{carrying.y + 10}px" aria-hidden="true">
      <Icon name="move" size={14} />
      <span>{n > 1 ? `${n} messages` : carrying.item.subject || "(no subject)"}</span>
    </div>
  {/if}
  <Toasts />
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 52px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    height: 100%;
  }
  .pane {
    min-width: 0;
    height: 100%;
  }
  .hidden {
    display: none !important;
  }
  .layout {
    display: grid;
    grid-template-columns: var(--side) var(--list) minmax(320px, 1fr);
    height: 100%;
    position: relative;
  }
  /* A thin handle on the line between two columns, easy to grab. */
  .splitter {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 6px;
    z-index: 5;
    cursor: col-resize;
    touch-action: none;
  }
  .splitter:hover,
  .splitter:focus-visible {
    background: color-mix(in srgb, var(--accent-ink) 35%, transparent);
    outline: none;
  }
  .resizing {
    cursor: col-resize;
    user-select: none;
  }
  /* The message frame would swallow the pointer while dragging over it. */
  .resizing :global(iframe),
  .carrying :global(iframe) {
    pointer-events: none;
  }
  .carrying {
    cursor: grabbing;
    user-select: none;
  }
  /* What a dragged row looks like under the pointer. */
  .drag-ghost {
    position: fixed;
    z-index: 30;
    pointer-events: none;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 260px;
    padding: 5px 10px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--bg);
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.2);
    font-size: 13px;
    opacity: 0.85;
  }
  .drag-ghost span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .drag-ghost.ready {
    opacity: 1;
    border-color: var(--accent);
  }
</style>
