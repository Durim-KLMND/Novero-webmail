<script lang="ts">
  // The to-do view on the rail: a task board, and your own notes. Both are
  // kept on this computer only.
  import { onDestroy, onMount, tick } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, errorText, type Note } from "../api";
  import { openMenu, type MenuItem } from "../shell/menus.svelte";
  import { toast } from "../shell/toast.svelte";
  import { editedText, matchNotes, notePreview, noteTitle } from "./todo";
  import Icon from "../ui/Icon.svelte";
  import TaskBoard from "./TaskBoard.svelte";

  interface Props {
    /** Called with the number of unfinished tasks due today or earlier. */
    onDue: (count: number) => void;
  }
  let { onDue }: Props = $props();

  type Section = "board" | "notes";
  const SECTION_KEY = "novero.todo";
  let section = $state<Section>(readSection());
  let board = $state<ReturnType<typeof TaskBoard>>();

  let notes = $state<Note[] | null>(null);
  let notesError = $state("");
  let query = $state("");
  /** The note open in the editor: a copy, saved a moment after typing stops. */
  let open = $state<Note | null>(null);
  let titleInput = $state<HTMLInputElement>();
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let unsaved = false;

  const shown = $derived(matchNotes(notes ?? [], query));

  function readSection(): Section {
    try {
      return localStorage.getItem(SECTION_KEY) === "notes" ? "notes" : "board";
    } catch {
      return "board";
    }
  }

  function show(s: Section) {
    section = s;
    try {
      localStorage.setItem(SECTION_KEY, s);
    } catch {
      // Remembering the section is only a convenience.
    }
  }

  async function loadNotes() {
    try {
      notes = await api.listNotes();
      notesError = "";
      if (!open && notes.length) open = { ...notes[0] };
    } catch (err) {
      notesError = errorText(err);
    }
  }

  onMount(loadNotes);
  onDestroy(() => {
    if (unsaved) flush();
  });

  /** Saves the open note now, if it has changed. */
  async function flush() {
    clearTimeout(saveTimer);
    if (!open || !unsaved) return;
    unsaved = false;
    const note = $state.snapshot(open);
    try {
      const saved = await api.saveNote(note);
      if (open?.id === saved.id) open.updated = saved.updated;
      notes = [saved, ...(notes ?? []).filter((n) => n.id !== saved.id)].sort(byPin);
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  /** Pinned first; within each, the order the list already has. */
  function byPin(a: Note, b: Note): number {
    return Number(b.pinned) - Number(a.pinned);
  }

  function edited() {
    unsaved = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, 600);
  }

  /** Opens a note; an empty one left behind is deleted. */
  async function pick(note: Note) {
    if (open?.id === note.id) return;
    await flush();
    const left = open;
    open = { ...note };
    if (left && !left.title.trim() && !left.body.trim()) {
      notes = (notes ?? []).filter((n) => n.id !== left.id);
      api.deleteNote(left.id).catch(() => {});
    }
  }

  export async function newNote() {
    show("notes");
    query = "";
    await flush();
    if (open && !open.title.trim() && !open.body.trim()) {
      await tick();
      titleInput?.focus();
      return;
    }
    try {
      const note = await api.saveNote({ id: 0, title: "", body: "", pinned: false, updated: 0 });
      notes = [note, ...(notes ?? [])].sort(byPin);
      open = { ...note };
      await tick();
      titleInput?.focus();
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  async function togglePin(note: Note) {
    // The open note is saved as typed, with the pin.
    const base = $state.snapshot(open?.id === note.id ? open : note);
    if (open?.id === note.id) {
      clearTimeout(saveTimer);
      unsaved = false;
    }
    try {
      const saved = await api.saveNote({ ...base, pinned: !base.pinned });
      if (open?.id === saved.id) Object.assign(open, { pinned: saved.pinned, updated: saved.updated });
      // Pinned notes go to the top; the rest stay newest first.
      notes = (notes ?? [])
        .map((n) => (n.id === saved.id ? saved : n))
        .sort((a, b) => byPin(a, b) || b.updated - a.updated || b.id - a.id);
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  async function removeNote(note: Note) {
    const named = noteTitle(note);
    const empty = !note.title.trim() && !note.body.trim();
    if (!empty) {
      const sure = await ask(`Delete “${named}”? This can't be undone.`, {
        title: "Delete note",
        kind: "warning",
        okLabel: "Delete",
        cancelLabel: "Keep it",
      });
      if (!sure) return;
    }
    if (open?.id === note.id) {
      clearTimeout(saveTimer);
      unsaved = false;
    }
    try {
      await api.deleteNote(note.id);
      const i = (notes ?? []).findIndex((n) => n.id === note.id);
      notes = (notes ?? []).filter((n) => n.id !== note.id);
      if (open?.id === note.id) {
        const next = notes[Math.min(i, notes.length - 1)];
        open = next ? { ...next } : null;
      }
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  function noteItems(note: Note): MenuItem[] {
    return [
      { label: note.pinned ? "Unpin" : "Pin to the top", icon: note.pinned ? "unpin" : "pin", run: () => togglePin(note) },
      "separator",
      { label: "Delete", icon: "trash", danger: true, run: () => removeNote(note) },
    ];
  }

  /** What the list shows for a note: the open one as it is being typed. */
  const live = (note: Note) => (open?.id === note.id ? open : note);

  function titleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      (document.querySelector(".note-body") as HTMLTextAreaElement | null)?.focus();
    }
  }
</script>

<div class="todo">
  <aside class="side">
    {#if section === "board"}
      <button class="primary new" onclick={() => board?.newTask()}><Icon name="plus" /> New task</button>
    {:else}
      <button class="primary new" onclick={newNote}><Icon name="plus" /> New note</button>
    {/if}
    <nav aria-label="To-do">
      <button class="place" class:current={section === "board"} aria-current={section === "board" ? "page" : undefined} onclick={() => show("board")}>
        <Icon name="board" /><span>Task board</span>
      </button>
      <button class="place" class:current={section === "notes"} aria-current={section === "notes" ? "page" : undefined} onclick={() => show("notes")}>
        <Icon name="note" /><span>Notes</span>
        {#if notes?.length}<span class="count">{notes.length}</span>{/if}
      </button>
    </nav>
    <p class="local quiet">Kept on this computer.</p>
  </aside>

  <section class="main" class:hidden={section !== "board"}>
    <header>
      <h1>Task board</h1>
      <span class="grow"></span>
      <span class="quiet hint">Drag cards between columns. Right-click for more.</span>
    </header>
    <TaskBoard bind:this={board} {onDue} />
  </section>

  <section class="main notes" class:hidden={section !== "notes"}>
    <div class="list">
      <div class="search">
        <Icon name="search" size={15} />
        <input type="search" placeholder="Search notes" aria-label="Search notes" bind:value={query} />
      </div>
      {#if notesError}<p class="error">{notesError}</p>{/if}
      <ul>
        {#each shown as note (note.id)}
          {@const n = live(note)}
          <li>
            <button
              class="note-row"
              class:current={open?.id === note.id}
              onclick={() => pick(note)}
              oncontextmenu={(e) => openMenu(e, noteItems(n))}
            >
              <span class="row-top">
                <span class="row-title">{noteTitle(n)}</span>
                {#if n.pinned}<span class="pin" title="Pinned"><Icon name="pin" size={12} /></span>{/if}
              </span>
              <span class="row-sub quiet">
                <span class="when">{editedText(n.updated)}</span>
                <span class="preview">{notePreview(n)}</span>
              </span>
            </button>
          </li>
        {:else}
          {#if notes}
            <li class="empty quiet">
              {#if query}
                No notes have “{query}” in them.
              {:else}
                No notes yet. <button class="link" onclick={newNote}>Write one</button>
              {/if}
            </li>
          {/if}
        {/each}
      </ul>
    </div>

    <div class="editor">
      {#if open}
        <div class="bar">
          <span class="quiet">Edited {editedText(open.updated)}</span>
          <span class="grow"></span>
          <button
            class="icon-only"
            class:on={open.pinned}
            aria-pressed={open.pinned}
            aria-label={open.pinned ? "Unpin" : "Pin to the top"}
            title={open.pinned ? "Unpin" : "Pin to the top"}
            onclick={() => open && togglePin(open)}
          >
            <Icon name="pin" size={16} filled={open.pinned} />
          </button>
          <button class="icon-only" aria-label="Delete note" title="Delete note" onclick={() => open && removeNote(open)}>
            <Icon name="trash" size={16} />
          </button>
        </div>
        <input
          class="note-title"
          placeholder="Title"
          aria-label="Title"
          bind:value={open.title}
          bind:this={titleInput}
          oninput={edited}
          onkeydown={titleKeydown}
          onblur={flush}
        />
        <textarea class="note-body" placeholder="Write anything…" aria-label="Note" bind:value={open.body} oninput={edited} onblur={flush}></textarea>
      {:else if notes}
        <div class="blank quiet">
          <Icon name="note" size={36} />
          <p>Notes you write here stay on this computer.</p>
          <button class="primary" onclick={newNote}><Icon name="plus" /> New note</button>
        </div>
      {/if}
    </div>
  </section>
</div>

<style>
  .todo {
    display: grid;
    grid-template-columns: 232px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    height: 100%;
    min-width: 0;
  }
  .side {
    background: var(--bg-side);
    border-right: 1px solid var(--border);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    overflow-y: auto;
  }
  .new {
    justify-content: center;
  }
  nav {
    display: grid;
    gap: 2px;
  }
  .place {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 10px;
    border: none;
    background: none;
    padding: 7px 10px;
    border-radius: var(--radius);
    color: var(--text);
    text-align: left;
  }
  .place span:first-of-type {
    flex: 1;
  }
  .place.current {
    background: var(--bg-selected);
    color: var(--accent-ink);
    font-weight: 600;
  }
  .count {
    font-size: 12px;
    color: var(--text-quiet);
    font-weight: 400;
  }
  .local {
    margin: auto 0 0;
    font-size: 12px;
  }
  .hidden {
    display: none !important;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 16px;
    min-height: 53px;
    border-bottom: 1px solid var(--border);
  }
  h1 {
    font-size: 18px;
    margin: 0;
    font-weight: 600;
  }
  .grow {
    flex: 1;
  }
  .hint {
    font-size: 12px;
  }
  .icon-only {
    border: none;
    background: none;
    padding: 5px;
    color: var(--text-quiet);
  }
  .icon-only:hover:not(:disabled) {
    color: var(--text);
  }
  .icon-only.on {
    color: var(--accent-ink);
  }

  /* Notes */
  .notes {
    display: grid;
    grid-template-columns: 280px minmax(0, 1fr);
  }
  .list {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--border);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 10px 12px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-quiet);
  }
  .search:focus-within {
    outline: 2px solid var(--accent-ink);
    outline-offset: 1px;
  }
  .search input {
    border: none;
    padding: 7px 0;
    background: none;
    outline: none;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0 6px 8px;
    overflow-y: auto;
    flex: 1;
  }
  .note-row {
    display: grid;
    gap: 2px;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    padding: 9px 10px;
    border-radius: var(--radius);
    color: var(--text);
  }
  .note-row.current {
    background: var(--bg-selected);
  }
  .row-top {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .row-title {
    flex: 1;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pin {
    display: inline-flex;
    color: var(--accent-ink);
  }
  .row-sub {
    display: flex;
    gap: 8px;
    font-size: 12px;
    min-width: 0;
  }
  .when {
    flex: none;
  }
  .preview {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .empty {
    padding: 16px 10px;
    font-size: 13px;
  }
  .editor {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 10px 28px 20px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    min-height: 32px;
  }
  .note-title {
    border: none;
    padding: 8px 0;
    font-size: 22px;
    font-weight: 600;
    background: none;
    outline: none;
  }
  .note-body {
    flex: 1;
    font: inherit;
    font-size: 15px;
    line-height: 1.6;
    color: var(--text);
    background: none;
    border: none;
    outline: none;
    resize: none;
    padding: 4px 0;
  }
  .blank {
    margin: auto;
    display: grid;
    justify-items: center;
    gap: 10px;
    text-align: center;
  }
  .blank p {
    margin: 0;
  }
</style>
