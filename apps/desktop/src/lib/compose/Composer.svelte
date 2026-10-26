<script lang="ts">
  import Icon from "../ui/Icon.svelte";
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api,
    errorText,
    onFileDrop,
    type Account,
    type ComposerOutcome,
    type Draft,
    type DraftFields,
    type FileDrop,
    type Signature,
    type Template,
  } from "../api";
  import { fileSize } from "../format";
  import RichEditor from "./RichEditor.svelte";
  import AddressInput from "./AddressInput.svelte";
  import { openMenu, toggleMenuUnder } from "../shell/menus.svelte";
  import { laterChoices } from "../later";
  import PickTime from "../ui/PickTime.svelte";
  import NameDialog from "../ui/NameDialog.svelte";
  import { toast } from "../shell/toast.svelte";
  import { textToHtml } from "./richtext";

  interface Props {
    draft: Draft;
    accounts: Account[];
    /** The composer is done: sent, saved or thrown away. */
    onClose: (outcome: ComposerOutcome) => void;
    /** Another account was picked as the sender; the draft moved to it. */
    onReplace: (draft: Draft) => void;
    /** Moves the draft to a window of its own; left out in that window. */
    onPopOut?: () => void;
    /** Close says "Close window" in a window of its own. */
    inWindow?: boolean;
  }
  let { draft, accounts, onClose, onReplace, onPopOut, inWindow = false }: Props = $props();

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
    html: start.html,
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
  let toInput = $state<AddressInput>();
  let editor = $state<RichEditor>();
  /** What the editor starts with: the formatted text, or the plain text shown as such. */
  const startHtml = start.html || textToHtml(start.body);
  let closed = false;

  const account = $derived(accounts.find((a) => a.id === draft.account));

  /** Every account's signatures, and the templates, for the menus. */
  let signatures = $state<Signature[]>([]);
  let templates = $state<Template[]>([]);
  /** The signature in the text now: its id, plain text and formatted text. */
  let signature = $state<{ id: number | null; text: string; html: string }>({ id: null, text: "", html: "" });
  let naming = $state(false);
  const mySignatures = $derived(signatures.filter((s) => s.account === draft.account));

  /** The signature a new message (or, for `reply`, a reply) from `a` starts with. */
  function startingSignature(a: Account | undefined, reply: boolean) {
    if (!a) return { id: null, text: "", html: "" };
    return reply
      ? { id: a.signatureReply, text: a.replySignature, html: a.replySignatureHtml }
      : { id: a.signatureNew, text: a.signature, html: a.signatureHtml };
  }

  onMount(async () => {
    await tick();
    // Which signature the draft started with: replies and forwards have
    // their own. A new draft has it as plain text; formatting takes its place.
    const reply = Boolean(draft.inReplyTo);
    const found = [startingSignature(account, reply), startingSignature(account, !reply)].find(
      (s) => s.id != null && editor?.hasSignature(s.text),
    );
    if (found) {
      signature = found;
      if (!start.html && found.html) editor?.replaceSignature(found.text, found.text, found.html);
    }
    Promise.all([api.listSignatures(), api.listTemplates()])
      .then(([s, t]) => ([signatures, templates] = [s, t]))
      .catch(() => {
        // The menus just have less in them.
      });
    if (draft.inReplyTo && fields.to) {
      // A reply: start typing above the quoted text.
      editor?.focusStart();
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
    unlistenDrop.then((fn) => fn());
  });

  let section = $state<HTMLElement>();
  /** Files are dragged over the composer: onto the text (pictures go in it)
   *  or anywhere else (everything is attached). */
  let dropZone = $state<"text" | "attach" | null>(null);

  function zoneAt(x: number, y: number): "text" | "attach" | null {
    const el = document.elementFromPoint(x, y);
    if (!el || !section?.contains(el)) return null;
    return el.closest(".editor") ? "text" : "attach";
  }

  const unlistenDrop = onFileDrop((event) => dropped(event));

  async function dropped(event: FileDrop) {
    if (event.kind === "leave") {
      dropZone = null;
      return;
    }
    const zone = zoneAt(event.x, event.y);
    if (event.kind === "over") {
      dropZone = zone;
      return;
    }
    dropZone = null;
    if (!zone || closed) return;
    const pictures = zone === "text" ? event.files.flatMap((f, i) => (f.picture ? [i] : [])) : [];
    const others = event.files.flatMap((_, i) => (pictures.includes(i) ? [] : [i]));
    error = "";
    try {
      let at = editor?.rangeAt(event.x, event.y) ?? null;
      for (const index of pictures) {
        await editor?.insertPicture(await api.droppedPicture(event.id, index), at);
        at = null;
      }
      if (others.length) {
        await save(false);
        const updated = await api.attachDropped(draft.id, event.id, others);
        attachments = updated.attachments;
        unuploaded = true;
      }
    } catch (err) {
      error = errorText(err);
    }
  }

  function bodyChanged(text: string, html: string) {
    fields.body = text;
    fields.html = html;
    edited();
  }

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

  async function popOut() {
    await flush();
    onPopOut?.();
  }

  /** "Pick a date and time" for Send later is open. */
  let picking = $state(false);

  function sendLaterMenu(e: MouseEvent) {
    const button = e.currentTarget as HTMLElement;
    toggleMenuUnder(button.parentElement ?? button, [
      ...laterChoices(new Date(), false).map((c) => ({
        label: `${c.label}`,
        shortcut: c.when,
        icon: "clock" as const,
        run: () => send(c.at),
      })),
      "separator",
      { label: "Pick a date and time…", icon: "calendar", run: () => (picking = true) },
    ]);
  }

  /** Sends now, or at `at`: the message waits in the outbox until then. */
  async function send(at: Date | null = null) {
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
      await api.sendDraft(draft.id, $state.snapshot(fields), at ? Math.floor(at.getTime() / 1000) : null);
      closed = true;
      onClose(at ? "scheduled" : "sent");
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

  /** Puts another signature in place of the one in the text, or adds it. */
  function useSignature(next: { id: number | null; text: string; html: string }) {
    editor?.replaceSignature(signature.text, next.text, next.html);
    signature = next;
  }

  function signatureMenu(event: MouseEvent) {
    toggleMenuUnder(event.currentTarget as Element, [
      ...mySignatures.map((s) => ({
        label: s.name,
        icon: signature.id === s.id ? ("check" as const) : undefined,
        run: () => useSignature(s),
      })),
      "separator" as const,
      {
        label: "No signature",
        icon: signature.id == null ? ("check" as const) : undefined,
        run: () => useSignature({ id: null, text: "", html: "" }),
      },
    ]);
  }

  /** Starts from a template: its text at the cursor, and its subject if there is none yet. */
  function useTemplate(template: Template) {
    if (!fields.subject.trim() && template.subject) fields.subject = template.subject;
    editor?.insertHtml(template.html || textToHtml(template.text));
    edited();
  }

  function templateMenu(event: MouseEvent) {
    toggleMenuUnder(event.currentTarget as Element, [
      ...templates.map((t) => ({ label: t.name, icon: "template" as const, run: () => useTemplate(t) })),
      ...(templates.length ? ["separator" as const] : []),
      { label: "Save as template…", icon: "plus", run: () => (naming = true) },
    ]);
  }

  /** Keeps the subject and what was written (not the signature or quote) as a template. */
  async function saveTemplate(name: string) {
    naming = false;
    const { text, html } = editor?.ownText(signature.text) ?? { text: "", html: "" };
    try {
      const saved = await api.saveTemplate({ id: 0, name, subject: fields.subject.trim(), text, html });
      templates = [...templates, saved].sort((a, b) => a.name.localeCompare(b.name));
      toast(`Saved the template "${saved.name}".`);
    } catch (err) {
      error = errorText(err);
    }
  }

  /** Moves the draft to another account: a new draft there, this one deleted. */
  async function changeSender(accountId: number) {
    if (accountId === draft.account) return;
    // The old sender's signature gives way to the new one's.
    useSignature(startingSignature(accounts.find((a) => a.id === accountId), Boolean(draft.inReplyTo)));
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
<section class="composer" aria-label="New message" onkeydown={keydown} bind:this={section}>
  {#if dropZone}
    <div class="drop" class:text={dropZone === "text"}>
      <Icon name={dropZone === "text" ? "image" : "attachment"} size={22} />
      {dropZone === "text" ? "Drop pictures to put them in the text; other files are attached" : "Drop files to attach them"}
    </div>
  {/if}
  <div class="actions" role="toolbar" aria-label="Message actions">
    <div class="send" role="group" aria-label="Send">
      <button class="primary main" onclick={() => send()} disabled={busy} title="Send (Ctrl+Enter)"><Icon name="sent" />{busy ? "Sending…" : "Send"}</button>
      <button
        class="primary more"
        onclick={sendLaterMenu}
        disabled={busy}
        aria-label="Send later"
        aria-haspopup="menu"
        title="Send later"
      ><Icon name="chevron-down" size={14} /></button>
    </div>
    <button onclick={attach} disabled={busy}><Icon name="attachment" />Attach</button>
    {#if mySignatures.length}
      <button
        class="icon-only"
        onclick={signatureMenu}
        disabled={busy}
        aria-haspopup="menu"
        aria-label="Signature"
        title="Change the signature"><Icon name="signature" /></button
      >
    {/if}
    <button
      class="icon-only"
      onclick={templateMenu}
      disabled={busy}
      aria-haspopup="menu"
      aria-label="Templates"
      title="Start from a template, or save this message as one"><Icon name="template" /></button
    >
    <span class="status quiet" aria-live="polite">{status}</span>
    <button onclick={discard} disabled={busy} title="Delete this draft"><Icon name="trash" />Discard</button>
    {#if onPopOut}
      <button
        class="icon-only"
        onclick={popOut}
        disabled={busy}
        title="Open in a new window"
        aria-label="Open in a new window"
      ><Icon name="external-link" /></button>
    {/if}
    <button onclick={close} disabled={busy} title={inWindow ? "Save the draft and close the window" : "Save the draft and close"}><Icon name="close" />Close</button>
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
      <AddressInput id="compose-to" bind:this={toInput} bind:value={fields.to} onInput={edited} placeholder="name@example.com, …" />
      {#if !showCopies}<button class="link" onclick={() => (showCopies = true)}>Cc Bcc</button>{/if}
    </div>

    {#if showCopies}
      <label for="compose-cc">Cc</label>
      <AddressInput id="compose-cc" bind:value={fields.cc} onInput={edited} />
      <label for="compose-bcc">Bcc</label>
      <AddressInput id="compose-bcc" bind:value={fields.bcc} onInput={edited} />
    {/if}

    <label for="compose-subject">Subject</label>
    <input id="compose-subject" bind:value={fields.subject} oninput={edited} />
  </div>

  {#if attachments.length}
    <ul class="attachments" aria-label="Attachments">
      {#each attachments as a, i (i)}
        <li
          title={a.mimeType}
          oncontextmenu={(e) => openMenu(e, [{ label: "Remove attachment", icon: "close", danger: true, run: () => detach(i) }])}
        >
          <Icon name="attachment" size={14} /> {a.name} <span class="quiet">{fileSize(a.size)}</span>
          <button class="remove" aria-label="Remove {a.name}" onclick={() => detach(i)}><Icon name="close" size={14} /></button>
        </li>
      {/each}
    </ul>
  {/if}

  <RichEditor bind:this={editor} html={startHtml} placeholder="Write your message" onChange={bodyChanged} />
  <p class="hint quiet">Ctrl+Enter sends. Drafts are saved as you type.</p>
</section>

{#if picking}
  <PickTime
    title="Send later"
    action="Schedule"
    start={laterChoices(new Date(), false)[0].at}
    onPick={(at) => {
      picking = false;
      send(at);
    }}
    onCancel={() => (picking = false)}
  />
{/if}

<style>
  .composer {
    position: relative;
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
  .send {
    display: flex;
  }
  .send .main {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .actions .send .more {
    padding: 5px 7px;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
    border-left: 1px solid color-mix(in srgb, var(--accent-text) 30%, transparent);
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
  .drop {
    position: absolute;
    inset: 8px;
    z-index: 30;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    border: 2px dashed var(--accent-ink);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--bg) 85%, transparent);
    color: var(--accent-ink);
    font-weight: 600;
    pointer-events: none;
  }
  .hint {
    font-size: 12px;
    margin: 0;
    padding: 6px 24px 10px;
    border-top: 1px solid var(--border);
  }
</style>

{#if naming}
  <NameDialog
    title="Save as template"
    action="Save"
    start={fields.subject.trim()}
    onName={saveTemplate}
    onCancel={() => (naming = false)}
  />
{/if}
