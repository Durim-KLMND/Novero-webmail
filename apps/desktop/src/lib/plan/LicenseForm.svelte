<script lang="ts">
  // Where a license key from the purchase email is typed and activated.
  import { api, errorText, type Plan } from "../api";
  import { pro } from "./pro.svelte";

  interface Props {
    onActivated?: (plan: Plan) => void;
  }
  let { onActivated }: Props = $props();

  let key = $state("");
  let busy = $state(false);
  let error = $state("");

  async function activate(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = "";
    try {
      const plan = await api.activateLicense(key);
      pro.plan = plan;
      key = "";
      onActivated?.(plan);
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }
</script>

<form class="license" onsubmit={activate}>
  <label for="license-key">License key</label>
  <div class="row">
    <input
      id="license-key"
      bind:value={key}
      placeholder="From the email you got after buying"
      autocomplete="off"
      spellcheck="false"
      disabled={busy}
    />
    <button type="submit" disabled={busy || !key.trim()}>{busy ? "Activating…" : "Activate"}</button>
  </div>
  {#if error}<small class="error" role="alert">{error}</small>{/if}
</form>

<style>
  .license {
    display: grid;
    gap: 6px;
  }
  label {
    font-weight: 600;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .row input {
    flex: 1;
    min-width: 0;
    font-family: ui-monospace, Consolas, monospace;
  }
  .row button {
    white-space: nowrap;
  }
</style>
