<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, errorText, type Account, type Draft, type OutboxItem } from "./api";
  import { listDate } from "./format";
  import { laterText } from "./later";

  /** A scheduled message whose time hasn't come. */
  const waiting = (item: OutboxItem) => item.state === "queued" && item.sendAt != null && item.sendAt * 1000 > Date.now();

  interface Props {
    account: Account;
    /** Bumped when the outbox may have changed. */
    version: number;
    onEdit: (draft: Draft) => void;
    onClose: () => void;
  }
  let { account, version, onEdit, onClose }: Props = $props();

  let items = $state<OutboxItem[]>([]);
  let error = $state("");

  async function load() {
    try {
      items = await api.listOutbox(account.id);
      error = "";
    } catch (err) {
      error = errorText(err);
    }
  }

  $effect(() => {
    void version;
    void account.id;
    load();
  });

  async function run(action: () => Promise<unknown>) {
    try {
      await action();
    } catch (err) {
      error = errorText(err);
    }
    await load();
  }

  async function remove(item: OutboxItem) {
    const confirmed = await ask(`Delete “${item.subject || "(no subject)"}” without sending it?`, {
      title: "Delete message",
      kind: "warning",
      okLabel: "Delete",
      cancelLabel: "Keep",
    });
    if (confirmed) run(() => api.deleteOutbox(item.id));
  }
</script>

<section class="outbox" aria-label="Outbox">
  <div class="actions">
    <h1>Outbox <span class="quiet">{account.email}</span></h1>
    <button onclick={onClose}>Close</button>
  </div>
  {#if error}<p class="error banner" role="alert">{error}</p>{/if}
  {#if items.length === 0}
    <p class="empty quiet">Everything has been sent.</p>
  {:else}
    <ul>
      {#each items as item (item.id)}
        <li>
          <div class="line">
            <strong>{item.subject || "(no subject)"}</strong>
            <span class="quiet">{listDate(item.createdAt)}</span>
          </div>
          <div class="quiet to">To {item.recipients.join(", ")}</div>
          {#if waiting(item)}
            <p class="scheduled">Scheduled to send {laterText(new Date(item.sendAt! * 1000))}.</p>
          {:else if item.state === "failed"}
            <p class="error">The server refused this message: {item.lastError}</p>
          {:else if item.lastError}
            <p class="quiet">Waiting to send. Last try: {errorText(item.lastError)}</p>
          {:else}
            <p class="quiet">Sending…</p>
          {/if}
          <div class="buttons">
            {#if item.state === "failed"}
              <button onclick={() => run(() => api.retryOutbox(item.id))}>Try again</button>
            {/if}
            {#if waiting(item)}
              <button class="primary" onclick={() => run(() => api.sendOutboxNow(item.id))}>Send now</button>
            {/if}
            <button onclick={() => run(async () => onEdit(await api.editOutbox(item.id)))}>Edit</button>
            <button onclick={() => remove(item)}>Delete</button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .outbox {
    overflow-y: auto;
    min-width: 0;
  }
  .actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 24px;
    border-bottom: 1px solid var(--border);
  }
  .scheduled {
    color: var(--accent-ink);
  }
  .actions button {
    width: auto;
    padding: 5px 12px;
    font-size: 13px;
  }
  h1 {
    font-size: 16px;
    margin: 0;
  }
  h1 .quiet {
    font-weight: 400;
    font-size: 13px;
    margin-left: 6px;
  }
  .banner {
    margin: 10px 24px 0;
  }
  .empty {
    padding: 24px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    padding: 14px 24px;
    border-bottom: 1px solid var(--border);
  }
  .line {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
  .to {
    font-size: 13px;
    word-break: break-word;
  }
  li p {
    margin: 8px 0;
    font-size: 13px;
  }
  .buttons {
    display: flex;
    gap: 6px;
  }
  .buttons button {
    width: auto;
    padding: 4px 10px;
    font-size: 13px;
  }
</style>
