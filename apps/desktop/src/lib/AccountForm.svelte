<script lang="ts">
  import { tick } from "svelte";
  import {
    api,
    errorText,
    providerLabel,
    type Account,
    type AuthMethod,
    type Endpoint,
    type Security,
    type SetupInfo,
  } from "./api";

  interface Props {
    onAdded: (account: Account) => void;
    onCancel?: () => void;
    stepLabel?: string;
  }
  let { onAdded, onCancel, stepLabel = "" }: Props = $props();

  /** First the address, then how to sign in once its settings are known. */
  let step = $state<"address" | "sign-in">("address");
  let displayName = $state("");
  let email = $state("");
  let password = $state("");
  let username = $state("");
  let imap = $state<Endpoint>({ host: "", port: 993, security: "implicitTls" });
  let smtp = $state<Endpoint>({ host: "", port: 465, security: "implicitTls" });
  let info = $state<SetupInfo | null>(null);
  /** An (app) password instead of the provider's sign-in page. */
  let usePassword = $state(false);
  let showServers = $state(false);
  let looking = $state(false);
  let busy = $state(false);
  let waitingForBrowser = $state(false);
  /** Set when the person stops the browser sign-in, so it isn't shown as an error. */
  let stopped = false;
  let error = $state("");
  let passwordInput = $state<HTMLInputElement>();

  const imapPorts: Record<Security, number> = { implicitTls: 993, startTls: 143, insecure: 143 };
  const smtpPorts: Record<Security, number> = { implicitTls: 465, startTls: 587, insecure: 25 };

  /** The provider sign-in page this build can open for the address, if any. */
  const provider = $derived(info?.oauth && info.oauthAvailable ? info.oauth : null);
  const withProvider = $derived(provider !== null && !usePassword);
  const appPassword = $derived(/app(-specific)? password/i.test(info?.note ?? ""));
  /** Microsoft only accepts its own sign-in, which this build can't open. */
  const blocked = $derived(info?.oauth === "microsoft" && !info.oauthAvailable);
  const validEmail = $derived(/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email.trim()));

  async function findSettings(event: SubmitEvent) {
    event.preventDefault();
    if (!validEmail || looking) return;
    looking = true;
    error = "";
    try {
      info = await api.discoverSettings(email.trim());
    } catch (err) {
      info = null;
      error = errorText(err);
    } finally {
      looking = false;
    }
    if (!info) {
      error ||= "Enter a full email address, like you@example.com.";
      return;
    }
    imap = { ...info.imap };
    smtp = { ...info.smtp };
    username = info.username ?? "";
    usePassword = !(info.oauth && info.oauthAvailable);
    showServers = info.source === "guess";
    step = "sign-in";
    if (usePassword && !blocked) {
      await tick();
      passwordInput?.focus();
    }
  }

  function stopSignIn() {
    stopped = true;
    api.cancelSignIn();
  }

  function changeAddress() {
    step = "address";
    error = "";
    password = "";
  }

  function securityChanged(which: "imap" | "smtp") {
    // Keep a custom port; only swap one of the standard ports.
    if (which === "imap") {
      if (Object.values(imapPorts).includes(imap.port)) imap.port = imapPorts[imap.security];
    } else if (Object.values(smtpPorts).includes(smtp.port)) {
      smtp.port = smtpPorts[smtp.security];
    }
  }

  const canSignIn = $derived(
    imap.host.trim() !== "" && smtp.host.trim() !== "" && !busy && (withProvider || password.length > 0),
  );

  async function signIn(event?: SubmitEvent) {
    event?.preventDefault();
    if (!canSignIn) return;
    busy = true;
    waitingForBrowser = withProvider;
    stopped = false;
    error = "";
    const auth: AuthMethod = withProvider && provider ? { kind: "oauth2", provider } : { kind: "password" };
    try {
      const account = await api.addAccount({
        displayName: displayName.trim(),
        email: email.trim(),
        username: withProvider ? "" : username.trim(),
        password: withProvider ? "" : password,
        imap: { ...imap, host: imap.host.trim() },
        smtp: { ...smtp, host: smtp.host.trim() },
        auth,
      });
      onAdded(account);
    } catch (err) {
      if (stopped) return;
      error = errorText(err);
      if (/could not reach|timed out|secure connection|STARTTLS/i.test(error)) showServers = true;
    } finally {
      busy = false;
      waitingForBrowser = false;
    }
  }
</script>

{#if step === "address" || !info}
  <form onsubmit={findSettings}>
    <h1>Add an email account</h1>
    <p class="lead">Enter your address and Novero Mail finds the settings for you.</p>

    <label>
      <span>Your name</span>
      <input bind:value={displayName} placeholder="Shown to people you email" autocomplete="name" />
    </label>
    <label>
      <span>Email address</span>
      <input type="email" bind:value={email} placeholder="you@example.com" autocomplete="email" required />
    </label>

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <div class="actions">
      {#if onCancel}<button type="button" onclick={onCancel} disabled={looking}>Cancel</button>{/if}
      <button type="submit" class="primary" disabled={!validEmail || looking}>
        {#if looking}<span class="spinner small"></span> Finding settings…{:else}Continue{/if}
      </button>
    </div>
    {#if stepLabel}<p class="step quiet">{stepLabel}</p>{/if}
  </form>
{:else}
  <form onsubmit={signIn}>
    <h1>Sign in</h1>
    <div class="who">
      <span><strong>{email.trim()}</strong> <span class="quiet">· {info.name}</span></span>
      {#if !busy}<button type="button" class="link" onclick={changeAddress}>Change</button>{/if}
    </div>

    {#if waitingForBrowser && provider}
      <div class="waiting" role="status">
        <span class="spinner"></span>
        <div>
          <strong>Finish signing in in your browser</strong>
          <p class="quiet">
            {providerLabel[provider]}'s sign-in page opened in your browser. When you're done there, come back here.
          </p>
        </div>
      </div>
      <div class="actions">
        <button type="button" onclick={stopSignIn}>Cancel</button>
      </div>
    {:else if withProvider && provider}
      <button type="submit" class="primary provider" disabled={!canSignIn}>Sign in with {providerLabel[provider]}</button>
      <p class="quiet small">
        Your browser opens {providerLabel[provider]}'s own sign-in page, so Novero Mail never sees your password.
      </p>
      {#if provider === "google"}
        <button type="button" class="link toggle" onclick={() => (usePassword = true)}>Use an app password instead</button>
      {/if}
    {:else}
      {#if info.oauth === "microsoft" && !info.oauthAvailable}
        <p class="note">
          This version of Novero Mail can't sign in with Microsoft yet, and Microsoft no longer accepts passwords from
          mail apps, so this account can't be added yet.
        </p>
      {:else if info.oauth && !info.oauthAvailable}
        <p class="note">
          This version of Novero Mail can't use {providerLabel[info.oauth]} sign-in yet, so sign in with an app password.
        </p>
      {/if}
      {#if !blocked}
        <label>
          <span>{appPassword ? "App password" : "Password"}</span>
          <input type="password" bind:this={passwordInput} bind:value={password} autocomplete="current-password" required />
        </label>
      {/if}
      {#if info.note && info.source !== "guess" && info.oauth !== "microsoft"}<p class="note">{info.note}</p>{/if}
      {#if provider}
        <button type="button" class="link toggle" onclick={() => (usePassword = false)}>
          Sign in with {providerLabel[provider]} instead
        </button>
      {/if}
    {/if}

    {#if !waitingForBrowser}
      <button type="button" class="link toggle" onclick={() => (showServers = !showServers)}>
        {showServers ? "Hide server settings" : "Server settings"}
      </button>

      {#if showServers}
        {#if info.source === "guess"}<p class="note">{info.note}</p>{/if}
        <fieldset>
          <legend>Incoming mail (IMAP)</legend>
          <div class="row">
            <input bind:value={imap.host} placeholder="imap.example.com" aria-label="IMAP server" />
            <input type="number" bind:value={imap.port} min="1" max="65535" aria-label="IMAP port" class="port" />
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
            <input bind:value={smtp.host} placeholder="smtp.example.com" aria-label="SMTP server" />
            <input type="number" bind:value={smtp.port} min="1" max="65535" aria-label="SMTP port" class="port" />
            <select bind:value={smtp.security} onchange={() => securityChanged("smtp")} aria-label="SMTP security">
              <option value="implicitTls">SSL/TLS</option>
              <option value="startTls">STARTTLS</option>
              <option value="insecure">None (unsafe)</option>
            </select>
          </div>
        </fieldset>
        {#if !withProvider}
          <label>
            <span>Username <span class="quiet">(if different from your email address)</span></span>
            <input bind:value={username} placeholder={email || "you@example.com"} autocomplete="username" />
          </label>
        {/if}
        {#if imap.security === "insecure" || smtp.security === "insecure"}
          <p class="error">
            Without encryption, your password and mail travel in plain text. Use this only on a network you control.
          </p>
        {/if}
      {/if}
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    {#if !waitingForBrowser}
      <div class="actions">
        {#if onCancel}<button type="button" onclick={onCancel} disabled={busy}>Cancel</button>{/if}
        {#if !withProvider && !blocked}
          <button type="submit" class="primary" disabled={!canSignIn}>
            {#if busy}<span class="spinner small"></span> Signing in…{:else}Sign in{/if}
          </button>
        {/if}
      </div>
    {/if}
    {#if stepLabel}<p class="step quiet">{stepLabel}</p>{/if}
  </form>
{/if}

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
  .who {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    margin-top: -6px;
    word-break: break-all;
  }
  .provider {
    padding: 10px 14px;
    font-size: 15px;
  }
  .small {
    font-size: 13px;
    margin: -6px 0 0;
  }
  .waiting {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px;
  }
  .waiting p {
    margin: 4px 0 0;
    font-size: 13px;
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
  .actions:empty {
    display: none;
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
