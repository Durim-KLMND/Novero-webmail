<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api, errorText, type Account, type Folder, type Rule, type RuleAction, type RuleField } from "./api";
  import { items, openMenu, toggleMenuUnder, type MenuItem } from "./menus.svelte";
  import { folderLabel } from "./format";
  import Icon from "./Icon.svelte";
  import { toast } from "./toast.svelte";

  interface Props {
    accounts: Account[];
    folders: Record<number, Folder[]>;
    /** The account shown first, and a sender to start a new rule for. */
    start: { account: number; from?: string };
    onClose: () => void;
  }
  let { accounts, folders, start, onClose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let account = $state(start.account);
  let rules = $state<Rule[]>([]);
  let loaded = $state(false);
  let error = $state("");
  let busy = $state(false);
  let dialog = $state<HTMLDivElement>();

  /** Running rules on mail already in a folder: one rule, or all that are on. */
  let runner = $state<{ rule: Rule | null; folder: number | null } | null>(null);
  let running = $state(false);
  /** After saving, the edited rule also runs on the inbox. */
  let runAfterSave = $state(false);
  /** Folders rules can run on; drafts are left alone. */
  const sources = $derived((folders[account] ?? []).filter((f) => f.role !== "drafts"));
  const inbox = $derived((folders[account] ?? []).find((f) => f.role === "inbox"));

  /** Where a rule sends mail, as one choice in the editor. */
  type Destination = "inbox" | "move" | "archive" | "delete";

  /** The rule being edited, in the editor's terms. */
  let editing = $state<{
    rule: Rule;
    read: boolean;
    flag: boolean;
    destination: Destination;
    folder: number | null;
  } | null>(null);

  const fields: { value: RuleField; label: string }[] = [
    { value: "from", label: "From" },
    { value: "to", label: "To or Cc" },
    { value: "subject", label: "Subject" },
  ];

  /** Folders mail can be moved to: not back to the inbox, nor into drafts. */
  const targets = $derived(
    (folders[account] ?? []).filter((f) => f.role !== "inbox" && f.role !== "drafts"),
  );
  const hasArchive = $derived((folders[account] ?? []).some((f) => f.role === "archive"));

  /** The folder a new rule offers first: one of the user's own. */
  function likelyFolder(): Folder | undefined {
    return targets.find((f) => f.role === "other") ?? targets.find((f) => f.role === "archive") ?? targets[0];
  }

  async function load() {
    loaded = false;
    error = "";
    try {
      rules = await api.listRules(account);
    } catch (err) {
      rules = [];
      error = errorText(err);
    }
    loaded = true;
  }

  onMount(async () => {
    await load();
    if (start.from) edit(blank(start.from));
    else dialog?.querySelector<HTMLElement>("select, button")?.focus();
  });

  function blank(from = ""): Rule {
    return {
      id: 0,
      account,
      name: "",
      enabled: true,
      matchAll: true,
      conditions: [{ field: "from", text: from }],
      actions: [],
    };
  }

  async function edit(rule: Rule) {
    const moving = rule.actions.find((a) => a.kind === "move" || a.kind === "archive" || a.kind === "delete");
    editing = {
      rule: $state.snapshot(rule) as Rule,
      read: rule.actions.some((a) => a.kind === "markRead"),
      flag: rule.actions.some((a) => a.kind === "flag"),
      destination: (moving?.kind as Destination | undefined) ?? "inbox",
      folder: moving?.kind === "move" ? moving.folder : (likelyFolder()?.id ?? null),
    };
    // A new rule from a sender starts with something to do.
    if (!rule.id && !rule.actions.length) editing.destination = targets.length ? "move" : "inbox";
    runAfterSave = false;
    runner = null;
    error = "";
    await tick();
    dialog?.querySelector<HTMLElement>(".editor input")?.focus();
  }

  function actionsOf(e: NonNullable<typeof editing>): RuleAction[] {
    const actions: RuleAction[] = [];
    if (e.read) actions.push({ kind: "markRead" });
    if (e.flag) actions.push({ kind: "flag" });
    if (e.destination === "move" && e.folder != null) actions.push({ kind: "move", folder: e.folder });
    if (e.destination === "archive") actions.push({ kind: "archive" });
    if (e.destination === "delete") actions.push({ kind: "delete" });
    return actions;
  }

  async function saveEdit() {
    if (!editing) return;
    busy = true;
    error = "";
    try {
      const rule = { ...$state.snapshot(editing.rule), account, actions: actionsOf(editing) } as Rule;
      rule.conditions = rule.conditions.filter((c) => c.text.trim());
      const saved = await api.saveRule(rule);
      const at = rules.findIndex((r) => r.id === saved.id);
      if (at >= 0) rules[at] = saved;
      else rules.push(saved);
      editing = null;
      if (runAfterSave && inbox) await runOn(saved, inbox.id);
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function run(job: () => Promise<void>) {
    error = "";
    try {
      await job();
    } catch (err) {
      error = errorText(err);
      await load();
    }
  }

  function toggle(rule: Rule) {
    run(async () => {
      const saved = await api.saveRule({ ...$state.snapshot(rule), enabled: !rule.enabled } as Rule);
      rules = rules.map((r) => (r.id === saved.id ? saved : r));
    });
  }

  function remove(rule: Rule) {
    run(async () => {
      await api.deleteRule(rule.id);
      rules = rules.filter((r) => r.id !== rule.id);
    });
  }

  function shift(rule: Rule, step: -1 | 1) {
    const at = rules.indexOf(rule);
    const to = at + step;
    if (at < 0 || to < 0 || to >= rules.length) return;
    const next = [...rules];
    [next[at], next[to]] = [next[to], next[at]];
    rules = next;
    run(() => api.orderRules(account, next.map((r) => r.id)));
  }

  function startRun(rule: Rule | null) {
    error = "";
    runner = { rule, folder: inbox?.id ?? sources[0]?.id ?? null };
  }

  /** Runs a rule (or, with null, all that are on) and says how it went. */
  async function runOn(rule: Rule | null, folder: number) {
    running = true;
    error = "";
    try {
      const matched = await api.runRules(folder, rule?.id ?? null);
      const where = folderName(folder);
      toast(
        matched === 0
          ? `No messages in ${where} matched.`
          : `${matched} message${matched === 1 ? "" : "s"} in ${where} matched and ${matched === 1 ? "was" : "were"} sorted.`,
      );
      runner = null;
    } catch (err) {
      error = errorText(err);
    } finally {
      running = false;
    }
  }

  function folderName(id: number): string {
    const f = (folders[account] ?? []).find((f) => f.id === id);
    return f ? folderLabel(f.name, f.role, f.delimiter) : "a removed folder";
  }

  /** "From has shop.test, then mark as read and move to Deals". */
  function summary(rule: Rule): string {
    const join = rule.matchAll ? " and " : " or ";
    const when = rule.conditions
      .map((c) => `${fields.find((f) => f.value === c.field)?.label ?? c.field} has “${c.text}”`)
      .join(join);
    const does = rule.actions.map((a) => {
      switch (a.kind) {
        case "markRead":
          return "mark as read";
        case "flag":
          return "flag";
        case "move":
          return `move to ${folderName(a.folder)}`;
        case "archive":
          return "archive";
        case "delete":
          return "delete";
      }
    });
    return `${when}, then ${does.join(" and ")}`;
  }

  function ruleMenu(rule: Rule, at: number): MenuItem[] {
    return items(
      { label: "Edit", icon: "edit", run: () => edit(rule) },
      { label: rule.enabled ? "Turn off" : "Turn on", icon: "power", run: () => toggle(rule) },
      { label: "Run now on existing mail…", icon: "run", run: () => startRun(rule) },
      "separator",
      at > 0 && { label: "Move up", icon: "move-up", run: () => shift(rule, -1) },
      at < rules.length - 1 && { label: "Move down", icon: "move-down", run: () => shift(rule, 1) },
      "separator",
      { label: "Delete rule", icon: "trash", danger: true, run: () => remove(rule) },
    );
  }

  function moreMenu(rule: Rule, at: number, event: MouseEvent) {
    toggleMenuUnder(event.currentTarget as HTMLElement, ruleMenu(rule, at), true);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    if (editing) {
      editing = null;
      error = "";
    } else {
      onClose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="backdrop" onkeydown={keydown} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="rules-title" bind:this={dialog}>
    <div class="head">
      <h2 id="rules-title">{editing ? (editing.rule.id ? "Edit rule" : "New rule") : "Rules"}</h2>
      {#if accounts.length > 1 && !editing}
        <select aria-label="Account" bind:value={account} onchange={load}>
          {#each accounts as a (a.id)}<option value={a.id}>{a.email}</option>{/each}
        </select>
      {/if}
    </div>

    {#if editing}
      <div class="editor">
        <label class="field">
          <span>Name</span>
          <input bind:value={editing.rule.name} placeholder="Optional, such as “Newsletters”" />
        </label>

        <div class="when">
          <span>When</span>
          <select aria-label="Which conditions" bind:value={editing.rule.matchAll}>
            <option value={true}>all of these are true</option>
            <option value={false}>any of these is true</option>
          </select>
        </div>
        {#each editing.rule.conditions as condition, i (i)}
          <div class="condition">
            <select aria-label="Field" bind:value={condition.field}>
              {#each fields as f (f.value)}<option value={f.value}>{f.label}</option>{/each}
            </select>
            <span class="quiet">has</span>
            <input aria-label="Text" bind:value={condition.text} placeholder="such as news@shop.com" />
            <button
              class="icon-only"
              title="Remove condition"
              aria-label="Remove condition"
              disabled={editing.rule.conditions.length === 1}
              onclick={() => editing!.rule.conditions.splice(i, 1)}
            ><Icon name="close" /></button>
          </div>
        {/each}
        <button class="link add" onclick={() => editing!.rule.conditions.push({ field: "from", text: "" })}>
          <Icon name="plus" size={14} /> Add condition
        </button>

        <div class="then">
          <span>Then</span>
          <label class="check"><input type="checkbox" bind:checked={editing.read} /> Mark as read</label>
          <label class="check"><input type="checkbox" bind:checked={editing.flag} /> Flag</label>
          <div class="destination">
            <select aria-label="Where the mail goes" bind:value={editing.destination}>
              <option value="inbox">Keep in the inbox</option>
              {#if targets.length}<option value="move">Move to</option>{/if}
              {#if hasArchive}<option value="archive">Archive</option>{/if}
              <option value="delete">Delete</option>
            </select>
            {#if editing.destination === "move"}
              <select aria-label="Folder" bind:value={editing.folder}>
                {#each targets as f (f.id)}<option value={f.id}>{folderLabel(f.name, f.role, f.delimiter)}</option>{/each}
              </select>
            {/if}
          </div>
        </div>
        <small class="quiet">Runs on new mail as it arrives in the inbox.</small>
        {#if inbox}
          <label class="check">
            <input type="checkbox" bind:checked={runAfterSave} /> Also run it now on the mail already in the inbox
          </label>
        {/if}

        {#if error}<p class="error" role="alert">{error}</p>{/if}
        <div class="buttons">
          <button onclick={() => ((editing = null), (error = ""))} disabled={busy}>Cancel</button>
          <button class="primary" onclick={saveEdit} disabled={busy}>{busy ? "Saving…" : "Save rule"}</button>
        </div>
      </div>
    {:else}
      <p class="quiet intro">
        Rules sort new mail as it arrives in the inbox. Every rule that matches a message acts on it, from the top down. Run them now to sort mail you already have.
      </p>
      {#if loaded && rules.length === 0}
        <p class="empty quiet">No rules yet.</p>
      {/if}
      <ul class="rules">
        {#each rules as rule, i (rule.id)}
          <li class:off={!rule.enabled} oncontextmenu={(e) => openMenu(e, ruleMenu(rule, i))}>
            <input
              type="checkbox"
              checked={rule.enabled}
              onchange={() => toggle(rule)}
              title={rule.enabled ? "Turn off" : "Turn on"}
              aria-label="{rule.name || 'Rule'} is on"
            />
            <button class="text" onclick={() => edit(rule)}>
              {#if rule.name}<strong>{rule.name}</strong>{/if}
              <span class:quiet={!!rule.name}>{summary(rule)}</span>
            </button>
            <button class="icon-only" title="More" aria-label="More for this rule" onclick={(e) => moreMenu(rule, i, e)}>
              <Icon name="more" />
            </button>
          </li>
        {/each}
      </ul>
      {#if runner}
        <div class="runner" role="group" aria-label="Run rules now">
          <span>
            Run {runner.rule ? `“${runner.rule.name || "this rule"}”` : "every rule that is on"} on the mail in this
            folder:
          </span>
          <select aria-label="Folder to run on" bind:value={runner.folder}>
            {#each sources as f (f.id)}<option value={f.id}>{folderLabel(f.name, f.role, f.delimiter)}</option>{/each}
          </select>
          <button onclick={() => (runner = null)} disabled={running}>Cancel</button>
          <button
            class="primary"
            disabled={running || runner.folder == null}
            onclick={() => runner && runner.folder != null && runOn(runner.rule, runner.folder)}
          >{running ? "Running…" : "Run"}</button>
        </div>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="buttons">
        <button onclick={() => edit(blank())} disabled={!loaded}><Icon name="plus" /> New rule</button>
        {#if rules.some((r) => r.enabled)}
          <button onclick={() => startRun(null)} disabled={!loaded || running}><Icon name="run" /> Run rules now…</button>
        {/if}
        <span class="grow"></span>
        <button class="primary" onclick={onClose}>Done</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: grid;
    place-items: center;
    z-index: 16;
  }
  .dialog {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 8px;
  }
  .head select {
    width: auto;
    margin-left: auto;
    max-width: 60%;
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  .intro {
    margin: 0 0 12px;
  }
  .empty {
    margin: 16px 0;
    text-align: center;
  }
  .rules {
    list-style: none;
    margin: 0 0 12px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .rules:empty {
    display: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 6px 6px 12px;
  }
  li + li {
    border-top: 1px solid var(--border);
  }
  li input {
    width: auto;
    margin: 0;
    accent-color: var(--accent-ink);
  }
  li.off .text {
    opacity: 0.6;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: grid;
    text-align: left;
    border: none;
    background: none;
    padding: 4px 6px;
  }
  .text span {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .icon-only {
    border: none;
    background: none;
    padding: 6px;
    color: var(--text-quiet);
  }
  .editor {
    display: grid;
    gap: 10px;
  }
  .field {
    display: grid;
    gap: 4px;
  }
  .when,
  .condition,
  .destination {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .when select,
  .destination select,
  .condition select {
    width: auto;
  }
  .condition input {
    flex: 1;
    min-width: 0;
  }
  .add {
    justify-self: start;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .then {
    display: grid;
    gap: 8px;
    margin-top: 4px;
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: center;
    cursor: pointer;
  }
  .check input {
    width: auto;
    margin: 0;
    accent-color: var(--accent-ink);
  }
  .runner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-side);
  }
  .runner span {
    flex-basis: 100%;
  }
  .runner select {
    width: auto;
    flex: 1;
    min-width: 140px;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 6px;
  }
  .grow {
    flex: 1;
  }
</style>
