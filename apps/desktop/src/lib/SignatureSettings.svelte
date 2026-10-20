<script lang="ts">
  // The Signatures part of Settings: each account's signatures, and which
  // ones its new messages and replies start with. Saved with Settings.
  import { onMount } from "svelte";
  import { api, errorText, type Account, type Signature } from "./api";
  import Icon from "./Icon.svelte";
  import RichEditor from "./RichEditor.svelte";
  import { textToHtml } from "./richtext";

  interface Props {
    accounts: Account[];
  }
  let { accounts }: Props = $props();

  /** A signature being edited; `key` stays the same once it is saved. */
  interface Entry extends Signature {
    key: string;
    saved: Signature | null;
  }
  let entries = $state<Entry[]>([]);
  /** Per account: the keys of the signatures for new messages and replies. */
  let defaults = $state<Record<number, { new: string | null; reply: string | null }>>({});
  let removed: number[] = [];
  let next = 0;
  let error = $state("");

  onMount(async () => {
    try {
      const list = await api.listSignatures();
      entries = list.map((s) => ({ ...s, key: `s${s.id}`, saved: s }));
    } catch (err) {
      error = errorText(err);
    }
    defaults = Object.fromEntries(
      accounts.map((a) => [
        a.id,
        {
          new: a.signatureNew == null ? null : `s${a.signatureNew}`,
          reply: a.signatureReply == null ? null : `s${a.signatureReply}`,
        },
      ]),
    );
  });

  function add(account: Account) {
    const key = `n${next++}`;
    const first = !entries.some((e) => e.account === account.id);
    entries.push({ id: 0, account: account.id, name: first ? "Signature" : "", text: "", html: "", key, saved: null });
    // An account's first signature is used for everything.
    if (first) defaults[account.id] = { new: key, reply: key };
  }

  function remove(entry: Entry) {
    if (entry.id) removed.push(entry.id);
    entries = entries.filter((e) => e.key !== entry.key);
    const d = defaults[entry.account];
    if (d?.new === entry.key) d.new = null;
    if (d?.reply === entry.key) d.reply = null;
  }

  function label(entry: Entry, at: number): string {
    return entry.name.trim() || `Signature ${at + 1}`;
  }

  /** Saves what changed; true when anything did. */
  export async function save(): Promise<boolean> {
    let changed = removed.length > 0;
    for (const id of removed) await api.deleteSignature(id);
    removed = [];
    const ids: Record<string, number> = {};
    for (const entry of entries) {
      const { key, saved, ...signature } = $state.snapshot(entry) as Entry;
      const mine = entries.filter((e) => e.account === entry.account);
      signature.name = label(entry, mine.indexOf(entry));
      const same =
        saved && saved.name === signature.name && saved.text === signature.text && saved.html === signature.html;
      if (same) {
        ids[key] = saved.id;
        continue;
      }
      const stored = await api.saveSignature(signature);
      entry.id = stored.id;
      entry.saved = stored;
      ids[key] = stored.id;
      changed = true;
    }
    for (const a of accounts) {
      const d = defaults[a.id];
      if (!d) continue;
      const pick = (key: string | null) => (key && key in ids ? ids[key] : null);
      const [newId, replyId] = [pick(d.new), pick(d.reply)];
      // Saving an account's first signature picks it, so this always runs.
      if (changed || newId !== a.signatureNew || replyId !== a.signatureReply) {
        await api.setDefaultSignatures(a.id, newId, replyId);
        changed = true;
      }
    }
    return changed;
  }
</script>

<p class="quiet intro">Pick which signature new messages and replies start with. You can change it in each message.</p>
{#if error}<p class="error">{error}</p>{/if}
{#each accounts as a (a.id)}
  {@const mine = entries.filter((e) => e.account === a.id)}
  <section class="account" aria-label="Signatures for {a.email}">
    <h4>
      {a.displayName}
      {#if a.displayName !== a.email}<small class="quiet">{a.email}</small>{/if}
    </h4>
    {#each mine as entry, i (entry.key)}
      <div class="sig">
        <div class="name">
          <input placeholder="Signature {i + 1}" aria-label="Name of signature {i + 1}" bind:value={entry.name} />
          <button
            class="icon-only danger"
            title="Delete this signature"
            aria-label="Delete {label(entry, i)}"
            onclick={() => remove(entry)}><Icon name="trash" /></button
          >
        </div>
        <RichEditor
          compact
          label="Signature {label(entry, i)} for {a.email}"
          html={entry.html || textToHtml(entry.text)}
          placeholder="Your name, and anything else to put below your messages"
          onChange={(text, html) => {
            entry.text = text;
            entry.html = html;
          }}
        />
      </div>
    {/each}
    <div class="row">
      <button onclick={() => add(a)}><Icon name="plus" /> Add signature</button>
    </div>
    {#if mine.length && defaults[a.id]}
      <div class="defaults">
        <label for="sig-new-{a.id}">New messages</label>
        <select id="sig-new-{a.id}" bind:value={defaults[a.id].new}>
          <option value={null}>No signature</option>
          {#each mine as entry, i (entry.key)}<option value={entry.key}>{label(entry, i)}</option>{/each}
        </select>
        <label for="sig-reply-{a.id}">Replies and forwards</label>
        <select id="sig-reply-{a.id}" bind:value={defaults[a.id].reply}>
          <option value={null}>No signature</option>
          {#each mine as entry, i (entry.key)}<option value={entry.key}>{label(entry, i)}</option>{/each}
        </select>
      </div>
    {/if}
  </section>
{/each}

<style>
  .intro {
    margin: 0 0 12px;
  }
  .account + .account {
    margin-top: 22px;
    padding-top: 16px;
    border-top: 1px solid var(--border);
  }
  h4 {
    margin: 0 0 8px;
    font-size: 14px;
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  h4 small {
    font-weight: normal;
  }
  .sig {
    display: grid;
    gap: 4px;
    margin-bottom: 12px;
  }
  .name {
    display: flex;
    gap: 6px;
  }
  .name input {
    flex: 1;
    font-weight: 600;
  }
  .danger:hover {
    color: var(--danger);
  }
  .row {
    margin: 4px 0 10px;
  }
  .row button {
    white-space: nowrap;
  }
  .defaults {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 6px 12px;
    max-width: 420px;
  }
  .defaults label {
    color: var(--text-quiet);
    font-size: 13px;
  }
</style>
