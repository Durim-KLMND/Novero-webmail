<script lang="ts">
  // The warning before category folders are made on the mail server
  // (mail/category_folders.rs): what will be added, and where.
  import { onMount } from "svelte";
  import type { Account } from "../api";
  import Icon from "../ui/Icon.svelte";
  import { categoryOf, filedCategories } from "./categories";

  interface Props {
    /** The accounts that get folders. */
    accounts: Account[];
    busy: boolean;
    onMake: () => void;
    onCancel: () => void;
  }
  let { accounts, busy, onMake, onCancel }: Props = $props();

  let cancelButton = $state<HTMLButtonElement>();
  onMount(() => cancelButton?.focus());

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !busy) {
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    }
  }
</script>

<div class="backdrop" role="presentation" onkeydown={keydown}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="folders-title" aria-describedby="folders-about">
    <h2 id="folders-title"><Icon name="alert" /> Add folders to your mailbox?</h2>
    <p id="folders-about">
      Novero Mail will add a <strong>Categories</strong> folder, with these folders inside, to the mailbox on
      {accounts.length === 1 ? "this account's" : "each of these accounts'"} mail server. You'll see them in webmail
      and on your phone too.
    </p>
    <ul class="folders">
      {#each filedCategories as id (id)}
        <li class="cat-{id}"><span class="dot"><Icon name={categoryOf[id].icon} size={14} /></span>{categoryOf[id].label}</li>
      {/each}
    </ul>
    <ul class="accounts">
      {#each accounts as a (a.id)}
        <li>{a.email}</li>
      {/each}
    </ul>
    <p class="quiet">
      Inbox mail you've sorted, and mail Clean up is sure about, moves into them now and as it arrives. Personal mail
      stays in the Inbox and Spam goes to Junk. Folders that already exist are reused.
    </p>
    <div class="buttons">
      <button bind:this={cancelButton} onclick={onCancel} disabled={busy}>Cancel</button>
      <button class="primary" onclick={onMake} disabled={busy}>{busy ? "Adding folders…" : "Add folders"}</button>
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
    width: min(440px, calc(100vw - 32px));
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
    display: flex;
    align-items: center;
    gap: 8px;
  }
  h2 :global(svg) {
    color: var(--danger);
  }
  p {
    margin: 0;
  }
  .quiet {
    font-size: 13px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .folders {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .folders li {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 13px;
    color: var(--cat);
    background: color-mix(in srgb, var(--cat) 12%, transparent);
    border-radius: 6px;
    padding: 2px 8px;
  }
  .dot {
    display: inline-flex;
  }
  .accounts {
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .accounts li {
    padding: 6px 12px;
    font-size: 13px;
  }
  .accounts li + li {
    border-top: 1px solid var(--border);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
