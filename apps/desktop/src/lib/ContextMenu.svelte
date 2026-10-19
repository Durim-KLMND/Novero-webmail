<script lang="ts">
  import { tick } from "svelte";
  import Icon from "./Icon.svelte";
  import { closeMenu, fallbackMenu, menu, type MenuAction, type MenuItem } from "./menus.svelte";

  let root = $state<HTMLDivElement>();
  let sub = $state<HTMLDivElement>();
  let pos = $state({ x: 0, y: 0 });
  /** The open submenu: the item it belongs to and where it is drawn. */
  let open = $state<{ item: MenuAction; x: number; y: number } | null>(null);

  const EDGE = 4;

  /** Keeps a menu inside the window, flipping it left or up when needed. */
  function fit(el: HTMLElement, x: number, y: number, flipFrom?: DOMRect): { x: number; y: number } {
    const { width, height } = el.getBoundingClientRect();
    if (x + width > innerWidth - EDGE) x = flipFrom ? flipFrom.left - width : innerWidth - width - EDGE;
    if (y + height > innerHeight - EDGE) y = Math.max(EDGE, innerHeight - height - EDGE);
    return { x: Math.max(EDGE, x), y: Math.max(EDGE, y) };
  }

  $effect(() => {
    if (!menu.items) return;
    const { x, y } = menu as { x: number; y: number };
    open = null;
    pos = { x, y };
    tick().then(() => {
      if (!root) return;
      pos = fit(root, x, y);
      first(root)?.focus();
    });
  });

  function buttons(el: HTMLElement | undefined): HTMLButtonElement[] {
    return Array.from(el?.querySelectorAll<HTMLButtonElement>(":scope > button:not(:disabled)") ?? []);
  }

  function first(el: HTMLElement | undefined) {
    return buttons(el)[0];
  }

  function choose(item: MenuAction) {
    if (item.disabled) return;
    if (item.items) return;
    closeMenu();
    item.run?.();
  }

  async function showSub(item: MenuAction, button: HTMLElement, focus = false) {
    if (open?.item === item) {
      if (focus) first(sub)?.focus();
      return;
    }
    const at = button.getBoundingClientRect();
    open = { item, x: at.right - 2, y: at.top - 5 };
    await tick();
    if (!sub) return;
    const fitted = fit(sub, open.x, open.y, at);
    open = { ...open, ...fitted };
    if (focus) first(sub)?.focus();
  }

  function hover(item: MenuItem, event: PointerEvent) {
    if (item === "separator") return;
    const button = event.currentTarget as HTMLElement;
    button.focus();
    if (item.items) showSub(item, button);
    else if (button.parentElement === root) open = null;
  }

  function keydown(event: KeyboardEvent) {
    const el = event.target as HTMLElement;
    const list = el.parentElement as HTMLElement;
    const all = buttons(list);
    const at = all.indexOf(el as HTMLButtonElement);
    const inSub = list === sub;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const step = event.key === "ArrowDown" ? 1 : -1;
      all[(at + step + all.length) % all.length]?.focus();
    } else if (event.key === "Home") {
      all[0]?.focus();
    } else if (event.key === "End") {
      all[all.length - 1]?.focus();
    } else if (event.key === "ArrowRight" && !inSub) {
      el.click();
    } else if ((event.key === "ArrowLeft" || event.key === "Escape") && inSub) {
      const owner = Array.from(root?.children ?? []).find((b) => b.getAttribute("aria-expanded") === "true");
      open = null;
      (owner as HTMLElement | undefined)?.focus();
    } else if (event.key === "Escape" || event.key === "Tab") {
      closeMenu();
    } else {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
  }

  function outside(event: PointerEvent) {
    if (!menu.items) return;
    const target = event.target as Node;
    if (root?.contains(target) || sub?.contains(target)) return;
    closeMenu();
  }

  /** Right-clicks nobody else handled: the text menu, or no menu at all. */
  function contextmenu(event: MouseEvent) {
    fallbackMenu(event);
  }
</script>

<svelte:window
  oncontextmenu={contextmenu}
  onpointerdowncapture={outside}
  onblur={closeMenu}
  onresize={closeMenu}
  onwheelcapture={(e) => {
    if (!root?.contains(e.target as Node) && !sub?.contains(e.target as Node)) closeMenu();
  }}
/>

{#snippet entries(list: MenuItem[], el: "root" | "sub")}
  {#each list as item, i (i)}
    {#if item === "separator"}
      <div class="separator" role="separator"></div>
    {:else}
      <button
        role="menuitem"
        class:danger={item.danger}
        disabled={item.disabled}
        aria-haspopup={item.items ? "menu" : undefined}
        aria-expanded={item.items ? open?.item === item : undefined}
        onpointerenter={(e) => hover(item, e)}
        onclick={(e) => (item.items ? showSub(item, e.currentTarget, true) : choose(item))}
        tabindex="-1"
      >
        <span class="icon-slot">{#if item.icon}<Icon name={item.icon} />{/if}</span>
        <span class="label">{item.label}</span>
        {#if item.shortcut}<span class="shortcut">{item.shortcut}</span>{/if}
        {#if item.items}<span class="shortcut"><Icon name="chevron-right" size={14} /></span>{/if}
      </button>
    {/if}
  {/each}
  {#if el === "sub" && list.length === 0}<div class="none quiet">Nothing here</div>{/if}
{/snippet}

{#if menu.items}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={root}
    style:left="{pos.x}px"
    style:top="{pos.y}px"
    onkeydown={keydown}
    oncontextmenu={(e) => e.preventDefault()}
  >
    {@render entries(menu.items, "root")}
  </div>
  {#if open?.item.items}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="menu sub"
      role="menu"
      tabindex="-1"
      bind:this={sub}
      style:left="{open.x}px"
      style:top="{open.y}px"
      onkeydown={keydown}
      oncontextmenu={(e) => e.preventDefault()}
    >
      {@render entries(open.item.items, "sub")}
    </div>
  {/if}
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 100;
    min-width: 200px;
    max-width: 320px;
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    padding: 4px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18);
    font-size: 13px;
    user-select: none;
  }
  .menu:focus {
    outline: none;
  }
  button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    border: none;
    background: none;
    border-radius: 5px;
    padding: 6px 10px 6px 8px;
    text-align: left;
    color: var(--text);
  }
  button:focus-visible,
  button:focus,
  button[aria-expanded="true"] {
    background: var(--bg-hover);
    outline: none;
  }
  button:disabled {
    opacity: 0.45;
  }
  .icon-slot {
    display: flex;
    width: 16px;
    color: var(--text-quiet);
  }
  .label {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .shortcut {
    display: flex;
    color: var(--text-quiet);
    font-size: 12px;
  }
  .danger,
  .danger .icon-slot {
    color: var(--danger);
  }
  .separator {
    height: 1px;
    margin: 4px 6px;
    background: var(--border);
  }
  .none {
    padding: 6px 10px;
  }
</style>
