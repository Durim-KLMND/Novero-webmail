<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    api,
    errorText,
    providerLabel,
    type Account,
    type Settings,
    type Theme,
    type UpdateChannel,
    type UpdateInfo,
  } from "./api";
  import { applyTheme } from "./theme";
  import Icon, { type IconName } from "./Icon.svelte";
  import SignatureSettings from "./SignatureSettings.svelte";
  import TemplateSettings from "./TemplateSettings.svelte";
  import AccountServers from "./AccountServers.svelte";

  interface Props {
    accounts: Account[];
    settings: Settings;
    /** Closed; `saved` holds the new settings when anything was saved. */
    onClose: (saved: Settings | null, signaturesChanged: boolean) => void;
    /** An update was found with Check now. */
    onUpdate: (update: UpdateInfo) => void;
    /** Open the rules, over this dialog. */
    onRules: () => void;
    /** These leave Settings (after saving it) for a screen of their own. */
    onAddAccount: () => void;
    onSignInAgain: (account: Account) => void;
    /** Asks first; Settings stays open. */
    onRemoveAccount: (account: Account) => void;
    /** An account's server settings were changed. */
    onAccountChanged: () => void;
  }
  let {
    accounts,
    settings,
    onClose,
    onUpdate,
    onRules,
    onAddAccount,
    onSignInAgain,
    onRemoveAccount,
    onAccountChanged,
  }: Props = $props();

  type Category = "general" | "mail" | "accounts" | "signatures" | "templates" | "updates" | "privacy";
  const categories: { id: Category; label: string; icon: IconName }[] = [
    { id: "general", label: "General", icon: "settings" },
    { id: "mail", label: "Mail", icon: "inbox" },
    { id: "accounts", label: "Accounts", icon: "contacts" },
    { id: "signatures", label: "Signatures", icon: "signature" },
    { id: "templates", label: "Templates", icon: "template" },
    { id: "updates", label: "Updates", icon: "refresh" },
    { id: "privacy", label: "Privacy", icon: "junk" },
  ];

  /** The category shown last, while the app runs. */
  const LAST = "novero.settingsCategory";
  function lastCategory(): Category {
    try {
      const saved = sessionStorage.getItem(LAST);
      if (categories.some((c) => c.id === saved)) return saved as Category;
    } catch {
      // Storage may be off; start at the top.
    }
    return "general";
  }
  let category = $state<Category>(lastCategory());
  $effect(() => {
    try {
      sessionStorage.setItem(LAST, category);
    } catch {
      // Not remembered; nothing else depends on it.
    }
  });

  // svelte-ignore state_referenced_locally
  let draft = $state<Settings>({ ...settings });
  let signatures = $state<SignatureSettings>();
  let templates = $state<TemplateSettings>();
  let saving = $state(false);
  let error = $state("");
  let dialog = $state<HTMLDivElement>();
  let pane = $state<HTMLDivElement>();

  const themes: { value: Theme; label: string }[] = [
    { value: "system", label: "Same as Windows" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ];

  const channels: { value: UpdateChannel; label: string; hint: string }[] = [
    { value: "stable", label: "Stable", hint: "Tested releases" },
    { value: "beta", label: "Beta", hint: "New versions first, before they are released to everyone" },
  ];

  let version = $state("");
  let canUpdate = $state(false);
  let checking = $state(false);
  let checkResult = $state("");

  onMount(async () => {
    dialog?.querySelector<HTMLElement>(".nav button.active")?.focus();
    try {
      [version, canUpdate] = await Promise.all([api.appVersion(), api.updatesAvailable()]);
    } catch {
      // The section just shows less.
    }
  });

  async function show(next: Category) {
    category = next;
    await tick();
    pane?.scrollTo({ top: 0 });
  }

  /** Up and down move between categories, as in a list. */
  function navKey(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const at = categories.findIndex((c) => c.id === category);
    const next = categories[(at + (event.key === "ArrowDown" ? 1 : -1) + categories.length) % categories.length];
    show(next.id);
    tick().then(() => dialog?.querySelector<HTMLElement>(".nav button.active")?.focus());
  }

  async function checkNow() {
    checking = true;
    checkResult = "";
    try {
      const found = await api.checkForUpdate(draft.updateChannel);
      if (found) {
        checkResult = `Version ${found.version} is ready to install.`;
        onUpdate(found);
      } else {
        checkResult = "You have the newest version.";
      }
    } catch (err) {
      checkResult = `Couldn't check for updates. ${errorText(err)}`;
    } finally {
      checking = false;
    }
  }

  /** The account whose servers are being edited. */
  let editing = $state<number | null>(null);
  let serversSaved = $state("");

  function serversChanged(account: Account) {
    editing = null;
    serversSaved = `${account.email} now uses ${account.imap.host} and ${account.smtp.host}.`;
    onAccountChanged();
  }

  let testKind = $state<"one" | "several">("one");
  let testDelay = $state(0);
  let testing = $state(false);
  let testResult = $state("");
  let testFailed = $state(false);
  let countdown: ReturnType<typeof setInterval> | undefined;

  async function testNotification() {
    testing = true;
    testFailed = false;
    let left = testDelay;
    const tickDown = () => (testResult = `Sending in ${left}… switch to another window to see it as you would new mail.`);
    if (left > 0) {
      tickDown();
      countdown = setInterval(() => {
        left = Math.max(0, left - 1);
        tickDown();
      }, 1000);
    }
    try {
      await api.testNotification(testKind === "several", testDelay);
      testResult = "Sent. It should appear in the corner of your screen.";
    } catch (err) {
      testFailed = true;
      testResult = errorText(err);
    } finally {
      clearInterval(countdown);
      testing = false;
    }
  }
  onMount(() => () => clearInterval(countdown));

  async function notificationSettings() {
    try {
      await api.openNotificationSettings();
    } catch (err) {
      testFailed = true;
      testResult = errorText(err);
    }
  }

  async function defaultApp() {
    try {
      await api.openDefaultApps();
    } catch (err) {
      error = errorText(err);
    }
  }

  // Show the theme as soon as it is picked; Cancel puts the old one back.
  $effect(() => applyTheme(draft.theme));

  function cancel() {
    applyTheme(settings.theme);
    onClose(null, false);
  }

  /** Saves everything and closes; false when it couldn't. */
  async function save(): Promise<boolean> {
    saving = true;
    error = "";
    try {
      const changed = (await signatures?.save()) ?? false;
      await templates?.save();
      const saved = await api.saveSettings($state.snapshot(draft));
      onClose(saved, changed);
      return true;
    } catch (err) {
      error = errorText(err);
      return false;
    } finally {
      saving = false;
    }
  }

  /** Saves, then goes on to a screen of its own (adding an account, signing in). */
  async function saveThen(next: () => void) {
    if (await save()) next();
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    }
  }

  function signIn(account: Account): string {
    return account.auth.kind === "oauth2" ? `${providerLabel[account.auth.provider]} sign-in` : "Password";
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="backdrop" onkeydown={keydown} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" bind:this={dialog}>
    <nav class="nav" aria-label="Settings categories">
      <h2 id="settings-title">Settings</h2>
      {#each categories as c (c.id)}
        <button
          class:active={category === c.id}
          aria-current={category === c.id ? "page" : undefined}
          tabindex={category === c.id ? 0 : -1}
          onclick={() => show(c.id)}
          onkeydown={navKey}
        >
          <Icon name={c.icon} />
          {c.label}
        </button>
      {/each}
    </nav>

    <div class="pane" bind:this={pane}>
      <h3>{categories.find((c) => c.id === category)?.label}</h3>

      {#if category === "general"}
        <fieldset>
          <legend>Appearance</legend>
          <div class="choices">
            {#each themes as t (t.value)}
              <label class="choice">
                <input type="radio" name="theme" value={t.value} bind:group={draft.theme} />
                {t.label}
              </label>
            {/each}
          </div>
        </fieldset>
        <fieldset>
          <legend>Title bar</legend>
          <label class="check">
            <input type="checkbox" bind:checked={draft.customTitlebar} />
            <span>
              Use the Novero Mail title bar
              <small class="quiet">Matches the app's colours. Turn it off to use the standard Windows title bar.</small>
            </span>
          </label>
        </fieldset>
        <fieldset>
          <legend>Notifications</legend>
          <label class="check">
            <input type="checkbox" bind:checked={draft.notifications} />
            <span>
              Notify me about new mail
              <small class="quiet">Shown by Windows while Novero Mail is in the background.</small>
            </span>
          </label>
          <div class="row test">
            <select bind:value={testKind} aria-label="What the test shows" disabled={testing}>
              <option value="one">One new message</option>
              <option value="several">Several new messages</option>
            </select>
            <select bind:value={testDelay} aria-label="When to send it" disabled={testing}>
              <option value={0}>Now</option>
              <option value={5}>In 5 seconds</option>
              <option value={10}>In 10 seconds</option>
            </select>
            <button onclick={testNotification} disabled={testing}><Icon name="bell" /> Send a test</button>
          </div>
          {#if testResult}
            <small class:error={testFailed} class:quiet={!testFailed} role="status">{testResult}</small>
          {/if}
          <small class="quiet">
            Nothing appeared? Check that Novero Mail is allowed and Do not disturb is off in
            <button class="link" onclick={notificationSettings}>Windows notification settings</button>.
          </small>
        </fieldset>
        <fieldset>
          <legend>Default email app</legend>
          <div class="row">
            <button onclick={defaultApp}>Make Novero Mail the default email app…</button>
          </div>
          <small class="quiet">Opens Windows settings, where email links are set to open here.</small>
        </fieldset>
      {:else if category === "mail"}
        <fieldset>
          <legend>Reading</legend>
          <label class="check">
            <input type="checkbox" bind:checked={draft.conversations} />
            <span>
              Group messages into conversations
              <small class="quiet">Replies are shown together with the message they answer.</small>
            </span>
          </label>
        </fieldset>
        <fieldset>
          <legend>Rules</legend>
          <div class="row">
            <button onclick={onRules}><Icon name="rules" /> Manage rules…</button>
          </div>
          <small class="quiet">Sort new mail as it arrives, such as moving newsletters to a folder.</small>
        </fieldset>
      {:else if category === "accounts"}
        <ul class="accounts">
          {#each accounts as a (a.id)}
            <li>
              <div class="line">
                <span class="who">
                  <strong>{a.displayName}</strong>
                  <small class="quiet">{a.email} · {signIn(a)}</small>
                </span>
                <button
                  onclick={() => {
                    serversSaved = "";
                    editing = editing === a.id ? null : a.id;
                  }}
                  disabled={saving}
                  aria-expanded={editing === a.id}
                >Server settings</button>
                <button onclick={() => saveThen(() => onSignInAgain(a))} disabled={saving}>Sign in again</button>
                <button
                  class="icon-only danger"
                  onclick={() => onRemoveAccount(a)}
                  disabled={saving}
                  title="Remove {a.email}"
                  aria-label="Remove {a.email}"
                ><Icon name="remove-account" /></button>
              </div>
              {#if editing === a.id}
                <AccountServers account={a} onSaved={serversChanged} onCancel={() => (editing = null)} />
              {/if}
            </li>
          {/each}
        </ul>
        {#if serversSaved}<small class="quiet saved" role="status">{serversSaved}</small>{/if}
        <div class="row">
          <button onclick={() => saveThen(onAddAccount)} disabled={saving}><Icon name="plus" /> Add account</button>
        </div>
        <small class="quiet">Removing an account only removes it from Novero Mail. Mail on the server is not touched.</small>
      {:else if category === "signatures" || category === "templates"}
        <!-- Shown below, so edits survive switching categories. -->
      {:else if category === "updates"}
        <fieldset>
          {#if canUpdate}
            <label class="check">
              <input type="checkbox" bind:checked={draft.autoUpdate} />
              <span>
                Download updates automatically
                <small class="quiet">You choose when to restart and install them.</small>
              </span>
            </label>
            <div class="choices channel">
              {#each channels as c (c.value)}
                <label class="choice" title={c.hint}>
                  <input type="radio" name="channel" value={c.value} bind:group={draft.updateChannel} />
                  {c.label}
                </label>
              {/each}
            </div>
            <small class="quiet">{channels.find((c) => c.value === draft.updateChannel)?.hint}</small>
            <div class="row">
              <button onclick={checkNow} disabled={checking}>{checking ? "Checking…" : "Check now"}</button>
              <span class="quiet" role="status">{checkResult}</span>
            </div>
          {:else}
            <p class="quiet">This copy of Novero Mail can't update itself. Download new versions from the releases page.</p>
          {/if}
          {#if version}<small class="quiet">Novero Mail {version}</small>{/if}
        </fieldset>
      {:else if category === "privacy"}
        <fieldset>
          <label class="check">
            <input type="checkbox" bind:checked={draft.crashReports} />
            <span>
              Offer to send a crash report after a problem
              <small class="quiet">You see each report first. Nothing is sent without asking.</small>
            </span>
          </label>
        </fieldset>
      {/if}
      <div hidden={category !== "signatures"}><SignatureSettings {accounts} bind:this={signatures} /></div>
      <div hidden={category !== "templates"}><TemplateSettings bind:this={templates} /></div>
    </div>

    <div class="footer">
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="buttons">
        <button onclick={cancel} disabled={saving}>Cancel</button>
        <button class="primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save"}</button>
      </div>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: grid;
    place-items: center;
    z-index: 15;
  }
  .dialog {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: min(760px, calc(100vw - 32px));
    height: min(560px, calc(100vh - 48px));
    display: grid;
    grid-template-columns: 180px 1fr;
    grid-template-rows: 1fr auto;
    overflow: hidden;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  .nav {
    grid-row: 1 / 3;
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    padding: 16px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .nav h2 {
    margin: 0 8px 12px;
    font-size: 18px;
  }
  .nav button {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 10px;
    border: none;
    background: none;
    padding: 7px 10px;
    text-align: left;
  }
  .nav button :global(.icon) {
    color: var(--text-quiet);
  }
  .nav button.active {
    background: var(--bg-selected);
    font-weight: 600;
  }
  .nav button.active :global(.icon) {
    color: var(--accent-ink);
  }
  .pane {
    overflow-y: auto;
    padding: 18px 24px;
    min-width: 0;
  }
  h3 {
    margin: 0 0 14px;
    font-size: 16px;
  }
  .footer {
    padding: 12px 24px 16px;
    border-top: 1px solid var(--border);
    display: grid;
    gap: 10px;
  }
  .footer .error {
    margin: 0;
  }
  fieldset {
    border: none;
    padding: 0;
    margin: 0 0 18px;
  }
  legend {
    font-weight: 600;
    margin-bottom: 8px;
  }
  .choices {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
  }
  .choice,
  .check {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    cursor: pointer;
  }
  .choice input,
  .check input {
    width: auto;
    margin: 3px 0 0;
    accent-color: var(--accent-ink);
  }
  .check span {
    display: grid;
  }
  .accounts {
    list-style: none;
    margin: 0 0 4px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .accounts .line {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 8px 8px 12px;
  }
  .accounts li + li {
    border-top: 1px solid var(--border);
  }
  .who {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  .who small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .accounts button {
    white-space: nowrap;
  }
  .danger:hover:not(:disabled) {
    color: var(--danger);
  }
  .channel {
    margin: 10px 0 2px;
  }
  .row button {
    white-space: nowrap;
  }
  .test select {
    width: auto;
  }
  .saved {
    display: block;
    margin-top: 6px;
  }
  small .link {
    font-size: inherit;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 10px 0 6px;
  }
  fieldset > small,
  .pane > small {
    display: block;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  @media (max-width: 620px) {
    .dialog {
      grid-template-columns: 1fr;
      grid-template-rows: auto 1fr auto;
    }
    .nav {
      grid-row: auto;
      flex-direction: row;
      overflow-x: auto;
      border-right: none;
      border-bottom: 1px solid var(--border);
    }
    .nav h2 {
      display: none;
    }
  }
</style>
