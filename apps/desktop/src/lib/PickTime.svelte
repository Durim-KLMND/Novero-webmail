<script lang="ts">
  // Picking a date and time, for "Send later" and "Snooze".
  import { onMount, untrack } from "svelte";
  import { dateValue, parseLocal, timeValue } from "./calendar";

  interface Props {
    title: string;
    action: string;
    /** Where the fields start. */
    start: Date;
    onPick: (date: Date) => void;
    onCancel: () => void;
  }
  let { title, action, start, onPick, onCancel }: Props = $props();

  let date = $state(untrack(() => dateValue(start)));
  let time = $state(untrack(() => timeValue(start)));
  let error = $state("");
  let first = $state<HTMLInputElement>();

  function pick() {
    const chosen = parseLocal(date, time);
    if (!chosen) error = "Enter a date and a time.";
    else if (chosen.getTime() <= Date.now()) error = "Pick a time in the future.";
    else onPick(chosen);
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    } else if (e.key === "Enter") {
      e.preventDefault();
      pick();
    }
  }

  onMount(() => first?.focus());
</script>

<div class="backdrop" role="presentation" onkeydown={keydown}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="pick-title">
    <h2 id="pick-title">{title}</h2>
    <div class="row">
      <input type="date" aria-label="Date" bind:value={date} bind:this={first} />
      <input type="time" aria-label="Time" step="300" bind:value={time} />
    </div>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="buttons">
      <button onclick={onCancel}>Cancel</button>
      <button class="primary" onclick={pick}>{action}</button>
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
  .row {
    display: grid;
    grid-template-columns: 1fr 120px;
    gap: 8px;
  }
  .error {
    margin: 0;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
