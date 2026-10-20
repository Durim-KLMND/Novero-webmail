<script lang="ts">
  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();

  const shortcuts: [string, string][] = [
    ["J / ↓", "Next conversation"],
    ["K / ↑", "Previous conversation"],
    ["C", "New message"],
    ["Shift+C", "New message in a new window"],
    ["R", "Reply"],
    ["A", "Reply all"],
    ["F", "Forward"],
    ["E", "Archive"],
    ["# / Delete", "Delete"],
    ["S", "Flag or unflag"],
    ["Shift+U", "Mark unread"],
    ["Shift+I", "Mark read"],
    ["Ctrl+K or /", "Search"],
    ["Ctrl+Enter", "Send, in the composer"],
    ["Esc", "Close or clear the search"],
    ["?", "Show this list"],
  ];
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onClose}>
  <div class="dialog" role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="keys-title" onclick={(e) => e.stopPropagation()}>
    <h2 id="keys-title">Keyboard shortcuts</h2>
    <dl>
      {#each shortcuts as [keys, action] (keys)}
        <dt><kbd>{keys}</kbd></dt>
        <dd>{action}</dd>
      {/each}
    </dl>
    <div class="buttons"><button onclick={onClose}>Close</button></div>
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
    width: min(420px, calc(100vw - 32px));
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  h2 {
    margin: 0 0 12px;
    font-size: 18px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 16px;
    margin: 0 0 16px;
  }
  dt,
  dd {
    margin: 0;
  }
  kbd {
    font: 12px/1.6 var(--font);
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 4px;
    padding: 0 6px;
    background: var(--bg-side);
    white-space: nowrap;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
  }
</style>
