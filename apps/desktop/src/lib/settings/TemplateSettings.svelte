<script lang="ts">
  // The Templates part of Settings: messages saved to start new ones from.
  // Saved with Settings.
  import { onMount } from "svelte";
  import { api, errorText, type Template } from "../api";
  import Icon from "../ui/Icon.svelte";
  import { needPro } from "../plan/pro.svelte";
  import RichEditor from "../compose/RichEditor.svelte";
  import { textToHtml } from "../compose/richtext";

  interface Entry extends Template {
    key: string;
    saved: Template | null;
  }
  let entries = $state<Entry[]>([]);
  let removed: number[] = [];
  let next = 0;
  let error = $state("");

  onMount(async () => {
    try {
      entries = (await api.listTemplates()).map((t) => ({ ...t, key: `t${t.id}`, saved: t }));
    } catch (err) {
      error = errorText(err);
    }
  });

  function add() {
    if (!needPro("templates")) return;
    entries.push({ id: 0, name: "", subject: "", text: "", html: "", key: `n${next++}`, saved: null });
  }

  function remove(entry: Entry) {
    if (entry.id) removed.push(entry.id);
    entries = entries.filter((e) => e.key !== entry.key);
  }

  /** Saves what changed; true when anything did. */
  export async function save(): Promise<boolean> {
    let changed = removed.length > 0;
    for (const id of removed) await api.deleteTemplate(id);
    removed = [];
    for (const [i, entry] of entries.entries()) {
      const { key: _, saved, ...template } = $state.snapshot(entry) as Entry;
      template.name = template.name.trim() || template.subject.trim() || `Template ${i + 1}`;
      const same =
        saved &&
        (["name", "subject", "text", "html"] as const).every((field) => saved[field] === template[field]);
      if (same) continue;
      const stored = await api.saveTemplate(template);
      entry.id = stored.id;
      entry.saved = stored;
      changed = true;
    }
    return changed;
  }
</script>

<p class="quiet intro">
  In a message, the templates button starts it from one of these, or saves what you're writing as a new one.
</p>
{#if error}<p class="error">{error}</p>{/if}
{#each entries as entry, i (entry.key)}
  <div class="template">
    <div class="name">
      <input placeholder="Template {i + 1}" aria-label="Name of template {i + 1}" bind:value={entry.name} />
      <button
        class="icon-only danger"
        title="Delete this template"
        aria-label="Delete template {entry.name || i + 1}"
        onclick={() => remove(entry)}><Icon name="trash" /></button
      >
    </div>
    <input placeholder="Subject (optional)" aria-label="Subject of template {i + 1}" bind:value={entry.subject} />
    <RichEditor
      compact
      label="Text of template {entry.name || i + 1}"
      html={entry.html || textToHtml(entry.text)}
      placeholder="The text of the message"
      onChange={(text, html) => {
        entry.text = text;
        entry.html = html;
      }}
    />
  </div>
{:else}
  <p class="quiet">No templates yet.</p>
{/each}
<div class="row">
  <button onclick={add}><Icon name="plus" /> Add template</button>
</div>

<style>
  .intro {
    margin: 0 0 12px;
  }
  .template {
    display: grid;
    gap: 6px;
    margin-bottom: 18px;
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
  .row button {
    white-space: nowrap;
  }
</style>
