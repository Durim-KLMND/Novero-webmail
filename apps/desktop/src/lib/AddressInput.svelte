<script lang="ts">
  import { api, type Contact } from "./api";
  import { openMenu } from "./menus.svelte";

  interface Props {
    id: string;
    value: string;
    placeholder?: string;
    onInput: () => void;
  }
  let { id, value = $bindable(), placeholder = "", onInput }: Props = $props();

  /** Suggestions are looked up after this pause in typing. */
  const LOOKUP_AFTER = 120;

  let input = $state<HTMLInputElement>();
  let suggestions = $state<Contact[]>([]);
  let active = $state(0);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let lookup = 0;
  /** Where the address being typed starts and ends in `value`. */
  let token = { start: 0, end: 0 };

  export function focus() {
    input?.focus();
  }

  /** The address the caret is in: from the last comma or semicolon before it
   *  (outside quotes and angle brackets) to the next one. */
  function current(text: string, caret: number): { start: number; end: number } {
    let start = 0;
    let quoted = false;
    let angle = false;
    for (let i = 0; i < caret; i++) {
      const c = text[i];
      if (c === '"' && !angle) quoted = !quoted;
      else if (c === "<" && !quoted) angle = true;
      else if (c === ">" && !quoted) angle = false;
      else if ((c === "," || c === ";") && !quoted && !angle) start = i + 1;
    }
    let end = text.slice(caret).search(/[,;]/);
    end = end < 0 ? text.length : caret + end;
    return { start, end };
  }

  /** Addresses already in the field, so they aren't offered again. */
  function present(): Set<string> {
    const found = value.match(/[^\s<>,;"]+@[^\s<>,;"]+/g) ?? [];
    return new Set(found.map((a) => a.toLowerCase()));
  }

  function typed() {
    onInput();
    clearTimeout(timer);
    const caret = input?.selectionStart ?? value.length;
    token = current(value, caret);
    const query = value.slice(token.start, caret).trim();
    // Only while typing at the end of an address that isn't complete yet.
    if (!query || caret < token.end || query.includes("<")) {
      close();
      return;
    }
    const mine = ++lookup;
    timer = setTimeout(async () => {
      const found = await api.suggestContacts(query).catch(() => []);
      if (mine !== lookup) return;
      const have = present();
      suggestions = found.filter((c) => !have.has(c.email.toLowerCase()) || c.email.toLowerCase() === query.toLowerCase());
      active = 0;
    }, LOOKUP_AFTER);
  }

  function close() {
    lookup++;
    clearTimeout(timer);
    suggestions = [];
  }

  /** How an address is written in the field (as `Mailbox::display` in Rust). */
  function display(c: Contact): string {
    if (!c.name) return c.email;
    if (/[,;<>"@]/.test(c.name)) return `"${c.name.replace(/"/g, "'")}" <${c.email}>`;
    return `${c.name} <${c.email}>`;
  }

  function choose(c: Contact) {
    const before = value.slice(0, token.start);
    const after = value.slice(token.end).replace(/^\s*[,;]?\s*/, "");
    const lead = before && !before.endsWith(" ") ? " " : "";
    const inserted = `${lead}${display(c)}, `;
    value = before + inserted + after;
    const caret = before.length + inserted.length;
    close();
    onInput();
    queueMicrotask(() => input?.setSelectionRange(caret, caret));
  }

  function keydown(event: KeyboardEvent) {
    if (!suggestions.length) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const step = event.key === "ArrowDown" ? 1 : -1;
      active = (active + step + suggestions.length) % suggestions.length;
    } else if (event.key === "Enter" || event.key === "Tab") {
      if (event.ctrlKey || event.metaKey) return;
      choose(suggestions[active]);
    } else if (event.key === "Escape") {
      close();
      event.stopPropagation();
    } else {
      return;
    }
    event.preventDefault();
  }

  async function forget(c: Contact) {
    await api.hideContact(c.email).catch(() => {});
    suggestions = suggestions.filter((s) => s.email !== c.email);
    active = Math.min(active, Math.max(0, suggestions.length - 1));
    input?.focus();
  }
</script>

<div class="address">
  <input
    {id}
    bind:this={input}
    bind:value
    oninput={typed}
    onkeydown={keydown}
    onblur={close}
    {placeholder}
    autocomplete="off"
    spellcheck="false"
    role="combobox"
    aria-autocomplete="list"
    aria-expanded={suggestions.length > 0}
    aria-controls="{id}-suggestions"
    aria-activedescendant={suggestions.length ? `${id}-suggestion-${active}` : undefined}
  />
  {#if suggestions.length}
    <ul class="suggestions" id="{id}-suggestions" role="listbox">
      {#each suggestions as c, i (c.email)}
        <li
          id="{id}-suggestion-{i}"
          role="option"
          aria-selected={i === active}
          class:active={i === active}
          onmousedown={(e) => {
            e.preventDefault();
            if (e.button === 0) choose(c);
          }}
          onmouseenter={() => (active = i)}
          oncontextmenu={(e) =>
            openMenu(e, [{ label: "Remove from suggestions", icon: "close", run: () => forget(c) }])}
        >
          {#if c.name}<span class="name">{c.name}</span>{/if}
          <span class="email">{c.email}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .address {
    position: relative;
    flex: 1;
    min-width: 0;
  }
  .suggestions {
    position: absolute;
    left: 0;
    right: 0;
    top: calc(100% + 2px);
    z-index: 20;
    list-style: none;
    margin: 0;
    padding: 4px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18);
    max-height: 280px;
    overflow-y: auto;
  }
  li {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 5px;
    cursor: pointer;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
  }
  li.active {
    background: var(--bg-hover);
  }
  .name {
    font-weight: 600;
  }
  .email {
    color: var(--text-quiet);
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
