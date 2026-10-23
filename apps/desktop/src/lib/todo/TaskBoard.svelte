<script lang="ts">
  // The task board: columns of cards, left to right. Cards are dragged with
  // pointer events (HTML drag and drop does not work in the Windows webview
  // while Tauri takes file drops), within a column or onto another one.
  import { onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, errorText, type Board, type Task, type TaskColumn } from "../api";
  import { openMenu, toggleMenuUnder, type MenuItem } from "../shell/menus.svelte";
  import { toast } from "../shell/toast.svelte";
  import { dayKey, dueCount, dueState, dueText } from "./todo";
  import Icon from "../ui/Icon.svelte";
  import TaskDialog from "./TaskDialog.svelte";

  interface Props {
    /** Called with the number of unfinished tasks due today or earlier. */
    onDue: (count: number) => void;
  }
  let { onDue }: Props = $props();

  let board = $state<Board | null>(null);
  let error = $state("");
  /** The card open in the dialog. */
  let editing = $state<Task | null>(null);
  /** The column a task is being typed into, at its bottom. */
  let adding = $state<number | null>(null);
  let newTitle = $state("");
  let addInput = $state<HTMLTextAreaElement>();
  /** The column being renamed in place, or "new" for a column being added. */
  let naming = $state<number | "new" | null>(null);
  let columnName = $state("");
  let nameInput = $state<HTMLInputElement>();
  let now = $state(new Date());

  /** The card pressed, before it has moved far enough to be dragged. */
  let press: { task: Task; x: number; y: number; dx: number; dy: number; width: number } | null = null;
  let carrying = $state<{ task: Task; x: number; y: number; dx: number; dy: number; width: number } | null>(null);
  /** Where the carried card would land. */
  let drop = $state<{ column: number; index: number } | null>(null);
  /** A drag ended on this card: the click that follows is not a click. */
  let dragged = false;
  const columnEls: Record<number, HTMLElement> = {};

  const cards = (column: number) => board?.tasks.filter((t) => t.column === column) ?? [];

  async function load() {
    try {
      board = await api.taskBoard();
      error = "";
    } catch (err) {
      error = errorText(err);
    }
  }

  $effect(() => {
    if (board) onDue(dueCount(board, dayKey(now)));
  });

  onMount(() => {
    load();
    // Due days are counted from today, so look again after midnight.
    const timer = setInterval(() => (now = new Date()), 60_000);
    return () => clearInterval(timer);
  });

  function failed(err: unknown) {
    toast(errorText(err), "error");
    load();
  }

  function replace(task: Task) {
    if (!board) return;
    const i = board.tasks.findIndex((t) => t.id === task.id);
    if (i >= 0) board.tasks[i] = task;
    else board.tasks.push(task);
  }

  async function save(task: Task) {
    try {
      replace(await api.saveTask(task));
    } catch (err) {
      failed(err);
    }
  }

  async function toggleDone(task: Task) {
    await save({ ...task, done: !task.done });
  }

  async function setDue(task: Task, days: number | null) {
    let due: string | null = null;
    if (days != null) {
      const d = new Date();
      d.setDate(d.getDate() + days);
      due = dayKey(d);
    }
    await save({ ...task, due });
  }

  async function removeTask(task: Task) {
    if (!board) return;
    board.tasks = board.tasks.filter((t) => t.id !== task.id);
    try {
      await api.deleteTask(task.id);
    } catch (err) {
      failed(err);
    }
  }

  /** Puts the card in `column` at `index` here, then in the database. */
  async function moveTo(task: Task, column: number, index: number) {
    if (!board) return;
    const order: Task[] = [];
    for (const c of board.columns) {
      const list = board.tasks.filter((t) => t.column === c.id && t.id !== task.id);
      if (c.id === column) list.splice(Math.min(index, list.length), 0, { ...task, column });
      order.push(...list);
    }
    board.tasks = order;
    try {
      await api.moveTask(task.id, column, index);
    } catch (err) {
      failed(err);
    }
  }

  /** Opens a new task in the first column. */
  export function newTask() {
    const column = board?.columns[0];
    if (column) editing = { id: 0, column: column.id, title: "", notes: "", due: null, done: false };
  }

  async function startAdding(column: number) {
    adding = column;
    newTitle = "";
    await tick();
    addInput?.focus();
  }

  async function addTask() {
    const title = newTitle.trim();
    if (!title || adding == null) return;
    const column = adding;
    newTitle = "";
    try {
      replace(await api.saveTask({ id: 0, column, title, notes: "", due: null, done: false }));
      await tick();
      addInput?.scrollIntoView({ block: "nearest" });
    } catch (err) {
      failed(err);
    }
  }

  function addKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      addTask();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      adding = null;
    }
  }

  async function addBlur() {
    if (newTitle.trim()) await addTask();
    adding = null;
  }

  async function startNaming(column: TaskColumn | "new") {
    naming = column === "new" ? "new" : column.id;
    columnName = column === "new" ? "" : column.name;
    await tick();
    nameInput?.select();
  }

  async function saveName() {
    if (naming == null || !board) return;
    const id = naming === "new" ? 0 : naming;
    const name = columnName.trim();
    naming = null;
    if (!name) return;
    try {
      const saved = await api.saveTaskColumn({ id, name });
      const i = board.columns.findIndex((c) => c.id === saved.id);
      if (i >= 0) board.columns[i] = saved;
      else board.columns.push(saved);
    } catch (err) {
      failed(err);
    }
  }

  function nameKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      saveName();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      naming = null;
    }
  }

  async function moveColumn(column: TaskColumn, by: number) {
    if (!board) return;
    const from = board.columns.findIndex((c) => c.id === column.id);
    const to = from + by;
    if (to < 0 || to >= board.columns.length) return;
    const columns = [...board.columns];
    columns.splice(from, 1);
    columns.splice(to, 0, column);
    board.columns = columns;
    // Cards follow their columns' order in the list too.
    board.tasks = columns.flatMap((c) => board!.tasks.filter((t) => t.column === c.id));
    try {
      await api.moveTaskColumn(column.id, to);
    } catch (err) {
      failed(err);
    }
  }

  async function deleteColumn(column: TaskColumn) {
    if (!board) return;
    const n = cards(column.id).length;
    if (n > 0) {
      const sure = await ask(
        `“${column.name}” has ${n === 1 ? "a task" : `${n} tasks`} in it, which will be deleted too.`,
        { title: "Delete column", kind: "warning", okLabel: "Delete", cancelLabel: "Keep it" },
      );
      if (!sure) return;
    }
    try {
      await api.deleteTaskColumn(column.id);
      board.columns = board.columns.filter((c) => c.id !== column.id);
      board.tasks = board.tasks.filter((t) => t.column !== column.id);
    } catch (err) {
      failed(err);
    }
  }

  async function clearDone(column: TaskColumn) {
    if (!board) return;
    board.tasks = board.tasks.filter((t) => t.column !== column.id || !t.done);
    try {
      await api.clearDoneTasks(column.id);
    } catch (err) {
      failed(err);
    }
  }

  function columnItems(column: TaskColumn): MenuItem[] {
    const i = board?.columns.findIndex((c) => c.id === column.id) ?? 0;
    const last = (board?.columns.length ?? 1) - 1;
    return [
      { label: "Add a task", icon: "plus", run: () => startAdding(column.id) },
      { label: "Rename", icon: "edit", run: () => startNaming(column) },
      { label: "Move left", icon: "chevron-left", disabled: i === 0, run: () => moveColumn(column, -1) },
      { label: "Move right", icon: "chevron-right", disabled: i === last, run: () => moveColumn(column, 1) },
      "separator",
      {
        label: "Clear finished tasks",
        icon: "task-done",
        disabled: !cards(column.id).some((t) => t.done),
        run: () => clearDone(column),
      },
      { label: "Delete column", icon: "trash", danger: true, disabled: last === 0, run: () => deleteColumn(column) },
    ];
  }

  function taskItems(task: Task): MenuItem[] {
    return [
      { label: "Open", icon: "edit", run: () => (editing = task) },
      {
        label: task.done ? "Mark as not done" : "Mark as done",
        icon: task.done ? "task-open" : "task-done",
        run: () => toggleDone(task),
      },
      {
        label: "Due",
        icon: "due",
        items: [
          { label: "Today", run: () => setDue(task, 0) },
          { label: "Tomorrow", run: () => setDue(task, 1) },
          { label: "In a week", run: () => setDue(task, 7) },
          task.due && "separator",
          task.due && { label: "No due date", run: () => setDue(task, null) },
        ].filter(Boolean) as MenuItem[],
      },
      {
        label: "Move to",
        icon: "move",
        items: (board?.columns ?? []).map((c) => ({
          label: c.name,
          disabled: c.id === task.column,
          run: () => moveTo(task, c.id, cards(c.id).length),
        })),
      },
      "separator",
      { label: "Delete", icon: "trash", danger: true, run: () => removeTask(task) },
    ];
  }

  // Dragging cards.

  function pointerDown(e: PointerEvent, task: Task) {
    if (e.button !== 0 || (e.target as Element).closest("button, input, textarea")) return;
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    press = { task, x: e.clientX, y: e.clientY, dx: e.clientX - box.left, dy: e.clientY - box.top, width: box.width };
    window.addEventListener("pointermove", pointerMove);
    window.addEventListener("pointerup", pointerUp);
    window.addEventListener("pointercancel", pointerCancel);
  }

  function pointerMove(e: PointerEvent) {
    if (!press) return;
    if (!carrying) {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return;
      carrying = { ...press };
    }
    carrying.x = e.clientX;
    carrying.y = e.clientY;
    drop = landing(e.clientX, e.clientY);
  }

  /** The column under the pointer, and where among its cards. */
  function landing(x: number, y: number): { column: number; index: number } | null {
    if (!carrying) return null;
    for (const [id, el] of Object.entries(columnEls)) {
      if (!el) continue;
      const box = el.getBoundingClientRect();
      if (x < box.left || x > box.right) continue;
      let index = 0;
      for (const card of el.querySelectorAll<HTMLElement>("[data-task]")) {
        if (Number(card.dataset.task) === carrying.task.id) continue;
        const r = card.getBoundingClientRect();
        if (y > r.top + r.height / 2) index++;
      }
      return { column: Number(id), index };
    }
    return null;
  }

  function pointerUp() {
    const done = carrying;
    const target = drop;
    pointerCancel();
    if (!done) return;
    dragged = true;
    setTimeout(() => (dragged = false));
    if (target) moveTo(done.task, target.column, target.index);
  }

  function pointerCancel() {
    press = null;
    carrying = null;
    drop = null;
    window.removeEventListener("pointermove", pointerMove);
    window.removeEventListener("pointerup", pointerUp);
    window.removeEventListener("pointercancel", pointerCancel);
  }

  /** The card the drop line goes above in a column, or "end". */
  function lineBefore(column: number): number | "end" | null {
    if (!carrying || drop?.column !== column) return null;
    const others = cards(column).filter((t) => t.id !== carrying!.task.id);
    return others[drop.index]?.id ?? "end";
  }

  function cardKeydown(e: KeyboardEvent, task: Task) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      editing = task;
    } else if (e.key === "Delete") {
      e.preventDefault();
      removeTask(task);
    }
  }
</script>

<div class="board" class:carrying={carrying != null}>
  {#if error}<p class="error">{error}</p>{/if}
  {#if board}
    {#each board.columns as column (column.id)}
      {@const list = cards(column.id)}
      {@const line = lineBefore(column.id)}
      <section
        class="column"
        class:target={drop?.column === column.id}
        bind:this={columnEls[column.id]}
        aria-label={column.name}
      >
        <header oncontextmenu={(e) => openMenu(e, columnItems(column))} role="group">
          {#if naming === column.id}
            <input
              class="rename"
              aria-label="Column name"
              bind:value={columnName}
              bind:this={nameInput}
              onkeydown={nameKeydown}
              onblur={saveName}
            />
          {:else}
            <h2 ondblclick={() => startNaming(column)}>{column.name}</h2>
            <span class="count quiet">{list.length}</span>
            <span class="grow"></span>
            <button class="icon-only" aria-label="Add a task to {column.name}" title="Add a task" onclick={() => startAdding(column.id)}>
              <Icon name="plus" size={16} />
            </button>
            <button
              class="icon-only"
              aria-label="More for {column.name}"
              title="More"
              onclick={(e) => toggleMenuUnder(e.currentTarget, columnItems(column), true)}
            >
              <Icon name="more" size={16} />
            </button>
          {/if}
        </header>
        <div class="cards">
          {#each list as task (task.id)}
            {@const due = dueState(task, now)}
            {#if line === task.id}<div class="line" aria-hidden="true"></div>{/if}
            <div
              class="card"
              class:done={task.done}
              class:lifted={carrying?.task.id === task.id}
              data-task={task.id}
              role="button"
              tabindex="0"
              aria-label="{task.title}{task.done ? ', done' : ''}{task.due ? `, due ${dueText(task.due, now)}` : ''}"
              onpointerdown={(e) => pointerDown(e, task)}
              onclick={() => !dragged && (editing = task)}
              onkeydown={(e) => cardKeydown(e, task)}
              oncontextmenu={(e) => openMenu(e, taskItems(task))}
            >
              <button
                class="check"
                class:on={task.done}
                aria-label={task.done ? "Mark as not done" : "Mark as done"}
                title={task.done ? "Mark as not done" : "Mark as done"}
                onclick={(e) => (e.stopPropagation(), toggleDone(task))}
              >
                <Icon name={task.done ? "task-done" : "task-open"} size={18} />
              </button>
              <div class="body">
                <span class="title">{task.title}</span>
                {#if task.due || task.notes}
                  <span class="meta">
                    {#if task.due}
                      <span class="due {task.done ? '' : due}"><Icon name="due" size={13} />{dueText(task.due, now)}</span>
                    {/if}
                    {#if task.notes}<span class="has-notes" title="Has notes"><Icon name="note" size={13} /></span>{/if}
                  </span>
                {/if}
              </div>
            </div>
          {/each}
          {#if line === "end"}<div class="line" aria-hidden="true"></div>{/if}
          {#if adding === column.id}
            <textarea
              class="new-task"
              rows="2"
              placeholder="What needs doing? Enter to add"
              aria-label="New task in {column.name}"
              bind:value={newTitle}
              bind:this={addInput}
              onkeydown={addKeydown}
              onblur={addBlur}
            ></textarea>
          {:else}
            <button class="add" onclick={() => startAdding(column.id)}><Icon name="plus" size={15} />Add a task</button>
          {/if}
        </div>
      </section>
    {/each}
    <div class="column ghost">
      {#if naming === "new"}
        <input
          class="rename"
          placeholder="Column name"
          aria-label="New column name"
          bind:value={columnName}
          bind:this={nameInput}
          onkeydown={nameKeydown}
          onblur={saveName}
        />
      {:else}
        <button class="add" onclick={() => startNaming("new")}><Icon name="plus" size={15} />Add a column</button>
      {/if}
    </div>
  {:else if !error}
    <span class="spinner"></span>
  {/if}
</div>

{#if carrying}
  <div
    class="card ghost-card"
    style:left="{carrying.x - carrying.dx}px"
    style:top="{carrying.y - carrying.dy}px"
    style:width="{carrying.width}px"
    aria-hidden="true"
  >
    <span class="check"><Icon name={carrying.task.done ? "task-done" : "task-open"} size={18} /></span>
    <div class="body"><span class="title">{carrying.task.title}</span></div>
  </div>
{/if}

{#if editing && board}
  <TaskDialog
    task={editing}
    columns={board.columns}
    onSave={async (task) => {
      const before = editing;
      editing = null;
      if (before && before.id !== 0 && task.column !== before.column) {
        await moveTo(before, task.column, cards(task.column).length);
      }
      await save(task);
    }}
    onDelete={(task) => {
      editing = null;
      if (task.id) removeTask(task);
    }}
    onCancel={() => (editing = null)}
  />
{/if}

<style>
  .board {
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 16px;
    overflow: auto;
  }
  .board.carrying {
    cursor: grabbing;
    user-select: none;
  }
  .column {
    flex: none;
    width: 272px;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-side);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
  .column.target {
    border-color: color-mix(in srgb, var(--accent-ink) 55%, var(--border));
  }
  .column.ghost {
    background: none;
    border-style: dashed;
    padding: 6px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 6px 4px 12px;
    min-height: 40px;
  }
  h2 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 12px;
    margin-left: 4px;
  }
  .grow {
    flex: 1;
  }
  .icon-only {
    border: none;
    background: none;
    padding: 4px;
    color: var(--text-quiet);
  }
  .icon-only:hover {
    color: var(--text);
  }
  .rename {
    padding: 4px 8px;
  }
  .cards {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 4px 8px 8px;
    overflow-y: auto;
    min-height: 0;
  }
  .card {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    padding: 8px 10px 8px 6px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    cursor: pointer;
    user-select: none;
    touch-action: none;
  }
  .card:hover {
    border-color: color-mix(in srgb, var(--text-quiet) 45%, var(--border));
  }
  .card:focus-visible {
    outline: 2px solid var(--accent-ink);
    outline-offset: 1px;
  }
  .card.lifted {
    opacity: 0.35;
  }
  .ghost-card {
    position: fixed;
    z-index: 30;
    pointer-events: none;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
    transform: rotate(1.5deg);
  }
  .check {
    display: inline-grid;
    place-items: center;
    flex: none;
    border: none;
    background: none;
    padding: 0;
    margin-top: 0;
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
  .body {
    min-width: 0;
    display: grid;
    gap: 4px;
  }
  .title {
    overflow-wrap: anywhere;
    line-height: 1.35;
    padding-top: 1px;
  }
  .card.done .title {
    color: var(--text-quiet);
    text-decoration: line-through;
  }
  .meta {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 12px;
    color: var(--text-quiet);
  }
  .due {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .due.overdue {
    color: var(--danger);
    font-weight: 600;
  }
  .due.today {
    color: var(--accent-ink);
    font-weight: 600;
  }
  .has-notes {
    display: inline-flex;
  }
  /* Where a dragged card would land. Takes no room, so nothing jumps. */
  .line {
    position: relative;
    height: 0;
    margin: -3px 0;
  }
  .line::before {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: -1px;
    height: 3px;
    border-radius: 2px;
    background: var(--accent-ink);
  }
  .add {
    border: none;
    background: none;
    color: var(--text-quiet);
    justify-content: flex-start;
    padding: 6px 8px;
    width: 100%;
  }
  .add:hover:not(:disabled) {
    color: var(--text);
  }
  .new-task {
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--accent-ink);
    border-radius: var(--radius);
    padding: 8px 10px;
    resize: none;
    outline: none;
  }
  .error {
    margin: 0;
  }
</style>
