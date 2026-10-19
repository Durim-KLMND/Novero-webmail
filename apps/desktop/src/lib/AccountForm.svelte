<script lang="ts">
  import { api, errorText, type Account, type Endpoint, type ProviderPreset, type Security } from "./api";

  interface Props {
    onAdded: (account: Account) => void;
    onCancel?: () => void;
    stepLabel?: string;
  }
  let { onAdded, onCancel, stepLabel = "" }: Props = $props();

  let displayName = $state("");
  let email = $state("");
  let password = $state("");
  let username = $state("");
  let imap = $state<Endpoint>({ host: "", port: 993, security: "implicitTls" });
  let smtp = $state<Endpoint>({ host: "", port: 465, security: "implicitTls" });
  let preset = $state<ProviderPreset | null>(null);
  let showServers = $state(false);
  let serversEdited = $state(false);
  let busy = $state(false);
  let error = $state("");

  const imapPorts: Record<Security, number> = { implicitTls: 993, startTls: 143, insecure: 143 };
  const smtpPorts: Record<Security, number> = { implicitTls: 465, startTls: 587, insecure: 25 };

  let lookup: ReturnType<typeof setTimeout> | undefined;
  function emailChanged() {
    clearTimeout(lookup);
    lookup = setTimeout(async () => {
      preset = email.includes("@") ? await api.providerPreset(email) : null;
      if (preset && !serversEdited) {
        imap = { ...preset.imap };
        smtp = { ...preset.smtp };
      }
    }, 250);
  }

  function securityChanged(which: "imap" | "smtp") {
    serversEdited = true;
    // Keep a custom port; only swap one of the standard ports.
    if (which === "imap") {
      if (Object.values(imapPorts).includes(imap.port)) imap.port = imapPorts[imap.security];
    } else if (Object.values(smtpPorts).includes(smtp.port)) {
      smtp.port = smtpPorts[smtp.security];
    }
  }

  const canSubmit = $derived(
    email.includes("@") && password.length > 0 && imap.host.trim() !== "" && smtp.host.trim() !== "" && !busy,
  );

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    busy = true;
    error = "";
    try {
      const account = await api.addAccount({
        displayName: displayName.trim(),
        email: email.trim(),
        username: username.trim(),
        password,
        imap: { ...imap, host: imap.host.trim() },
        smtp: { ...smtp, host: smtp.host.trim() },
      });
      onAdded(account);
    } catch (err) {
      error = errorText(err);
      if (/could not reach|timed out|secure connection|STARTTLS/i.test(error)) showServers = true;
    } finally {
      busy = false;
    }
  }
</script>

<form onsubmit={submit}>
  <h1>Add an email account</h1>
  <p class="lead">Novero Mail signs in with IMAP to read your mail.</p>

  <label>
    <span>Your name</span>
    <input bind:value={displayName} placeholder="Shown to people you email" autocomplete="name" />
  </label>
  <label>
    <span>Email address</span>
    <input
      type="email"
      bind:value={email}
      oninput={emailChanged}
      placeholder="you@example.com"
      autocomplete="email"
      required
    />
  </label>
  <label>
    <span>Password</span>
    <input type="password" bind:value={password} autocomplete="current-password" required />
  </label>

  {#if preset}
    <div class="provider">
      <span class="quiet">Provider: <strong>{preset.name}</strong></span>
      {#if preset.note}<p class="note">{preset.note}</p>{/if}
    </div>
  {/if}

  <button type="button" class="link toggle" onclick={() => (showServers = !showServers)}>
    {showServers ? "Hide server settings" : "Server settings"}
  </button>

  {#if showServers}
    <fieldset>
      <legend>Incoming mail (IMAP)</legend>
      <div class="row">
        <input bind:value={imap.host} oninput={() => (serversEdited = true)} placeholder="imap.example.com" aria-label="IMAP server" />
        <input type="number" bind:value={imap.port} oninput={() => (serversEdited = true)} min="1" max="65535" aria-label="IMAP port" class="port" />
        <select bind:value={imap.security} onchange={() => securityChanged("imap")} aria-label="IMAP security">
          <option value="implicitTls">SSL/TLS</option>
          <option value="startTls">STARTTLS</option>
          <option value="insecure">None (unsafe)</option>
        </select>
      </div>
    </fieldset>
    <fieldset>
      <legend>Outgoing mail (SMTP)</legend>
      <div class="row">
        <input bind:value={smtp.host} oninput={() => (serversEdited = true)} placeholder="smtp.example.com" aria-label="SMTP server" />
        <input type="number" bind:value={smtp.port} oninput={() => (serversEdited = true)} min="1" max="65535" aria-label="SMTP port" class="port" />
        <select bind:value={smtp.security} onchange={() => securityChanged("smtp")} aria-label="SMTP security">
          <option value="implicitTls">SSL/TLS</option>
          <option value="startTls">STARTTLS</option>
          <option value="insecure">None (unsafe)</option>
        </select>
      </div>
    </fieldset>
    <label>
      <span>Username <span class="quiet">(if different from your email address)</span></span>
      <input bind:value={username} placeholder={email || "you@example.com"} autocomplete="username" />
    </label>
    {#if imap.security === "insecure" || smtp.security === "insecure"}
      <p class="error">Without encryption, your password and mail travel in plain text. Use this only on a network you control.</p>
    {/if}
  {/if}

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  <div class="actions">
    {#if onCancel}<button type="button" onclick={onCancel} disabled={busy}>Cancel</button>{/if}
    <button type="submit" class="primary" disabled={!canSubmit}>
      {#if busy}<span class="spinner small"></span> Signing in…{:else}Sign in{/if}
    </button>
  </div>
  {#if stepLabel}<p class="step quiet">{stepLabel}</p>{/if}
</form>

<style>
  form {
    display: grid;
    gap: 14px;
  }
  h1 {
    font-size: 22px;
    margin: 0;
  }
  .lead {
    color: var(--text-quiet);
    margin: -6px 0 4px;
  }
  label {
    display: grid;
    gap: 4px;
  }
  label > span {
    font-weight: 600;
    font-size: 13px;
  }
  .provider {
    display: grid;
    gap: 6px;
  }
  .provider .note {
    margin: 0;
  }
  .toggle {
    justify-self: start;
  }
  fieldset {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 12px 12px;
    margin: 0;
  }
  legend {
    font-weight: 600;
    font-size: 13px;
    padding: 0 4px;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 80px 130px;
    gap: 8px;
  }
  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .error,
  .note {
    margin: 0;
  }
  .step {
    text-align: center;
    margin: 6px 0 0;
    font-size: 12px;
  }
  .spinner.small {
    width: 12px;
    height: 12px;
    border-color: rgba(255, 255, 255, 0.4);
    border-top-color: #fff;
    margin-right: 4px;
  }
</style>
