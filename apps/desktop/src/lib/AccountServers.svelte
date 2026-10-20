<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, providerLabel, type Account, type Endpoint } from "./api";
  import { imapPorts, portFor, securityChoices, smtpPorts } from "./servers";

  interface Props {
    account: Account;
    onSaved: (account: Account) => void;
    onCancel: () => void;
  }
  let { account, onSaved, onCancel }: Props = $props();

  // svelte-ignore state_referenced_locally
  let imap = $state<Endpoint>({ ...account.imap });
  // svelte-ignore state_referenced_locally
  let smtp = $state<Endpoint>({ ...account.smtp });
  // svelte-ignore state_referenced_locally
  let username = $state(account.username === account.email ? "" : account.username);
  let password = $state("");
  let saving = $state(false);
  let error = $state("");
  let first = $state<HTMLInputElement>();

  const withPassword = $derived(account.auth.kind === "password");
  const validPort = (p: number) => Number.isInteger(p) && p >= 1 && p <= 65535;
  const changed = $derived(
    JSON.stringify([imap, smtp]) !== JSON.stringify([account.imap, account.smtp]) ||
      (withPassword && ((username.trim() || account.email) !== account.username || password !== "")),
  );
  const canSave = $derived(
    changed && !saving && imap.host.trim() !== "" && smtp.host.trim() !== "" && validPort(imap.port) && validPort(smtp.port),
  );

  onMount(() => first?.focus());

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!canSave) return;
    saving = true;
    error = "";
    try {
      const saved = await api.updateAccountServers(account.id, {
        imap: $state.snapshot(imap),
        smtp: $state.snapshot(smtp),
        username: username.trim(),
        password,
      });
      onSaved(saved);
    } catch (err) {
      error = errorText(err);
    } finally {
      saving = false;
    }
  }

  function keydown(event: KeyboardEvent) {
    // Escape closes this form, not all of Settings.
    if (event.key === "Escape" && !saving) {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form class="servers" onsubmit={save} onkeydown={keydown} aria-label="Server settings for {account.email}">
  <fieldset disabled={saving}>
    <legend>Incoming mail (IMAP)</legend>
    <div class="row">
      <label>
        <span>Server</span>
        <input bind:this={first} bind:value={imap.host} placeholder="imap.example.com" spellcheck="false" />
      </label>
      <label>
        <span>Port</span>
        <input type="number" bind:value={imap.port} min="1" max="65535" class="port" />
      </label>
      <label>
        <span>Security</span>
        <select bind:value={imap.security} onchange={() => (imap.port = portFor(imap, imapPorts))}>
          {#each securityChoices as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
        </select>
      </label>
    </div>
  </fieldset>
  <fieldset disabled={saving}>
    <legend>Outgoing mail (SMTP)</legend>
    <div class="row">
      <label>
        <span>Server</span>
        <input bind:value={smtp.host} placeholder="smtp.example.com" spellcheck="false" />
      </label>
      <label>
        <span>Port</span>
        <input type="number" bind:value={smtp.port} min="1" max="65535" class="port" />
      </label>
      <label>
        <span>Security</span>
        <select bind:value={smtp.security} onchange={() => (smtp.port = portFor(smtp, smtpPorts))}>
          {#each securityChoices as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
        </select>
      </label>
    </div>
  </fieldset>

  {#if withPassword}
    <fieldset disabled={saving} class="login">
      <label>
        <span>Username <span class="quiet">(if not your email address)</span></span>
        <input bind:value={username} placeholder={account.email} autocomplete="username" spellcheck="false" />
      </label>
      <label>
        <span>Password <span class="quiet">(only to change it)</span></span>
        <input type="password" bind:value={password} placeholder="Keep the saved password" autocomplete="current-password" />
      </label>
    </fieldset>
  {:else if account.auth.kind === "oauth2"}
    <small class="quiet">Signs in with {providerLabel[account.auth.provider]} as {account.email}.</small>
  {/if}

  {#if imap.security === "insecure" || smtp.security === "insecure"}
    <p class="error">Without encryption, your password and mail travel in plain text. Use this only on a network you control.</p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}

  <div class="actions">
    {#if saving}<span class="quiet" role="status">Checking the servers…</span>{/if}
    <button type="button" onclick={onCancel} disabled={saving}>Cancel</button>
    <button type="submit" class="primary" disabled={!canSave}>{saving ? "Checking…" : "Save servers"}</button>
  </div>
  <small class="quiet">Both servers are tried before anything is saved.</small>
</form>

<style>
  .servers {
    display: grid;
    gap: 10px;
    padding: 4px 12px 12px;
  }
  fieldset {
    border: none;
    padding: 0;
    margin: 0;
    min-width: 0;
  }
  legend {
    font-weight: 600;
    font-size: 13px;
    margin-bottom: 4px;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 84px 130px;
    gap: 8px;
  }
  .login {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  label {
    display: grid;
    gap: 3px;
    min-width: 0;
  }
  label > span {
    font-size: 12px;
    color: var(--text-quiet);
  }
  .error {
    margin: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
  }
  .actions [role="status"] {
    margin-right: auto;
  }
  @media (max-width: 620px) {
    .row,
    .login {
      grid-template-columns: 1fr;
    }
  }
</style>
