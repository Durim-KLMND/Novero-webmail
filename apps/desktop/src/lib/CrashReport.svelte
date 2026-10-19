<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "./api";

  interface Props {
    report: string;
    /** `never` turns the question off in Settings as well. */
    onClose: (never: boolean) => void;
  }
  let { report, onClose }: Props = $props();

  let showing = $state(false);
  let error = $state("");
  let dialog = $state<HTMLDivElement>();

  onMount(() => dialog?.querySelector<HTMLElement>("button.primary")?.focus());

  async function send() {
    try {
      await api.sendCrashReport();
      onClose(false);
    } catch (err) {
      error = errorText(err);
    }
  }

  async function skip(never: boolean) {
    await api.discardCrashReport().catch(() => {});
    onClose(never);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      skip(false);
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="backdrop" onkeydown={keydown} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="crash-title" bind:this={dialog}>
    <h2 id="crash-title">Novero Mail ran into a problem</h2>
    <p>
      Sending a crash report helps get it fixed. It opens as a GitHub issue in your browser, so you can read it before you
      submit it. It says where the app failed, without your addresses, subjects or messages.
    </p>
    <button class="link" onclick={() => (showing = !showing)} aria-expanded={showing}>
      {showing ? "Hide the report" : "Show the report"}
    </button>
    {#if showing}<pre>{report}</pre>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="buttons">
      <button class="link" onclick={() => skip(true)}>Don't ask again</button>
      <span class="spacer"></span>
      <button onclick={() => skip(false)}>Don't send</button>
      <button class="primary" onclick={send}>Send report…</button>
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
    width: min(500px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  h2 {
    margin: 0 0 8px;
    font-size: 18px;
  }
  p {
    margin: 0 0 10px;
  }
  pre {
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    background: var(--bg-side);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    max-height: 200px;
    overflow-y: auto;
  }
  .buttons {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 16px;
  }
  .spacer {
    flex: 1;
  }
</style>
