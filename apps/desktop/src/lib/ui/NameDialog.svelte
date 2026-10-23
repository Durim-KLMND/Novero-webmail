<script lang="ts">
  // Asking for a name, e.g. for a new template.
  import { onMount, untrack } from "svelte";

  interface Props {
    title: string;
    action: string;
    start?: string;
    /** A line under the title, e.g. where a new folder goes. */
    hint?: string;
    /** Why the last name was not taken; the dialog stays open to fix it. */
    error?: string;
    /** The name is being saved. */
    busy?: boolean;
    onName: (name: string) => void;
    onCancel: () => void;
  }
  let { title, action, start = "", hint = "", error = "", busy = false, onName, onCancel }: Props = $props();

  let name = $state(untrack(() => start));
  let input = $state<HTMLInputElement>();

  function done() {
    if (name.trim() && !busy) onName(name.trim());
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    } else if (e.key === "Enter") {
      e.preventDefault();
      done();
    }
  }

  onMount(() => input?.select());
</script>

<div class="backdrop" role="presentation" onkeydown={keydown}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="name-title">
    <h2 id="name-title">{title}</h2>
    {#if hint}<p class="quiet hint">{hint}</p>{/if}
    <input aria-label="Name" bind:value={name} bind:this={input} aria-invalid={!!error} />
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="buttons">
      <button onclick={onCancel}>Cancel</button>
      <button class="primary" disabled={!name.trim() || busy} onclick={done}>{action}</button>
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
    z-index: 17;
  }
  .dialog {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: min(360px, calc(100vw - 32px));
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
    display: grid;
    gap: 12px;
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  .hint,
  .error {
    margin: -4px 0 0;
    font-size: 13px;
  }
  .error {
    color: var(--danger);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
