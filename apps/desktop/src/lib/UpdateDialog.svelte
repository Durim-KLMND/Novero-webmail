<script lang="ts">
  // What's new in an update, with the button that installs it (or downloads
  // it first, when automatic downloads are off).
  import { onMount } from "svelte";
  import type { UpdateStatus } from "./api";
  import { downloadPercent, downloadText, releaseNotes } from "./notes";
  import { outsideLicense, pro } from "./pro.svelte";

  interface Props {
    status: UpdateStatus;
    onInstall: () => void;
    onDownload: () => void;
    onClose: () => void;
  }
  let { status, onInstall, onDownload, onClose }: Props = $props();

  let dialog = $state<HTMLDivElement>();
  const notes = $derived(releaseNotes(status.update?.notes ?? null));
  /** Installing it would turn Pro off. */
  const beyondLicense = $derived(outsideLicense(pro.plan, status.update?.released ?? null));
  const percent = $derived(downloadPercent(status.downloaded, status.total));

  onMount(() => dialog?.querySelector<HTMLElement>("button.primary, button")?.focus());

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="backdrop" onkeydown={keydown} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="update-title" bind:this={dialog}>
    <h2 id="update-title">What's new in Novero Mail {status.update?.version}</h2>
    {#if notes.length}
      <div class="notes selectable">
        {#each notes as line, i (i)}
          {#if line.kind === "heading"}
            <h3>{line.text}</h3>
          {:else if line.kind === "item"}
            <p class="item">{line.text}</p>
          {:else}
            <p>{line.text}</p>
          {/if}
        {/each}
      </div>
    {:else}
      <p class="quiet">This release has no notes.</p>
    {/if}
    {#if beyondLicense}
      <p class="warn" role="note">
        This version came out after the year of updates your Pro license includes. After updating, Novero Mail
        works as Free until you buy a new key.
      </p>
    {/if}
    {#if status.phase === "downloading"}
      <progress max="100" value={percent} aria-label="Download progress"></progress>
      <small class="quiet">Downloading… {downloadText(status.downloaded, status.total)}</small>
    {/if}
    <div class="buttons">
      {#if status.phase === "ready"}
        <small class="quiet">Messages you are writing are saved first.</small>
      {/if}
      <span class="spacer"></span>
      <button onclick={onClose}>Later</button>
      {#if status.phase === "available"}
        <button class="primary" onclick={onDownload}>Download</button>
      {:else if status.phase === "ready" || status.phase === "installing"}
        <button class="primary" onclick={onInstall} disabled={status.phase === "installing"}>
          {status.phase === "installing" ? "Restarting…" : "Restart to update"}
        </button>
      {/if}
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
    /* Over Settings, which can open it. */
    z-index: 16;
  }
  .dialog {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  h2 {
    margin: 0 0 12px;
    font-size: 18px;
  }
  .notes {
    overflow-y: auto;
    scrollbar-gutter: stable;
    min-height: 0;
    margin-bottom: 8px;
  }
  h3 {
    margin: 12px 0 6px;
    font-size: 14px;
  }
  h3:first-child {
    margin-top: 0;
  }
  p {
    margin: 0 0 6px;
  }
  .item {
    position: relative;
    padding-left: 16px;
  }
  .item::before {
    content: "";
    position: absolute;
    left: 4px;
    top: 0.6em;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent-ink);
  }
  .warn {
    color: var(--danger);
    margin: 4px 0 8px;
  }
  progress {
    width: 100%;
    margin: 4px 0;
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
