<script lang="ts">
  import { onDestroy } from "svelte";
  import { api, errorText, onMailEvent, type Account, type MailEvent } from "./api";
  import AccountForm from "./AccountForm.svelte";

  interface Props {
    /** First launch: start with the welcome step. Otherwise go straight to the form. */
    firstRun: boolean;
    onFinish: () => void;
    onCancel?: () => void;
  }
  let { firstRun, onFinish, onCancel }: Props = $props();

  type Step = "welcome" | "account" | "done";
  // Only the first value matters: the step changes as the person moves on.
  // svelte-ignore state_referenced_locally
  let step = $state<Step>(firstRun ? "welcome" : "account");
  let added = $state<Account[]>([]);
  let progress = $state<Record<number, { folder: string; done: number; total: number; finished: boolean; error: string | null }>>({});
  let formKey = $state(0);

  const unlisten = onMailEvent((e: MailEvent) => {
    if (!added.some((a) => a.id === e.account)) return;
    const current = progress[e.account] ?? { folder: "", done: 0, total: 0, finished: false, error: null };
    if (e.type === "syncProgress") {
      progress[e.account] = { ...current, folder: e.folder, done: e.done, total: e.total };
    } else if (e.type === "syncFinished") {
      progress[e.account] = { ...current, finished: true, error: e.error };
    }
  });
  onDestroy(() => {
    unlisten.then((fn) => fn());
  });

  function accountAdded(account: Account) {
    added = [...added, account];
    step = "done";
  }

  function addAnother() {
    formKey += 1;
    step = "account";
  }

  function statusLine(id: number): string {
    const p = progress[id];
    if (!p) return "Starting to download your message list…";
    if (p.error) return `Sync stopped: ${errorText(p.error)}`;
    if (p.finished) return "Message list downloaded.";
    if (p.total === 0) return `Checking ${p.folder}…`;
    return `Downloading ${p.folder}: ${p.done.toLocaleString()} of ${p.total.toLocaleString()}`;
  }
</script>

<div class="page">
  <div class="card">
    {#if step === "welcome"}
      <div class="logo" aria-hidden="true">✉</div>
      <h1>Welcome to Novero Mail</h1>
      <p class="lead">A fast, private email app for Windows. Let's connect your first account.</p>
      <ul class="points">
        <li><strong>All your accounts in one place.</strong> Add as many IMAP accounts as you like.</li>
        <li><strong>Private by default.</strong> Remote images and trackers stay blocked until you allow them.</li>
        <li><strong>Your password stays on this PC,</strong> in Windows Credential Manager.</li>
      </ul>
      <div class="actions">
        <button class="primary" onclick={() => (step = "account")}>Add an email account</button>
      </div>
      <p class="step quiet">Step 1 of 3</p>
    {:else if step === "account"}
      {#key formKey}
        <AccountForm
          onAdded={accountAdded}
          onCancel={added.length > 0 ? () => (step = "done") : onCancel}
          stepLabel={firstRun ? "Step 2 of 3" : ""}
        />
      {/key}
    {:else}
      <div class="logo ok" aria-hidden="true">✓</div>
      <h1>{added.length === 1 ? "You're all set" : `${added.length} accounts connected`}</h1>
      <ul class="accounts">
        {#each added as account (account.id)}
          <li>
            <strong>{account.email}</strong>
            <span class="quiet">
              {#if !progress[account.id]?.finished}<span class="spinner small"></span>{/if}
              {statusLine(account.id)}
            </span>
          </li>
        {/each}
      </ul>
      <p class="quiet">You can start reading while the rest downloads.</p>
      <div class="actions">
        <button onclick={addAnother}>Add another account</button>
        <button class="primary" onclick={onFinish}>Open Novero Mail</button>
      </div>
      {#if firstRun}<p class="step quiet">Step 3 of 3</p>{/if}
    {/if}
  </div>
</div>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    display: grid;
    place-items: center;
    padding: 32px 16px;
    background: var(--bg-side);
  }
  .card {
    width: min(520px, 100%);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 32px;
  }
  .logo {
    width: 48px;
    height: 48px;
    border-radius: 12px;
    background: var(--accent);
    color: var(--accent-text);
    display: grid;
    place-items: center;
    font-size: 24px;
    margin-bottom: 16px;
  }
  .logo.ok {
    background: var(--success);
  }
  h1 {
    font-size: 22px;
    margin: 0 0 8px;
  }
  .lead {
    color: var(--text-quiet);
    margin: 0 0 20px;
  }
  .points {
    padding-left: 18px;
    margin: 0 0 24px;
    display: grid;
    gap: 10px;
  }
  .accounts {
    list-style: none;
    padding: 0;
    margin: 16px 0;
    display: grid;
    gap: 12px;
  }
  .accounts li {
    display: grid;
    gap: 2px;
  }
  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    flex-wrap: wrap;
  }
  .step {
    text-align: center;
    margin: 20px 0 0;
    font-size: 12px;
  }
  .spinner.small {
    width: 12px;
    height: 12px;
    margin-right: 4px;
  }
</style>
