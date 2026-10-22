<script lang="ts">
  import { linkStatus } from "./hoverlink.svelte";

  interface Props {
    /** The pane whose bottom corner the bubble sits in. */
    area: HTMLElement | undefined;
  }
  let { area }: Props = $props();

  let width = $state(0);
  let height = $state(0);
  // Measured on every pointer move over a link, so it follows a resized pane.
  const box = $derived(linkStatus.url && area ? area.getBoundingClientRect() : null);
  // Like a browser, the bubble moves to the other corner when the pointer is on it.
  const right = $derived(!!box && linkStatus.x < box.left + width + 24 && linkStatus.y > box.bottom - height - 24);
</script>

{#if box}
  <div
    class="status"
    class:right
    role="status"
    style:left={right ? "auto" : `${box.left}px`}
    style:right={right ? `${window.innerWidth - box.right}px` : "auto"}
    style:bottom="{window.innerHeight - box.bottom}px"
    style:max-width="{Math.round(box.width * 0.8)}px"
    bind:offsetWidth={width}
    bind:offsetHeight={height}
  >
    {linkStatus.url}
  </div>
{/if}

<style>
  /* Always light, whatever the theme, so it reads like a browser's status bar. */
  .status {
    position: fixed;
    z-index: 50;
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
    border-left: 1px solid #c9c9c4;
    border-right: 0;
    border-radius: 6px 0 0 0;
  }
</style>
