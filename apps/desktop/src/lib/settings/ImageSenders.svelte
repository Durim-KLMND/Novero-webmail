<script lang="ts">
  // Addresses and domains whose remote images always load (mail/images.rs).
  // Changes are saved right away, like "Always load" in a message.
  import { onMount } from "svelte";
  import { api, errorText } from "../api";
  import Icon from "../ui/Icon.svelte";

  let senders = $state<string[]>([]);
  let adding = $state("");
  let error = $state("");

  onMount(async () => {
    try {
      senders = await api.listImageSenders();
    } catch (err) {
      error = errorText(err);
    }
  });

  async function add(event: SubmitEvent) {
    event.preventDefault();
    if (!adding.trim()) return;
    try {
      senders = await api.allowImagesFrom(adding);
      adding = "";
      error = "";
    } catch (err) {
      error = errorText(err);
    }
  }

  async function remove(sender: string) {
    try {
      senders = await api.forgetImageSender(sender);
      error = "";
    } catch (err) {
      error = errorText(err);
    }
  }
</script>

<fieldset>
  <legend>Remote images</legend>
  <small class="quiet">
    Images from these senders always load. A domain, such as example.com, covers everyone who writes from it.
  </small>
  {#if senders.length}
    <ul class="senders">
      {#each senders as sender (sender)}
        <li>
          <span class="selectable">{sender}</span>
          <button class="icon-only danger" title="Remove {sender}" aria-label="Remove {sender}" onclick={() => remove(sender)}>
            <Icon name="close" size={14} />
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="quiet empty">No senders yet. Use "Always load" above a message's blocked images to add one.</p>
  {/if}
  <form class="add" onsubmit={add}>
    <input
      type="text"
      placeholder="jane@example.com or example.com"
      aria-label="Address or domain"
      bind:value={adding}
      spellcheck="false"
    />
    <button type="submit" disabled={!adding.trim()}><Icon name="plus" size={14} /> Add</button>
  </form>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</fieldset>

<style>
  fieldset {
    border: none;
    padding: 0;
    margin: 0 0 18px;
  }
  legend {
    font-weight: 600;
    margin-bottom: 8px;
  }
  small {
    display: block;
  }
  .senders {
    list-style: none;
    margin: 10px 0 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    max-height: 200px;
    overflow: auto;
  }
  .senders li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 4px 4px 12px;
  }
  .senders li + li {
    border-top: 1px solid var(--border);
  }
  .empty {
    margin: 10px 0 0;
    font-size: 13px;
  }
  .add {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }
  .add input {
    flex: 1;
    min-width: 0;
  }
  .add button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  .danger:hover:not(:disabled) {
    color: var(--danger);
  }
  .error {
    color: var(--danger);
    margin: 6px 0 0;
    font-size: 13px;
  }
</style>
