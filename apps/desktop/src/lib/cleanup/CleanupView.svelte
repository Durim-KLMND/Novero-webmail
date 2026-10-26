<script lang="ts">
  // The cleanup view on the rail: every account's mail sorted into
  // categories by sender, guessed on this computer from headers and sender
  // history. Suggestions can be accepted in bulk, senders put in another
  // category, and their mail moved to Junk, deleted or archived.
  import { onDestroy } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api,
    errorText,
    onCleanupProgress,
    onMailEvent,
    type Account,
    type Category,
    type CleanupItem,
    type CleanupReport,
    type CleanupScope,
    type SenderGroup,
  } from "../api";
  import { listDate } from "../format";
  import { toggleMenuUnder, openMenu, type MenuItem } from "../shell/menus.svelte";
  import { toast } from "../shell/toast.svelte";
  import Icon from "../ui/Icon.svelte";
  import { accept, categories, categoryOf, groupKey, senderLabel, why } from "./categories";
  import FoldersDialog from "./FoldersDialog.svelte";

  interface Props {
    accounts: Account[];
    /** The view is on screen: it loads the first time, and follows mail changes. */
    active: boolean;
    /** Opens a new message to a list's mailto: unsubscribe address. */
    onCompose: (account: number, link: string) => void;
  }
  let { accounts, active, onCompose }: Props = $props();

  type Section = "review" | "all" | Category;
  const PREFS_KEY = "novero.cleanup";
  const PAGE = 150;

  let prefs = readPrefs();
  let scope = $state<CleanupScope>(prefs.scope);
  let account = $state<number | null>(null);
  let section = $state<Section>("review");
  let query = $state("");
  let report = $state<CleanupReport | null>(null);
  let loadError = $state("");
  let loading = $state(false);
  /** Reading list headers of older mail: how far along. */
  let checking = $state<{ done: number; total: number } | null>(null);
  let selected = $state<Set<string>>(new Set());
  let open = $state<string | null>(null);
  let items = $state<CleanupItem[] | null>(null);
  let shownCount = $state(PAGE);
  let busy = $state(false);
  let loaded = false;

  function readPrefs(): { scope: CleanupScope } {
    try {
      const saved = JSON.parse(localStorage.getItem(PREFS_KEY) ?? "{}");
      return { scope: saved.scope === "allMail" ? "allMail" : "inbox" };
    } catch {
      return { scope: "inbox" };
    }
  }

  function savePrefs() {
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify({ scope }));
    } catch {
      // Remembering the scope is only a convenience.
    }
  }

  const groups = $derived(report?.groups ?? []);
  const counts = $derived.by(() => {
    const out: Record<string, number> = { review: 0, all: 0 };
    for (const c of categories) out[c.id] = 0;
    for (const g of groups) {
      out.all += g.ids.length;
      out[g.category] += g.ids.length;
      if (!g.confirmed) out.review += g.ids.length;
    }
    return out;
  });
  const visible = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return groups.filter(
      (g) =>
        (section === "all" || (section === "review" ? !g.confirmed : g.category === section)) &&
        (!q || g.email.includes(q) || (g.name ?? "").toLowerCase().includes(q) || (g.subject ?? "").toLowerCase().includes(q)),
    );
  });
  const picked = $derived(visible.filter((g) => selected.has(groupKey(g))));
  const pickedMessages = $derived(picked.reduce((n, g) => n + g.ids.length, 0));
  const unconfirmed = $derived(visible.filter((g) => !g.confirmed));
  const allPicked = $derived(visible.length > 0 && picked.length === visible.length);
  const heading = $derived(
    section === "review" ? "To review" : section === "all" ? "All senders" : categoryOf[section].label,
  );
  const about = $derived(
    section === "review"
      ? "Suggested categories you haven't confirmed yet. Accept them, or pick another category for a sender."
      : section === "all"
        ? "Everyone who sent you mail, biggest senders first."
        : categoryOf[section].about,
  );

  async function load() {
    loading = true;
    try {
      report = await api.cleanupReport(scope, account);
      loadError = "";
      // Selections and the open group stay only while they still exist.
      const keys = new Set(report.groups.map(groupKey));
      selected = new Set([...selected].filter((k) => keys.has(k)));
      if (open && !keys.has(open)) {
        open = null;
        items = null;
      } else if (open) {
        loadItems(report.groups.find((g) => groupKey(g) === open)!);
      }
      if (report.unchecked > 0 && !checking) checkHeaders();
    } catch (err) {
      loadError = errorText(err);
    } finally {
      loading = false;
    }
  }

  /** Reads list headers older mail was cached without, then sorts again. */
  async function checkHeaders() {
    checking = { done: 0, total: report?.unchecked ?? 0 };
    try {
      await api.checkListHeaders(scope, account);
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      checking = null;
    }
    report = await api.cleanupReport(scope, account).catch(() => report);
  }

  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  /** Mail changed: sort again once things settle, if the view is open. */
  function scheduleLoad() {
    if (!loaded) return;
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => {
      if (active) load();
      else stale = true;
    }, 800);
  }
  let stale = false;

  $effect(() => {
    if (active && (!loaded || stale)) {
      loaded = true;
      stale = false;
      load();
    }
  });

  const unlisteners = [
    onMailEvent((e) => {
      if (e.type === "mailboxChanged" || e.type === "syncFinished") scheduleLoad();
    }),
    onCleanupProgress((p) => {
      if (checking) checking = p;
    }),
  ];
  onDestroy(() => {
    clearTimeout(refreshTimer);
    for (const u of unlisteners) u.then((f) => f());
  });

  function setScope(next: CleanupScope) {
    if (scope === next) return;
    scope = next;
    savePrefs();
    shownCount = PAGE;
    load();
  }

  function setAccount(value: string) {
    account = value ? Number(value) : null;
    shownCount = PAGE;
    load();
  }

  function show(next: Section) {
    section = next;
    selected = new Set();
    shownCount = PAGE;
  }

  function toggle(g: SenderGroup) {
    const key = groupKey(g);
    const next = new Set(selected);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    selected = next;
  }

  function toggleAll() {
    selected = allPicked ? new Set() : new Set(visible.map(groupKey));
  }

  async function loadItems(g: SenderGroup) {
    try {
      const found = await api.cleanupItems(g.ids);
      if (open === groupKey(g)) items = found;
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  function expand(g: SenderGroup) {
    const key = groupKey(g);
    if (open === key) {
      open = null;
      items = null;
      return;
    }
    open = key;
    items = null;
    loadItems(g);
  }

  /** Runs a change, then sorts again; one at a time. */
  async function run(change: () => Promise<unknown>, done?: string) {
    if (busy) return;
    busy = true;
    try {
      await change();
      if (done) toast(done);
      selected = new Set();
      await load();
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      busy = false;
    }
  }

  const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

  function acceptGroups(list: SenderGroup[]) {
    const n = list.reduce((sum, g) => sum + g.ids.length, 0);
    run(() => accept(list, groups), `Accepted for ${plural(n, "message")}.`);
  }

  /** Puts senders in a category: the whole sender, or just the group when it has no address. */
  function recategorise(list: SenderGroup[], category: Category) {
    run(async () => {
      for (const g of list) {
        if (g.email) await api.setSenderCategory(g.email, category);
        else await api.setMessageCategory(g.ids, category);
      }
    }, `${list.length === 1 ? senderLabel(list[0]) : plural(list.length, "sender")} now in ${categoryOf[category].label}.`);
  }

  function forget(g: SenderGroup) {
    run(async () => {
      if (g.email) await api.setSenderCategory(g.email, null);
      await api.setMessageCategory(g.ids, null);
    });
  }

  const idsOf = (list: SenderGroup[]) => list.flatMap((g) => g.ids);

  function junk(list: SenderGroup[]) {
    const ids = idsOf(list);
    run(async () => {
      // Moving them to Junk also says what they are.
      for (const g of list) {
        if (g.email && g.category !== "spam") await api.setSenderCategory(g.email, "spam");
      }
      await api.junkMessages(ids);
    }, `${plural(ids.length, "message")} moved to Junk.`);
  }

  async function remove(list: SenderGroup[]) {
    const ids = idsOf(list);
    const who = list.length === 1 ? `from ${senderLabel(list[0])}` : `from ${plural(list.length, "sender")}`;
    const sure = await ask(`Move ${plural(ids.length, "message")} ${who} to Trash?`, {
      title: "Delete mail",
      kind: "warning",
      okLabel: "Delete",
      cancelLabel: "Keep them",
    });
    if (sure) run(() => api.deleteMessages(ids), `${plural(ids.length, "message")} moved to Trash.`);
  }

  function archive(list: SenderGroup[]) {
    const ids = idsOf(list);
    run(() => api.archiveMessages(ids), `${plural(ids.length, "message")} archived.`);
  }

  function markRead(list: SenderGroup[]) {
    run(() => api.setFlag(idsOf(list), "seen", true));
  }

  async function unsubscribe(g: SenderGroup) {
    const link = g.unsubscribe;
    if (!link) return;
    if (/^mailto:/i.test(link)) {
      const sure = await ask(`Write to ${senderLabel(g)} to leave their list? The message opens for you to send.`, {
        title: "Unsubscribe",
        okLabel: "Write it",
        cancelLabel: "Cancel",
      });
      if (sure) onCompose(g.accounts[0] ?? accounts[0].id, link);
      return;
    }
    const sure = await ask(`Open ${senderLabel(g)}'s unsubscribe page in your browser?\n\n${link}`, {
      title: "Unsubscribe",
      okLabel: "Open page",
      cancelLabel: "Cancel",
    });
    if (sure) openUrl(link).catch((err) => toast(errorText(err), "error"));
  }

  function categoryItems(current: Category | null, pick: (c: Category) => void): MenuItem[] {
    return categories.map((c) => ({
      label: c.label,
      icon: c.id === current ? "check" : c.icon,
      run: () => pick(c.id),
    }));
  }

  function groupItems(g: SenderGroup): MenuItem[] {
    return [
      !g.confirmed && { label: `Accept ${categoryOf[g.category].label}`, icon: "check", run: () => acceptGroups([g]) },
      { label: "Put sender in", icon: "rules", items: categoryItems(g.category, (c) => recategorise([g], c)) },
      g.confirmed && { label: "Go back to the suggestion", icon: "rotate", run: () => forget(g) },
      "separator",
      g.unsubscribe && { label: "Unsubscribe…", icon: "external-link", run: () => unsubscribe(g) },
      g.unread > 0 && { label: "Mark all read", icon: "read", run: () => markRead([g]) },
      { label: "Archive all", icon: "archive", run: () => archive([g]) },
      { label: "Move all to Junk", icon: "junk", run: () => junk([g]) },
      { label: "Delete all…", icon: "trash", danger: true, run: () => remove([g]) },
    ].filter(Boolean) as MenuItem[];
  }

  function itemMenu(event: MouseEvent, item: CleanupItem) {
    openMenu(event, [
      ...categoryItems(item.category, (c) =>
        run(() => api.setMessageCategory([item.id], c), `Moved to ${categoryOf[c].label}.`),
      ),
      "separator",
      { label: "Move to Junk", icon: "junk", run: () => run(() => api.junkMessages([item.id])) },
      { label: "Delete", icon: "trash", danger: true, run: () => run(() => api.deleteMessages([item.id])) },
    ]);
  }

  // Category folders on the mail servers.
  let folderAccounts = $state<number[]>([]);
  let askingFolders = $state(false);
  let makingFolders = $state(false);
  const withoutFolders = $derived(accounts.filter((a) => !folderAccounts.includes(a.id)));

  async function loadFolderAccounts() {
    folderAccounts = await api.categoryFolderAccounts().catch(() => folderAccounts);
  }
  $effect(() => {
    if (active) loadFolderAccounts();
  });

  async function makeFolders() {
    makingFolders = true;
    try {
      const made = await api.makeCategoryFolders(withoutFolders.map((a) => a.id));
      askingFolders = false;
      for (const f of made.failed) toast(`Folders couldn't be added for ${f}`, "error");
      if (made.failed.length < withoutFolders.length) {
        toast(made.filed ? `Folders added. ${plural(made.filed, "message")} filed into them.` : "Folders added.");
      }
      await loadFolderAccounts();
      await load();
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      makingFolders = false;
    }
  }

  function fileNow() {
    run(async () => {
      const filed = await api.fileIntoCategoryFolders();
      toast(filed ? `${plural(filed, "message")} filed into category folders.` : "Nothing new to file.");
    });
  }

  async function stopFolders() {
    const sure = await ask("New mail will stay in the Inbox. The folders and the mail in them stay on the server.", {
      title: "Stop filing into folders",
      kind: "info",
      okLabel: "Stop filing",
      cancelLabel: "Keep filing",
    });
    if (!sure) return;
    try {
      await api.stopCategoryFolders();
      await loadFolderAccounts();
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  const accountEmail = (id: number) => accounts.find((a) => a.id === id)?.email ?? "";
</script>

<div class="cleanup">
  <aside class="side">
    <div class="scope" role="group" aria-label="Which mail to look through">
      <button class:on={scope === "inbox"} aria-pressed={scope === "inbox"} onclick={() => setScope("inbox")}>Inboxes</button>
      <button class:on={scope === "allMail"} aria-pressed={scope === "allMail"} onclick={() => setScope("allMail")} title="Inboxes, Archive and your own folders">All mail</button>
    </div>
    {#if accounts.length > 1}
      <select aria-label="Account" value={account ?? ""} onchange={(e) => setAccount(e.currentTarget.value)}>
        <option value="">All accounts</option>
        {#each accounts as a (a.id)}
          <option value={a.id}>{a.email}</option>
        {/each}
      </select>
    {/if}
    <nav aria-label="Categories">
      <button class="place" class:current={section === "review"} aria-current={section === "review" ? "page" : undefined} onclick={() => show("review")}>
        <Icon name="review" /><span>To review</span>
        {#if counts.review}<span class="count">{counts.review.toLocaleString()}</span>{/if}
      </button>
      <button class="place" class:current={section === "all"} aria-current={section === "all" ? "page" : undefined} onclick={() => show("all")}>
        <Icon name="all-inboxes" /><span>All senders</span>
        {#if counts.all}<span class="count">{counts.all.toLocaleString()}</span>{/if}
      </button>
      <span class="divider" role="separator"></span>
      {#each categories as c (c.id)}
        <button class="place" class:current={section === c.id} aria-current={section === c.id ? "page" : undefined} onclick={() => show(c.id)}>
          <span class="dot cat-{c.id}"><Icon name={c.icon} /></span><span>{c.label}</span>
          {#if counts[c.id]}<span class="count">{counts[c.id].toLocaleString()}</span>{/if}
        </button>
      {/each}
    </nav>
    <div class="server-folders">
      <strong><Icon name="folder" size={14} /> Folders on your mail server</strong>
      {#if folderAccounts.length}
        <small class="quiet">Sorted Inbox mail is filed into a Categories folder, also seen in webmail and on your phone.</small>
        <div class="folder-actions">
          <button disabled={busy} onclick={fileNow}>File Inbox now</button>
          {#if withoutFolders.length}
            <button onclick={() => (askingFolders = true)}>Add to new accounts…</button>
          {/if}
          <button class="link" onclick={stopFolders}>Stop filing</button>
        </div>
      {:else}
        <small class="quiet">File sorted mail into category folders that webmail and your phone see too.</small>
        <div class="folder-actions">
          <button disabled={!accounts.length} onclick={() => (askingFolders = true)}>Add folders…</button>
        </div>
      {/if}
    </div>
    <p class="local quiet">Sorted on this computer from each message's sender and headers. Nothing is sent anywhere.</p>
  </aside>

  <section class="main">
    <header>
      <div class="titles">
        <h1>{heading}</h1>
        <p class="quiet about">{about}</p>
      </div>
      <span class="grow"></span>
      <div class="find">
        <Icon name="search" size={15} />
        <input type="search" placeholder="Find a sender" aria-label="Find a sender" bind:value={query} />
      </div>
      <button class="icon-only" aria-label="Sort again" title="Sort again" disabled={loading || busy} onclick={load}>
        <Icon name="refresh" size={16} />
      </button>
    </header>

    {#if checking}
      <div class="checking" role="status">
        <span>Reading list headers of older mail… {checking.done.toLocaleString()} of {checking.total.toLocaleString()}</span>
        <progress max={Math.max(checking.total, 1)} value={checking.done}></progress>
      </div>
    {/if}

    <div class="bar">
      <label class="check-all">
        <input type="checkbox" checked={allPicked} indeterminate={picked.length > 0 && !allPicked} disabled={!visible.length} onchange={toggleAll} />
        <span>{picked.length ? `${plural(picked.length, "sender")} · ${plural(pickedMessages, "message")}` : plural(visible.length, "sender")}</span>
      </label>
      <span class="grow"></span>
      {#if picked.length}
        {@const unconfirmedPicked = picked.filter((g) => !g.confirmed)}
        {#if unconfirmedPicked.length}
          <button class="primary" disabled={busy} onclick={() => acceptGroups(unconfirmedPicked)}><Icon name="check" /> Accept</button>
        {/if}
        <button disabled={busy} onclick={(e) => toggleMenuUnder(e.currentTarget, categoryItems(null, (c) => recategorise(picked, c)))}>
          Put in <Icon name="chevron-down" size={14} />
        </button>
        <button disabled={busy} onclick={() => archive(picked)}><Icon name="archive" /> Archive</button>
        <button disabled={busy} onclick={() => junk(picked)}><Icon name="junk" /> Junk</button>
        <button class="danger" disabled={busy} onclick={() => remove(picked)}><Icon name="trash" /> Delete</button>
      {:else if unconfirmed.length}
        <button class="primary" disabled={busy} onclick={() => acceptGroups(unconfirmed)}>
          <Icon name="check" /> {unconfirmed.length > 1 ? `Accept all ${unconfirmed.length.toLocaleString()} suggestions` : "Accept the suggestion"}
        </button>
      {/if}
    </div>

    <div class="list">
      {#if loadError}
        <p class="error">{loadError}</p>
      {:else if !report}
        <div class="blank"><span class="spinner"></span></div>
      {:else if !visible.length}
        <div class="blank quiet">
          <Icon name="cleanup" size={36} />
          {#if query}
            <p>No senders match “{query}”.</p>
          {:else if section === "review"}
            <p>Nothing left to review. New mail gets a suggestion here as it arrives.</p>
          {:else}
            <p>No mail here.</p>
          {/if}
        </div>
      {:else}
        <ul>
          {#each visible.slice(0, shownCount) as g (groupKey(g))}
            {@const key = groupKey(g)}
            {@const c = categoryOf[g.category]}
            <li class="group" class:picked={selected.has(key)} class:open={open === key}>
              <div class="row" role="presentation" oncontextmenu={(e) => openMenu(e, groupItems(g))}>
                <input type="checkbox" aria-label="Select {senderLabel(g)}" checked={selected.has(key)} onchange={() => toggle(g)} />
                <button class="expand icon-only" aria-expanded={open === key} aria-label="Show messages from {senderLabel(g)}" onclick={() => expand(g)}>
                  <Icon name={open === key ? "chevron-down" : "chevron-right"} size={16} />
                </button>
                <button class="who" onclick={() => expand(g)}>
                  <span class="name">
                    <strong>{senderLabel(g)}</strong>
                    {#if g.name && g.email}<span class="quiet addr selectable">{g.email}</span>{/if}
                  </span>
                  <span class="quiet subject">
                    {g.subject || "(no subject)"}
                  </span>
                </button>
                <span class="numbers">
                  <span>{plural(g.ids.length, "message")}</span>
                  <span class="quiet">{g.unread ? `${g.unread.toLocaleString()} unread · ` : ""}{listDate(g.latest)}</span>
                </span>
                <span class="verdict">
                  <button
                    class="chip cat-{g.category}"
                    class:suggested={!g.confirmed}
                    title={g.confirmed ? `${c.label}: your choice. Click to change.` : `Suggested: ${c.label}. Click to change.`}
                    onclick={(e) => toggleMenuUnder(e.currentTarget, categoryItems(g.category, (next) => recategorise([g], next)))}
                  >
                    <Icon name={c.icon} size={14} />
                    {c.label}
                    {#if !g.confirmed && !g.sure}<span class="maybe">?</span>{/if}
                  </button>
                  {#if !g.confirmed}
                    <span class="why quiet" title={why(g.reasons, g.sure)}>{why(g.reasons, g.sure)}</span>
                  {:else}
                    <span class="why quiet">{g.senderCategory ? "Your choice for this sender" : "Your choice"}</span>
                  {/if}
                </span>
                <span class="actions">
                  {#if !g.confirmed}
                    <button class="icon-only accept" aria-label="Accept {c.label} for {senderLabel(g)}" title="Accept {c.label}" disabled={busy} onclick={() => acceptGroups([g])}>
                      <Icon name="check" size={16} />
                    </button>
                  {/if}
                  <button class="icon-only" aria-label="Move mail from {senderLabel(g)} to Junk" title="Move all to Junk" disabled={busy} onclick={() => junk([g])}>
                    <Icon name="junk" size={16} />
                  </button>
                  <button class="icon-only" aria-label="More for {senderLabel(g)}" title="More" onclick={(e) => toggleMenuUnder(e.currentTarget, groupItems(g), true)}>
                    <Icon name="more" size={16} />
                  </button>
                </span>
              </div>
              {#if open === key}
                <div class="items">
                  {#if accounts.length > 1}
                    <p class="quiet in">In {g.accounts.map(accountEmail).join(", ")}</p>
                  {/if}
                  {#if !items}
                    <span class="spinner"></span>
                  {:else}
                    <ul>
                      {#each items as item (item.id)}
                        <li class="item" oncontextmenu={(e) => itemMenu(e, item)}>
                          <span class="unread-dot" class:on={item.unread} aria-label={item.unread ? "Unread" : undefined}></span>
                          <span class="item-subject selectable" class:bold={item.unread}>{item.subject || "(no subject)"}</span>
                          <span class="quiet item-date">{listDate(item.date)}</span>
                          <button
                            class="chip small cat-{item.category}"
                            class:suggested={!item.confirmed}
                            title={why(item.reasons, item.sure) || "Your choice"}
                            onclick={(e) => itemMenu(e, item)}
                          >
                            <Icon name={categoryOf[item.category].icon} size={12} />
                            {categoryOf[item.category].label}
                          </button>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
        {#if visible.length > shownCount}
          <button class="more" onclick={() => (shownCount += PAGE)}>Show {Math.min(PAGE, visible.length - shownCount)} more senders</button>
        {/if}
      {/if}
    </div>
  </section>
</div>

{#if askingFolders}
  <FoldersDialog accounts={withoutFolders} busy={makingFolders} onMake={makeFolders} onCancel={() => (askingFolders = false)} />
{/if}

<style>
  .cleanup {
    display: grid;
    grid-template-columns: 232px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    height: 100%;
    min-width: 0;
  }

  .side {
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
  }
  .scope {
    display: grid;
    grid-template-columns: 1fr 1fr;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 2px;
    background: var(--bg);
  }
  .scope button {
    border: none;
    background: none;
    padding: 5px 8px;
    color: var(--text-quiet);
  }
  .scope button.on {
    background: var(--bg-selected);
    color: var(--accent-ink);
    font-weight: 600;
  }
  nav {
    display: grid;
    gap: 2px;
  }
  .divider {
    height: 1px;
    background: var(--border);
    margin: 6px 4px;
  }
  .place {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 10px;
    border: none;
    background: none;
    padding: 7px 10px;
    border-radius: var(--radius);
    color: var(--text);
    text-align: left;
  }
  .place > span:not(.dot):not(.count) {
    flex: 1;
  }
  .place.current {
    background: var(--bg-selected);
    color: var(--accent-ink);
    font-weight: 600;
  }
  .dot {
    display: inline-flex;
    color: var(--cat);
  }
  .count {
    font-size: 12px;
    color: var(--text-quiet);
    font-weight: 400;
  }
  .server-folders {
    margin-top: auto;
    display: grid;
    gap: 6px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 13px;
  }
  .server-folders strong {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .server-folders small {
    font-size: 12px;
  }
  .folder-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  .folder-actions button {
    font-size: 12px;
    padding: 4px 10px;
  }
  .folder-actions .link {
    border: none;
    background: none;
    color: var(--text-quiet);
    text-decoration: underline;
    padding: 4px 2px;
  }
  .local {
    margin: 0;
    font-size: 12px;
  }

  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    min-height: 53px;
    border-bottom: 1px solid var(--border);
  }
  .titles {
    min-width: 0;
  }
  h1 {
    font-size: 18px;
    margin: 0;
    font-weight: 600;
  }
  .about {
    margin: 2px 0 0;
    font-size: 12px;
  }
  .grow {
    flex: 1;
  }
  .find {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    width: 220px;
    flex-shrink: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-quiet);
  }
  .find:focus-within {
    outline: 2px solid var(--accent-ink);
    outline-offset: 1px;
  }
  .find input {
    border: none;
    padding: 6px 0;
    background: none;
    outline: none;
  }
  .checking {
    display: grid;
    gap: 6px;
    padding: 10px 16px;
    font-size: 13px;
    border-bottom: 1px solid var(--border);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    min-height: 50px;
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
  }
  .bar button {
    padding: 5px 12px;
  }
  .bar .danger {
    color: var(--danger);
  }
  .check-all {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
    color: var(--text-quiet);
  }
  input[type="checkbox"] {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--accent);
    flex-shrink: 0;
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .group {
    border-bottom: 1px solid var(--border);
  }
  .group.picked {
    background: var(--bg-selected);
  }
  .row {
    display: grid;
    grid-template-columns: 16px 24px minmax(160px, 1.4fr) 120px minmax(170px, 1.2fr) 96px;
    align-items: center;
    gap: 10px;
    padding: 8px 12px 8px 16px;
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .group.picked .row:hover {
    background: var(--bg-selected);
  }
  .expand {
    padding: 4px;
  }
  .who {
    display: grid;
    gap: 1px;
    min-width: 0;
    border: none;
    background: none;
    padding: 0;
    text-align: left;
  }
  .who:hover:not(:disabled) {
    background: none;
  }
  .name,
  .subject {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .addr {
    margin-left: 6px;
    font-size: 12px;
  }
  .subject {
    font-size: 13px;
  }
  .numbers {
    display: grid;
    font-size: 13px;
    white-space: nowrap;
  }
  .numbers .quiet {
    font-size: 12px;
  }
  .verdict {
    display: grid;
    justify-items: start;
    gap: 3px;
    min-width: 0;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 10px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--cat) 45%, transparent);
    background: color-mix(in srgb, var(--cat) 12%, var(--bg));
    color: var(--text);
    font-size: 13px;
    font-weight: 600;
  }
  .chip :global(svg) {
    color: var(--cat);
  }
  .chip:hover:not(:disabled) {
    background: color-mix(in srgb, var(--cat) 20%, var(--bg));
  }
  /* A suggestion not yet accepted: outlined, not filled. */
  .chip.suggested {
    background: var(--bg);
    border-style: dashed;
    font-weight: 500;
  }
  .chip.small {
    font-size: 12px;
    padding: 1px 8px;
  }
  .maybe {
    color: var(--text-quiet);
  }
  .why {
    font-size: 12px;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
  }
  .accept {
    color: var(--accent-ink);
  }
  .items {
    padding: 0 16px 10px 66px;
  }
  .in {
    margin: 0 0 6px;
    font-size: 12px;
  }
  .item {
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 10px;
    padding: 4px 0;
    font-size: 13px;
    border-top: 1px solid var(--border);
  }
  .unread-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .unread-dot.on {
    background: var(--accent-ink);
  }
  .item-subject {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bold {
    font-weight: 600;
  }
  .item-date {
    font-size: 12px;
  }
  .blank {
    display: grid;
    place-items: center;
    align-content: center;
    gap: 10px;
    height: 100%;
    min-height: 240px;
    text-align: center;
    padding: 24px;
  }
  .blank p {
    margin: 0;
    max-width: 360px;
  }
  .more {
    display: block;
    margin: 12px auto;
  }
  @media (max-width: 1100px) {
    .row {
      grid-template-columns: 16px 24px minmax(140px, 1fr) minmax(150px, 1fr) 96px;
    }
    .numbers {
      display: none;
    }
  }
</style>
