<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type Account } from "./lib/api";
  import Onboarding from "./lib/Onboarding.svelte";
  import MailView from "./lib/MailView.svelte";

  let accounts = $state<Account[] | null>(null);
  let loadError = $state("");
  let adding = $state(false);

  async function refreshAccounts() {
    try {
      accounts = await api.listAccounts();
    } catch (err) {
      loadError = errorText(err);
    }
  }

  onMount(refreshAccounts);
</script>

{#if loadError}
  <div class="center"><p class="error">Novero Mail could not start: {loadError}</p></div>
{:else if accounts === null}
  <div class="center"><span class="spinner"></span></div>
{:else if accounts.length === 0}
  <Onboarding firstRun onFinish={refreshAccounts} />
{:else}
  <MailView {accounts} onAddAccount={() => (adding = true)} onAccountsChanged={refreshAccounts} />
  {#if adding}
    <div class="overlay">
      <Onboarding
        firstRun={false}
        onFinish={async () => {
          adding = false;
          await refreshAccounts();
        }}
        onCancel={() => (adding = false)}
      />
    </div>
  {/if}
{/if}

<style>
  .center {
    height: 100%;
    display: grid;
    place-items: center;
  }
  .overlay {
    position: fixed;
    inset: 0;
    background: var(--bg);
    z-index: 10;
  }
</style>
