<script lang="ts">
  // Novero Mail's own title bar, shown instead of the Windows one when
  // "Use the Novero Mail title bar" is on. Dragging it moves the window (and
  // snaps it to screen edges), double-clicking maximises, and the buttons on
  // the right do what the Windows ones do.
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";
  import { titlebar } from "./frame.svelte";

  interface Props {
    title: string;
  }
  let { title }: Props = $props();

  const win = getCurrentWindow();
  let maximized = $state(false);
  let focused = $state(true);

  // Leaves room for the bar above the rest of the window (app.css).
  $effect(() => {
    document.documentElement.classList.toggle("titlebar", titlebar.on);
  });

  onMount(() => {
    const check = () =>
      win
        .isMaximized()
        .then((m) => (maximized = m))
        .catch(() => {});
    check();
    const stops = [win.onResized(check), win.onFocusChanged(({ payload }) => (focused = payload))];
    return () => stops.forEach((stop) => stop.then((unlisten) => unlisten()));
  });
</script>

{#if titlebar.on}
  <header class="bar" class:blurred={!focused} data-tauri-drag-region>
    <span class="logo" data-tauri-drag-region><Icon name="mail" size={12} /></span>
    <span class="title" data-tauri-drag-region>{title}</span>
    <div class="controls">
      <button tabindex="-1" aria-label="Minimise" title="Minimise" onclick={() => win.minimize()}>
        <Icon name="minimize" size={15} />
      </button>
      <button
        tabindex="-1"
        aria-label={maximized ? "Restore" : "Maximise"}
        title={maximized ? "Restore down" : "Maximise"}
        onclick={() => win.toggleMaximize()}
      >
        <Icon name={maximized ? "restore" : "maximize"} size={maximized ? 13 : 12} />
      </button>
      <button class="close" tabindex="-1" aria-label="Close" title="Close" onclick={() => win.close()}>
        <Icon name="close" size={16} />
      </button>
    </div>
  </header>
{/if}

<style>
  .bar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: var(--titlebar-height);
    display: flex;
    align-items: center;
    background: var(--bg-side);
    border-bottom: 1px solid var(--border);
    color: var(--text);
    font-size: 12px;
    user-select: none;
    /* Above dialogs, so the window can still be moved and closed; below menus. */
    z-index: 50;
  }
  .logo {
    /* Centred over the icon rail below it. */
    width: 52px;
    flex-shrink: 0;
    display: grid;
    place-items: center;
  }
  .logo::before {
    content: "";
    grid-area: 1 / 1;
    width: 20px;
    height: 20px;
    border-radius: 5px;
    background: var(--accent);
  }
  .logo :global(.icon) {
    grid-area: 1 / 1;
    color: var(--accent-text);
  }
  .title {
    flex: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .blurred .title,
  .blurred .controls button {
    color: var(--text-quiet);
  }
  .controls {
    display: flex;
    height: 100%;
  }
  .controls button {
    width: 46px;
    height: 100%;
    padding: 0;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--text);
  }
  .controls button:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .controls .close:hover:not(:disabled) {
    /* The Windows close red, so it reads as Close at a glance. */
    background: #c42b1c;
    color: #ffffff;
  }
</style>
