<script lang="ts">
  interface Props {
    onClose: () => void;
  }
  let { onClose }: Props = $props();

  const general: [string, string][] = [
    ["Ctrl+1", "Mail"],
    ["Ctrl+2", "Calendar"],
    ["Ctrl+K", "Search mail"],
    ["?", "Show this list"],
  ];
  const mail: [string, string][] = [
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
    ["B", "Snooze, or unsnooze"],
    ["Shift+U", "Mark unread"],
    ["Shift+I", "Mark read"],
    ["/", "Search"],
    ["Ctrl+Enter", "Send, in the composer"],
    ["Esc", "Close or clear the search"],
  ];
  const calendar: [string, string][] = [
    ["C", "New event"],
    ["T", "Today"],
    ["J / →", "Next day, week or month"],
    ["K / ←", "Previous day, week or month"],
    ["D / W / M", "Day, week or month view"],
    ["Ctrl+Enter", "Save, in an event"],
  ];
  const sections: [string, [string, string][]][] = [
    ["General", general],
    ["Mail", mail],
    ["Calendar", calendar],
  ];
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onClose}>
  <div class="dialog" role="dialog" tabindex="-1" aria-modal="true" aria-labelledby="keys-title" onclick={(e) => e.stopPropagation()}>
    <h2 id="keys-title">Keyboard shortcuts</h2>
    <div class="sections">
      {#each sections as [name, list] (name)}
        <section>
          <h3>{name}</h3>
          <dl>
            {#each list as [keys, action] (keys)}
              <dt><kbd>{keys}</kbd></dt>
              <dd>{action}</dd>
            {/each}
          </dl>
        </section>
      {/each}
    </div>
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
    width: min(760px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  h2 {
    margin: 0 0 12px;
    font-size: 18px;
  }
  .sections {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 0 24px;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 13px;
    color: var(--text-quiet);
    text-transform: uppercase;
    letter-spacing: 0.04em;
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
