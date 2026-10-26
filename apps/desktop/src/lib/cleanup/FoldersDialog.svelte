<script lang="ts">
  // Choosing one of your own folders for each category
  // (mail/category_folders.rs). Nothing moves until a folder is picked.
  import { onMount, untrack } from "svelte";
  import { api, errorText, type Account, type CategoryFolder, type Folder } from "../api";
  import { folderPath } from "../format";
  import { toast } from "../shell/toast.svelte";
  import Icon from "../ui/Icon.svelte";
  import { categoryOf, filedCategories } from "./categories";

  interface Props {
    accounts: Account[];
    chosen: CategoryFolder[];
    onChange: (chosen: CategoryFolder[]) => void;
    onClose: () => void;
  }
  let { accounts, chosen, onChange, onClose }: Props = $props();

  let account = $state(untrack(() => accounts[0]?.id ?? 0));
  let folders = $state<Folder[]>([]);
  let filing = $state(false);
  let closeButton = $state<HTMLButtonElement>();

  /** Your own folders and Archive: where mail can be filed. */
  const choices = $derived(folders.filter((f) => f.role === "other" || f.role === "archive"));

  $effect(() => {
    const id = account;
    api
      .listFolders(id)
      .then((list) => {
        if (id === account) folders = list;
      })
      .catch((err) => toast(errorText(err), "error"));
  });

  onMount(() => closeButton?.focus());

  function current(category: string): string {
    const found = chosen.find((c) => c.account === account && c.category === category);
    return found ? String(found.folder) : "";
  }

  async function pick(category: CategoryFolder["category"], value: string) {
    try {
      onChange(await api.setCategoryFolder(account, category, value ? Number(value) : null));
    } catch (err) {
      toast(errorText(err), "error");
    }
  }

  async function fileNow() {
    filing = true;
    try {
      const filed = await api.fileIntoCategoryFolders();
      toast(filed ? `${filed} ${filed === 1 ? "message" : "messages"} moved to your folders.` : "Nothing to move right now.");
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      filing = false;
    }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  }
</script>

<div class="backdrop" role="presentation" onkeydown={keydown}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="folders-title" aria-describedby="folders-about">
    <h2 id="folders-title">Move categories to your folders</h2>
    <p id="folders-about" class="quiet">
      Pick a folder for any category you want out of the Inbox. Mail you've sorted, and mail Clean up is sure about,
      moves there as it arrives. Categories left on "Keep in Inbox" stay where they are. Spam always goes to Junk.
    </p>
    {#if accounts.length > 1}
      <select aria-label="Account" bind:value={account}>
        {#each accounts as a (a.id)}
          <option value={a.id}>{a.email}</option>
        {/each}
      </select>
    {/if}
    <ul class="rows">
      {#each filedCategories as id (id)}
        <li class="cat-{id}">
          <span class="name"><span class="dot"><Icon name={categoryOf[id].icon} size={14} /></span>{categoryOf[id].label}</span>
          <select
            aria-label="Folder for {categoryOf[id].label}"
            value={current(id)}
            onchange={(e) => pick(id, e.currentTarget.value)}
          >
            <option value="">Keep in Inbox</option>
            {#each choices as f (f.id)}
              <option value={String(f.id)}>{f.role === "archive" ? "Archive" : folderPath(f.name, f.delimiter)}</option>
            {/each}
          </select>
        </li>
      {/each}
    </ul>
    {#if !choices.some((f) => f.role === "other")}
      <p class="quiet hint">This account has no folders of its own yet. Make one from the folder list in Mail, then pick it here.</p>
    {/if}
    <div class="buttons">
      <button onclick={fileNow} disabled={filing || !chosen.length}>{filing ? "Moving…" : "Move Inbox mail now"}</button>
      <button class="primary" bind:this={closeButton} onclick={onClose}>Done</button>
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
    max-height: calc(100vh - 64px);
    overflow: auto;
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
    display: grid;
    gap: 12px;
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  p {
    margin: 0;
    font-size: 13px;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .rows li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 8px 6px 12px;
  }
  .rows li + li {
    border-top: 1px solid var(--border);
  }
  .name {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .dot {
    display: inline-flex;
    color: var(--cat);
  }
  .rows select {
    width: 200px;
    max-width: 55%;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
