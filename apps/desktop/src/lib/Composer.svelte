<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, errorText, type Account, type Draft, type DraftFields } from "./api";
  import { fileSize } from "./format";

  interface Props {
    draft: Draft;
    accounts: Account[];
    /** The composer is done: sent, saved or thrown away. */
    onClose: (outcome: "sent" | "saved" | "discarded") => void;
    /** Another account was picked as the sender; the draft moved to it. */
    onReplace: (draft: Draft) => void;
  }
  let { draft, accounts, onClose, onReplace }: Props = $props();

  /** Typing is saved locally after this pause, and copied to the server this often. */
  const SAVE_AFTER = 1200;
  const UPLOAD_EVERY = 30_000;

  // The composer is created per draft (MailView keys it by id), so it starts
  // from the draft once and then owns the fields.
  const start = untrack(() => $state.snapshot(draft));
  let fields = $state<DraftFields>({
    to: start.to,
    cc: start.cc,
    bcc: start.bcc,
    subject: start.subject,
    body: start.body,
  });
  let attachments = $state(start.attachments);
  let showCopies = $state(Boolean(start.cc || start.bcc));
  let status = $state("");
  let error = $state("");
  let busy = $state(false);
  let dirty = false;
  let unuploaded = false;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let uploadTimer: ReturnType<typeof setInterval> | undefined;
  let toInput = $state<HTMLInputElement>();
  let bodyInput = $state<HTMLTextAreaElement>();
  let closed = false;

  const account = $derived(accounts.find((a) => a.id === draft.account));

  onMount(async () => {
    await tick();
    if (draft.inReplyTo && fields.to) {
      // A reply: start typing above the quoted text.
      bodyInput?.focus();
      bodyInput?.setSelectionRange(0, 0);
      if (bodyInput) bodyInput.scrollTop = 0;
    } else {
      toInput?.focus();
    }
    uploadTimer = setInterval(() => {
      if (unuploaded && !dirty) save(true);
    }, UPLOAD_EVERY);
  });

  onDestroy(() => {
    clearTimeout(saveTimer);
    clearInterval(uploadTimer);
  });

  function edited() {
    dirty = true;
    unuploaded = true;
    status = "";
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => save(false), SAVE_AFTER);
  }

  async function save(upload: boolean) {
    clearTimeout(saveTimer);
    if (closed) return;
    const wasDirty = dirty;
    dirty = false;
    try {
      await api.saveDraft(draft.id, $state.snapshot(fields), upload);
      if (upload) unuploaded = false;
      status = "Draft saved";
      error = "";
    } catch (err) {
      dirty = dirty || wasDirty;
      error = `The draft could not be saved: ${errorText(err)}`;
    }
  }

  /** Saves and closes, e.g. when another message is opened. */
  export async function flush() {
    if (closed) return;
    await save(unuploaded);
    closed = true;
  }

  async function close() {
    await flush();
    onClose("saved");
  }

  async function send() {
    if (busy) return;
    if (!fields.subject.trim()) {
      const go = await ask("This message has no subject. Send it anyway?", {
        title: "No subject",
        okLabel: "Send",
        cancelLabel: "Add a subject",
      });
      if (!go) return;
    }
    busy = true;
    clearTimeout(saveTimer);
    try {
      await api.sendDraft(draft.id, $state.snapshot(fields));
      closed = true;
      onClose("sent");
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function discard() {
    const empty = !fields.to && !fields.cc && !fields.bcc && !fields.subject && !fields.body.trim() && !attachments.length;
    if (!empty) {
      const confirmed = await ask("Delete this draft?", {
        title: "Discard draft",
        kind: "warning",
        okLabel: "Delete",
        cancelLabel: "Keep editing",
      });
      if (!confirmed) return;
    }
    clearTimeout(saveTimer);
    closed = true;
    try {
      await api.discardDraft(draft.id);
    } catch (err) {
      error = errorText(err);
      closed = false;
      return;
    }
    onClose("discarded");
  }

  async function attach() {
    error = "";
    try {
      await save(false);
      const updated = await api.attachFiles(draft.id);
      attachments = updated.attachments;
      unuploaded = true;
    } catch (err) {
      error = errorText(err);
    }
  }

  async function detach(index: number) {
    try {
      const updated = await api.removeAttachment(draft.id, index);
      attachments = updated.attachments;
      unuploaded = true;
    } catch (err) {
      error = errorText(err);
    }
  }

  /** Moves the draft to another account: a new draft there, this one deleted. */
  async function changeSender(accountId: number) {
    if (accountId === draft.account) return;
    try {
      const moved = await api.newDraft(accountId);
      const saved = await api.saveDraft(moved.id, $state.snapshot(fields), false);
      closed = true;
      await api.discardDraft(draft.id);
      onReplace(saved);
    } catch (err) {
      error = errorText(err);
    }
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      send();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="composer" aria-label="New message" onkeydown={keydown}>
  <div class="actions" role="toolbar" aria-label="Message actions">
    <button class="primary" onclick={send} disabled={busy}>{busy ? "Sending…" : "Send"}</button>
    <button onclick={attach} disabled={busy}>📎 Attach</button>
    <span class="status quiet" aria-live="polite">{status}</span>
    <button onclick={discard} disabled={busy} title="Delete this draft">Discard</button>
    <button onclick={close} disabled={busy} title="Save the draft and close">Close</button>
  </div>

  {#if error}<p class="error banner" role="alert">{error}</p>{/if}

  <div class="fields">
    <label for="compose-from">From</label>
    {#if accounts.length > 1 && attachments.length === 0}
      <select id="compose-from" value={draft.account} onchange={(e) => changeSender(Number(e.currentTarget.value))}>
        {#each accounts as a (a.id)}
          <option value={a.id}>{a.displayName} &lt;{a.email}&gt;</option>
        {/each}
      </select>
    {:else}
      <span id="compose-from" class="from">{account ? `${account.displayName} <${account.email}>` : ""}</span>
    {/if}

    <label for="compose-to">To</label>
    <div class="row">
      <input
        id="compose-to"
        bind:this={toInput}
        bind:value={fields.to}
        oninput={edited}
        autocomplete="off"
        spellcheck="false"
        placeholder="name@example.com, …"
      />
      {#if !showCopies}<button class="link" onclick={() => (showCopies = true)}>Cc Bcc</button>{/if}
    </div>

    {#if showCopies}
      <label for="compose-cc">Cc</label>
      <input id="compose-cc" bind:value={fields.cc} oninput={edited} autocomplete="off" spellcheck="false" />
      <label for="compose-bcc">Bcc</label>
      <input id="compose-bcc" bind:value={fields.bcc} oninput={edited} autocomplete="off" spellcheck="false" />
    {/if}

    <label for="compose-subject">Subject</label>
    <input id="compose-subject" bind:value={fields.subject} oninput={edited} />
  </div>

  {#if attachments.length}
    <ul class="attachments" aria-label="Attachments">
      {#each attachments as a, i (i)}
        <li title={a.mimeType}>
          📎 {a.name} <span class="quiet">{fileSize(a.size)}</span>
          <button class="remove" aria-label="Remove {a.name}" onclick={() => detach(i)}>✕</button>
        </li>
      {/each}
    </ul>
  {/if}

  <textarea
    bind:this={bodyInput}
    bind:value={fields.body}
    oninput={edited}
    aria-label="Message"
    placeholder="Write your message"
  ></textarea>
  <p class="hint quiet">Ctrl+Enter sends. Drafts are saved as you type.</p>
</section>

<style>
  .composer {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 24px;
    border-bottom: 1px solid var(--border);
  }
  .actions button {
    width: auto;
    padding: 5px 12px;
    font-size: 13px;
  }
  .status {
    flex: 1;
    font-size: 12px;
  }
  .banner {
    margin: 10px 24px 0;
  }
  .fields {
    display: grid;
    grid-template-columns: 64px 1fr;
    align-items: center;
    gap: 6px 10px;
    padding: 12px 24px;
    border-bottom: 1px solid var(--border);
  }
  label {
    color: var(--text-quiet);
    font-size: 13px;
  }
  .row {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .row .link {
    white-space: nowrap;
    font-size: 13px;
  }
  .from {
    padding: 8px 0;
  }
  .attachments {
    list-style: none;
    margin: 0;
    padding: 10px 24px;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    border-bottom: 1px solid var(--border);
  }
  .attachments li {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 4px 3px 8px;
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .remove {
    border: none;
    background: none;
    padding: 0 4px;
    color: var(--text-quiet);
  }
  textarea {
    flex: 1;
    min-height: 120px;
    resize: none;
    border: none;
    padding: 16px 24px;
    font: 14px/1.5 var(--font);
    color: var(--text);
    background: var(--bg);
  }
  textarea:focus {
    outline: none;
  }
  .hint {
    font-size: 12px;
    margin: 0;
    padding: 6px 24px 10px;
    border-top: 1px solid var(--border);
  }
</style>
