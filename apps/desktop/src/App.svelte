<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type Account } from "./lib/api";
  import Onboarding from "./lib/accounts/Onboarding.svelte";
  import MailView from "./lib/mail/MailView.svelte";
  import ContextMenu from "./lib/shell/ContextMenu.svelte";
  import ComposerWindow from "./lib/compose/ComposerWindow.svelte";
  import TitleBar from "./lib/shell/TitleBar.svelte";

  /** Set in a window opened for one draft ("New message in a new window"). */
  const composeId = Number(new URLSearchParams(location.search).get("compose")) || null;

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

  onMount(() => {
    if (composeId == null) refreshAccounts();
  });
</script>

{#if composeId != null}
  <ComposerWindow id={composeId} />
{:else if loadError}
  <div class="center"><p class="error">Novero Mail could not start: {loadError}</p></div>
{:else if accounts === null}
  <div class="center"><span class="spinner"></span></div>
{:else if accounts.length === 0}
  <Onboarding firstRun onFinish={refreshAccounts} />
{:else}
  <MailView {accounts} onAddAccount={() => (adding = true)} onAccountsChanged={refreshAccounts} />
  {#if adding}
    <div class="overlay" role="dialog" aria-modal="true" aria-label="Add account">
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

{#if composeId == null}
  <TitleBar title="Novero Mail" />
{/if}
<ContextMenu />

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
