<script lang="ts">
  import { linkStatus } from "./hoverlink.svelte";

  let width = $state(0);
  let height = $state(0);
  // Like a browser, the bubble moves to the other corner when the pointer is on it.
  const right = $derived(linkStatus.x < width + 24 && linkStatus.y > window.innerHeight - height - 24);
</script>

{#if linkStatus.url}
  <div class="status" class:right role="status" bind:offsetWidth={width} bind:offsetHeight={height}>{linkStatus.url}</div>
{/if}

<style>
  /* Always light, whatever the theme, so it reads like a browser's status bar. */
  .status {
    position: fixed;
    left: 0;
    bottom: 0;
    z-index: 10000;
    max-width: min(60vw, 720px);
    padding: 3px 8px;
    background: #f6f6f4;
    color: #1f1f1e;
    border: 1px solid #c9c9c4;
    border-left: 0;
    border-bottom: 0;
    border-radius: 0 6px 0 0;
    box-shadow: 0 0 6px rgb(0 0 0 / 0.12);
    font: 12px/1.4 var(--font, "Segoe UI", system-ui, sans-serif);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
  }
  .right {
    left: auto;
    right: 0;
    border-left: 1px solid #c9c9c4;
    border-right: 0;
    border-radius: 6px 0 0 0;
  }
</style>
