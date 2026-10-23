<script lang="ts">
  // Shown when something needs Pro: what Pro adds, buying it, and a place for
  // the key. Works in any window, the message windows included.
  import { onMount } from "svelte";
  import { api, errorText } from "../api";
  import Icon from "../ui/Icon.svelte";
  import LicenseForm from "./LicenseForm.svelte";
  import { featureNames, pro, type ProFeature } from "./pro.svelte";

  interface Props {
    feature: ProFeature;
    onClose: () => void;
  }
  let { feature, onClose }: Props = $props();

  let dialog = $state<HTMLDivElement>();
  let error = $state("");
  let opened = $state(false);
  const canBuy = $derived(pro.plan?.canBuy ?? false);

  onMount(() => dialog?.querySelector<HTMLElement>("button.primary, button")?.focus());

  async function buy() {
    error = "";
    try {
      await api.openCheckout();
      opened = true;
    } catch (err) {
      error = errorText(err);
    }
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  }

  const perks = [
    "Unlimited mail accounts",
    "Google and Outlook calendar sync",
    "Rules that sort new mail",
    "Send later and snooze",
    "Templates and more signatures",
  ];
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="backdrop" onkeydown={keydown} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="upgrade-title" bind:this={dialog}>
    <div class="head">
      <Icon name="pro" size={22} />
      <h2 id="upgrade-title">{featureNames[feature]} {feature === "accounts" || feature === "signatures" ? "need" : "needs"} Novero Mail Pro</h2>
    </div>
    <p class="quiet">Free covers everyday mail. Pro is a one-time purchase that adds:</p>
    <ul>
      {#each perks as perk (perk)}
        <li><Icon name="check" /> {perk}</li>
      {/each}
    </ul>
    {#if opened}
      <p class="quiet" role="status">The checkout opened in your browser. Paste the key from the email here.</p>
    {/if}
    <LicenseForm onActivated={onClose} />
    {#if error}<small class="error" role="alert">{error}</small>{/if}
    <div class="buttons">
      <button onclick={onClose}>Not now</button>
      {#if canBuy}<button class="primary" onclick={buy}>Buy Pro</button>{/if}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: grid;
    place-items: center;
    /* Over Settings and the other dialogs, which can open it. */
    z-index: 20;
  }
  .dialog {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: min(460px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow-y: auto;
    display: grid;
    gap: 12px;
    padding: 20px 24px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25);
  }
  .head {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .head :global(.icon) {
    color: var(--accent-ink);
    flex: none;
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  p {
    margin: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  li {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  li :global(.icon) {
    color: var(--accent-ink);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
</style>
