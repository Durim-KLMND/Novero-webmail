<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type Account, type Settings, type Theme } from "./api";
  import { applyTheme } from "./theme";

  interface Props {
    accounts: Account[];
    settings: Settings;
    /** Closed; `saved` holds the new settings when anything was saved. */
    onClose: (saved: Settings | null, signaturesChanged: boolean) => void;
  }
  let { accounts, settings, onClose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let draft = $state<Settings>({ ...settings });
  // svelte-ignore state_referenced_locally
  let signatures = $state<Record<number, string>>(Object.fromEntries(accounts.map((a) => [a.id, a.signature])));
  let saving = $state(false);
  let error = $state("");
  let dialog = $state<HTMLDivElement>();

  const themes: { value: Theme; label: string }[] = [
    { value: "system", label: "Same as Windows" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ];

  onMount(() => dialog?.querySelector<HTMLElement>("input, textarea, button")?.focus());

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
      const changed = accounts.filter((a) => (signatures[a.id] ?? "") !== a.signature);
      for (const a of changed) await api.setSignature(a.id, signatures[a.id] ?? "");
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
        <label class="sig">
          <span>
            {a.displayName}
            {#if a.displayName !== a.email}<small class="quiet">{a.email}</small>{/if}
          </span>
          <textarea rows="3" bind:value={signatures[a.id]} placeholder="Added below new messages and replies"></textarea>
        </label>
      {/each}
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
  textarea {
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    resize: vertical;
  }
  textarea:focus {
    outline: 2px solid var(--accent-ink);
    outline-offset: 1px;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
