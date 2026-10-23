<script lang="ts">
  // Settings > Plan: which plan this copy is on, buying Pro and its license key.
  import { api, errorText } from "../api";
  import Icon from "../ui/Icon.svelte";
  import LicenseForm from "./LicenseForm.svelte";
  import { planLine, pro } from "./pro.svelte";

  const plan = $derived(pro.plan);
  const license = $derived(plan?.license ?? null);
  const unlocked = $derived(plan?.reason === "unlocked");
  /** A key that works on this version. */
  const licensed = $derived(plan?.reason === "license");

  let busy = $state(false);
  let note = $state("");
  let failed = $state(false);

  const day = (seconds: number) =>
    new Date(seconds * 1000).toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric" });

  async function run(action: () => Promise<unknown>, done: string) {
    busy = true;
    note = "";
    failed = false;
    try {
      await action();
      note = done;
    } catch (err) {
      failed = true;
      note = errorText(err);
    } finally {
      busy = false;
    }
  }

  const buy = () => run(() => api.openCheckout(), "The checkout opened in your browser. Your key arrives by email.");
  const check = () => run(async () => (pro.plan = await api.refreshPlan()), "Checked just now.");
  const remove = () =>
    run(async () => (pro.plan = await api.deactivateLicense()), "The key was removed. You can use it on another computer.");

  const features: { name: string; free: string; pro: string }[] = [
    { name: "Mail accounts", free: "2", pro: "Unlimited" },
    { name: "Reading, writing, search and folders", free: "Yes", pro: "Yes" },
    { name: "Calendar on this computer", free: "Yes", pro: "Yes" },
    { name: "Google and Outlook calendar sync", free: "", pro: "Yes" },
    { name: "Rules", free: "", pro: "Yes" },
    { name: "Send later and snooze", free: "", pro: "Yes" },
    { name: "Templates", free: "", pro: "Yes" },
    { name: "Signatures per account", free: "1", pro: "Unlimited" },
  ];
</script>

{#if !plan}
  <span class="spinner"></span>
{:else}
  <div class="status" class:pro={plan.tier === "pro"}>
    <Icon name={plan.tier === "pro" ? "pro" : "key"} size={20} />
    <div>
      <strong>{planLine(plan)}</strong>
      {#if plan.reason === "trial"}
        <small class="quiet">Every Pro feature is on until the trial ends. After that, everything in Free keeps working.</small>
      {:else if licensed && license}
        <small class="quiet">
          Key {license.key}{license.variant ? ` · ${license.variant}` : ""}.
          {#if license.updatesUntil}Includes versions released until {day(license.updatesUntil)}.{/if}
        </small>
      {:else if plan.reason === "unchecked"}
        <small class="quiet">Novero Mail couldn't reach the store for two weeks. Connect to the internet and check again.</small>
      {:else if plan.reason === "ended" && license}
        <small class="quiet">The store reports this key as {license.status}. Check the email from your purchase, or buy a new one.</small>
      {:else if plan.reason === "tooNew" && license?.updatesUntil}
        <small class="quiet">
          Your key covers versions released until {day(license.updatesUntil)}. Buy a new key for this version and a
          year of updates, or keep using an older version with Pro.
        </small>
      {:else if !unlocked}
        <small class="quiet">Upgrade once to unlock everything below. No subscription.</small>
      {/if}
      {#if plan.pausedAccounts.length}
        <small class="warn">
          {plan.pausedAccounts.length === 1 ? "One account isn't" : `${plan.pausedAccounts.length} accounts aren't`} syncing,
          since Free includes {plan.freeAccounts}. Its mail stays readable here.
        </small>
      {/if}
    </div>
  </div>

  {#if !unlocked}
    <div class="row">
      {#if !licensed && plan.canBuy}
        <button class="primary" onclick={buy} disabled={busy}><Icon name="pro" /> Buy Novero Mail Pro</button>
      {/if}
      {#if license}
        <button onclick={check} disabled={busy}><Icon name="refresh" /> Check license</button>
        <button onclick={remove} disabled={busy}>Remove key from this computer</button>
      {/if}
    </div>
    {#if note}<small class:error={failed} class:quiet={!failed} role="status">{note}</small>{/if}
    {#if !licensed}
      <div class="key"><LicenseForm /></div>
    {/if}
  {/if}

  <table>
    <thead>
      <tr><th></th><th>Free</th><th>Pro</th></tr>
    </thead>
    <tbody>
      {#each features as f (f.name)}
        <tr>
          <td>{f.name}</td>
          <td>{#if f.free === "Yes"}<Icon name="check" label="Included" />{:else}{f.free}{/if}</td>
          <td>{#if f.pro === "Yes"}<Icon name="check" label="Included" />{:else}{f.pro}{/if}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if !unlocked}
    <small class="quiet">
      Pro is a one-time purchase that is yours to keep, with a year of new versions included. One key works on up to
      three computers.
    </small>
  {/if}
{/if}

<style>
  .status {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .status > div {
    display: grid;
    gap: 4px;
  }
  .status :global(.icon) {
    margin-top: 1px;
    color: var(--text-quiet);
    flex: none;
  }
  .status.pro :global(.icon) {
    color: var(--accent-ink);
  }
  .warn {
    color: var(--danger);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin: 12px 0 6px;
  }
  .row button {
    white-space: nowrap;
  }
  .key {
    margin: 14px 0 6px;
    max-width: 520px;
  }
  table {
    width: 100%;
    max-width: 520px;
    border-collapse: collapse;
    margin: 18px 0 8px;
  }
  th,
  td {
    text-align: left;
    padding: 7px 8px;
    border-bottom: 1px solid var(--border);
  }
  th:not(:first-child),
  td:not(:first-child) {
    text-align: center;
    width: 90px;
  }
  td :global(.icon) {
    color: var(--accent-ink);
    vertical-align: middle;
  }
  small {
    display: block;
  }
</style>
