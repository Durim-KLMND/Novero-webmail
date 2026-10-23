<script lang="ts">
  // The narrow strip on the far left: switches between mail, the calendar and
  // the to-do list, with settings and the shortcut list at the bottom.
  import Icon, { type IconName } from "../ui/Icon.svelte";

  export type AppView = "mail" | "calendar" | "todo";

  interface Props {
    view: AppView;
    /** Unread mail in all inboxes, shown on the mail button. */
    unread?: number;
    /** Unfinished tasks due today or earlier, shown on the to-do button. */
    due?: number;
    onView: (view: AppView) => void;
    onSettings: () => void;
    onHelp: () => void;
  }
  let { view, unread = 0, due = 0, onView, onSettings, onHelp }: Props = $props();

  const views: { id: AppView; icon: IconName; label: string; keys: string }[] = [
    { id: "mail", icon: "mail", label: "Mail", keys: "Ctrl+1" },
    { id: "calendar", icon: "calendar", label: "Calendar", keys: "Ctrl+2" },
    { id: "todo", icon: "todo", label: "To-do and notes", keys: "Ctrl+3" },
  ];
</script>

<nav class="rail" aria-label="Switch between mail, calendar and to-do">
  {#each views as v (v.id)}
    <button
      class="spot"
      class:current={view === v.id}
      aria-label={v.label}
      aria-current={view === v.id ? "page" : undefined}
      title="{v.label} ({v.keys})"
      onclick={() => onView(v.id)}
    >
      <Icon name={v.icon} size={20} />
      {#if v.id === "mail" && unread > 0}
        <span class="badge" aria-label="{unread} unread">{unread > 99 ? "99+" : unread}</span>
      {:else if v.id === "todo" && due > 0}
        <span class="badge" aria-label="{due} due">{due > 99 ? "99+" : due}</span>
      {/if}
    </button>
  {/each}
  <span class="grow"></span>
  <button class="spot" aria-label="Keyboard shortcuts" title="Keyboard shortcuts (?)" onclick={onHelp}>
    <Icon name="keyboard" size={20} />
  </button>
  <button class="spot" aria-label="Settings" title="Settings and accounts" onclick={onSettings}>
    <Icon name="settings" size={20} />
  </button>
</nav>

<style>
  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 10px 0;
    width: 52px;
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    height: 100%;
  }
  .spot {
    position: relative;
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--text-quiet);
  }
  .spot:hover {
    background: var(--bg-hover);
    color: var(--text);
  }
  .spot.current {
    background: var(--bg-selected);
    color: var(--accent-ink);
  }
  /* A bar on the left edge marks the open view. */
  .spot.current::before {
    content: "";
    position: absolute;
    left: -7px;
    top: 9px;
    bottom: 9px;
    width: 3px;
    border-radius: 2px;
    background: var(--accent-ink);
  }
  .grow {
    flex: 1;
  }
  .badge {
    position: absolute;
    top: 2px;
    right: 0;
    min-width: 16px;
    padding: 0 4px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 10px;
    line-height: 16px;
    font-weight: 600;
    text-align: center;
  }
</style>
