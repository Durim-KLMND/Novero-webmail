<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type Account, type Settings, type Theme, type UpdateChannel, type UpdateInfo } from "./api";
  import { applyTheme } from "./theme";
  import RichEditor from "./RichEditor.svelte";
  import { textToHtml } from "./richtext";

  interface Props {
    accounts: Account[];
    settings: Settings;
    /** Closed; `saved` holds the new settings when anything was saved. */
    onClose: (saved: Settings | null, signaturesChanged: boolean) => void;
    /** An update was found with Check now. */
    onUpdate: (update: UpdateInfo) => void;
    /** Open the rules, over this dialog. */
    onRules: () => void;
  }
  let { accounts, settings, onClose, onUpdate, onRules }: Props = $props();

  // svelte-ignore state_referenced_locally
  let draft = $state<Settings>({ ...settings });
  // svelte-ignore state_referenced_locally
  let signatures = $state<Record<number, { text: string; html: string }>>(
    Object.fromEntries(accounts.map((a) => [a.id, { text: a.signature, html: a.signatureHtml }])),
  );
  let saving = $state(false);
  let error = $state("");
  let dialog = $state<HTMLDivElement>();

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
    dialog?.querySelector<HTMLElement>("input, textarea, button")?.focus();
    try {
      [version, canUpdate] = await Promise.all([api.appVersion(), api.updatesAvailable()]);
    } catch {
      // The section just shows less.
    }
  });

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

  async function save() {
    saving = true;
    error = "";
    try {
      const changed = accounts.filter((a) => {
        const sig = signatures[a.id];
        return sig && (sig.text !== a.signature || sig.html !== a.signatureHtml);
      });
      for (const a of changed) await api.setSignature(a.id, signatures[a.id].text, signatures[a.id].html);
      const saved = await api.saveSettings($state.snapshot(draft));
      onClose(saved, changed.length > 0);
    } catch (err) {
      error = errorText(err);
    } finally {
      saving = false;
    }
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="backdrop" onkeydown={keydown} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" bind:this={dialog}>
    <h2 id="settings-title">Settings</h2>

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
      <legend>Reading</legend>
      <label class="check">
        <input type="checkbox" bind:checked={draft.conversations} />
        <span>
          Group messages into conversations
          <small class="quiet">Replies are shown together with the message they answer.</small>
        </span>
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={draft.notifications} />
        <span>
          Notify me about new mail
          <small class="quiet">Shown by Windows while Novero Mail is in the background.</small>
        </span>
      </label>
    </fieldset>

    <fieldset>
      <legend>Signatures</legend>
      {#each accounts as a (a.id)}
        <div class="sig">
          <span>
            {a.displayName}
            {#if a.displayName !== a.email}<small class="quiet">{a.email}</small>{/if}
          </span>
          <RichEditor
            compact
            label="Signature for {a.email}"
            html={a.signatureHtml || textToHtml(a.signature)}
            placeholder="Added below new messages and replies"
            onChange={(text, html) => (signatures[a.id] = { text, html })}
          />
        </div>
      {/each}
    </fieldset>

    <fieldset>
      <legend>Rules</legend>
      <div class="row">
        <button onclick={onRules}>Manage rules…</button>
      </div>
      <small class="quiet">Sort new mail as it arrives, such as moving newsletters to a folder.</small>
    </fieldset>

    <fieldset>
      <legend>Updates</legend>
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

    <fieldset>
      <legend>Windows</legend>
      <div class="row">
        <button onclick={defaultApp}>Make Novero Mail the default email app…</button>
      </div>
      <small class="quiet">Opens Windows settings, where email links are set to open here.</small>
      <label class="check crash">
        <input type="checkbox" bind:checked={draft.crashReports} />
        <span>
          Offer to send a crash report after a problem
          <small class="quiet">You see each report first. Nothing is sent without asking.</small>
        </span>
      </label>
    </fieldset>

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="buttons">
      <button onclick={cancel} disabled={saving}>Cancel</button>
      <button class="primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save"}</button>
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
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  h2 {
    margin: 0 0 12px;
    font-size: 18px;
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
  .check + .check {
    margin-top: 10px;
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
  .sig {
    display: grid;
    gap: 4px;
  }
  .sig + .sig {
    margin-top: 12px;
  }
  .channel {
    margin: 10px 0 2px;
  }
  .row button {
    white-space: nowrap;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 10px 0 6px;
  }
  .check.crash {
    margin-top: 12px;
  }
  fieldset > small {
    display: block;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
