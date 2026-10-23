<script lang="ts">
  import { api, errorText, providerLabel, type Account } from "../api";

  interface Props {
    account: Account;
    onDone: (signedIn: boolean) => void;
  }
  let { account, onDone }: Props = $props();

  let password = $state("");
  let busy = $state(false);
  let error = $state("");
  let stopped = false;

  const provider = $derived(account.auth.kind === "oauth2" ? account.auth.provider : null);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy || (!provider && !password)) return;
    busy = true;
    stopped = false;
    error = "";
    try {
      await api.signInAgain(account.id, provider ? null : password);
      onDone(true);
    } catch (err) {
      if (!stopped) error = errorText(err);
    } finally {
      busy = false;
    }
  }

  function cancel() {
    if (busy && provider) {
      stopped = true;
      api.cancelSignIn();
    }
    else onDone(false);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") cancel();
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="backdrop">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="sign-in-again">
    <form onsubmit={submit}>
      <h2 id="sign-in-again">Sign in again</h2>
      <p class="quiet">{account.email}</p>
      {#if provider}
        {#if busy}
          <p class="waiting"><span class="spinner small"></span> Finish signing in in your browser, then come back here.</p>
        {:else}
          <p>Your browser opens {providerLabel[provider]}'s sign-in page. Choose {account.email} there.</p>
        {/if}
      {:else}
        <label>
          <span>Password</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input type="password" bind:value={password} autocomplete="current-password" autofocus required />
        </label>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="actions">
        <button type="button" onclick={cancel}>Cancel</button>
        {#if !(provider && busy)}
          <button type="submit" class="primary" disabled={busy || (!provider && !password)}>
            {#if busy}<span class="spinner small"></span> Signing in…{:else if provider}Sign in with {providerLabel[provider]}{:else}Sign in{/if}
          </button>
        {/if}
      </div>
    </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: grid;
    place-items: center;
    z-index: 20;
  }
  .dialog {
    width: min(420px, calc(100% - 32px));
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 24px;
  }
  form {
    display: grid;
    gap: 12px;
  }
  h2 {
    margin: 0;
    font-size: 18px;
  }
  p {
    margin: 0;
  }
  label {
    display: grid;
    gap: 4px;
  }
  label > span {
    font-weight: 600;
    font-size: 13px;
  }
  .waiting {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .spinner.small {
    width: 12px;
    height: 12px;
  }
</style>
