<script lang="ts">
  // One task opened from the board: its name, notes, due date and column.
  import { onMount, untrack } from "svelte";
  import type { Task, TaskColumn } from "./api";
  import { dayKey } from "./todo";
  import Icon from "./Icon.svelte";

  interface Props {
    task: Task;
    columns: TaskColumn[];
    onSave: (task: Task) => void;
    onDelete: (task: Task) => void;
    onCancel: () => void;
  }
  let { task, columns, onSave, onDelete, onCancel }: Props = $props();

  let draft = $state<Task>(untrack(() => ({ ...task })));
  let titleInput = $state<HTMLInputElement>();

  function save() {
    if (!draft.title.trim()) return;
    onSave({ ...draft, title: draft.title.trim(), due: draft.due || null });
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    } else if (e.key === "Enter" && (e.ctrlKey || (e.target as HTMLElement).tagName === "INPUT")) {
      e.preventDefault();
      save();
    }
  }

  function dueIn(days: number) {
    const d = new Date();
    d.setDate(d.getDate() + days);
    draft.due = dayKey(d);
  }

  onMount(() => titleInput?.focus());
</script>

<div class="backdrop" role="presentation" onkeydown={keydown}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Task">
    <div class="top">
      <button
        class="check"
        class:on={draft.done}
        aria-pressed={draft.done}
        title={draft.done ? "Mark as not done" : "Mark as done"}
        onclick={() => (draft.done = !draft.done)}
      >
        <Icon name={draft.done ? "task-done" : "task-open"} size={22} />
      </button>
      <input class="title" aria-label="Task" placeholder="Task" bind:value={draft.title} bind:this={titleInput} />
    </div>

    <label>
      <span class="quiet">Notes</span>
      <textarea rows="5" placeholder="Details, links, anything else" bind:value={draft.notes}></textarea>
    </label>

    <div class="row">
      <label class="grow">
        <span class="quiet">Due</span>
        <input type="date" bind:value={draft.due} />
      </label>
      <label class="grow">
        <span class="quiet">Column</span>
        <select bind:value={draft.column}>
          {#each columns as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
        </select>
      </label>
    </div>
    <div class="quick">
      <button class="link" onclick={() => dueIn(0)}>Today</button>
      <button class="link" onclick={() => dueIn(1)}>Tomorrow</button>
      <button class="link" onclick={() => dueIn(7)}>In a week</button>
      {#if draft.due}<button class="link" onclick={() => (draft.due = null)}>No due date</button>{/if}
    </div>

    <div class="buttons">
      {#if task.id}
        <button class="danger" onclick={() => onDelete(task)}><Icon name="trash" size={15} />Delete</button>
      {/if}
      <span class="grow"></span>
      <button onclick={onCancel}>Cancel</button>
      <button class="primary" disabled={!draft.title.trim()} onclick={save}>{task.id ? "Save" : "Add task"}</button>
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
    width: min(460px, calc(100vw - 32px));
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
    display: grid;
    gap: 14px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .title {
    font-size: 16px;
    font-weight: 600;
  }
  .check {
    border: none;
    background: none;
    padding: 2px;
    color: var(--text-quiet);
    border-radius: 50%;
  }
  .check:hover:not(:disabled) {
    background: none;
    color: var(--accent-ink);
  }
  .check.on {
    color: var(--success);
  }
  label {
    display: grid;
    gap: 4px;
    font-size: 13px;
  }
  textarea {
    font: inherit;
    font-size: 14px;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    resize: vertical;
  }
  textarea:focus {
    outline: 2px solid var(--accent-ink);
    outline-offset: 1px;
  }
  .row {
    display: flex;
    gap: 12px;
  }
  .grow {
    flex: 1;
  }
  .quick {
    display: flex;
    gap: 14px;
    margin-top: -6px;
    font-size: 13px;
  }
  .buttons {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .danger {
    color: var(--danger);
  }
</style>
