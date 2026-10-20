<script lang="ts">
  // A draft in a window of its own ("New message in a new window"). The main
  // window learns how it ended from the "composer-closed" event.
  import { onMount } from "svelte";
  import { emit } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api, errorText, type Account, type ComposerOutcome, type Draft } from "./api";
  import Composer from "./Composer.svelte";
  import TitleBar from "./TitleBar.svelte";
  import Toasts from "./Toasts.svelte";
  import { applyTheme } from "./theme";

  interface Props {
    id: number;
  }
  let { id }: Props = $props();

  let draft = $state<Draft | null>(null);
  let accounts = $state<Account[]>([]);
  let error = $state("");
  let composer = $state<Composer>();
  let done = false;

  async function finish(outcome: ComposerOutcome) {
    if (done) return;
    done = true;
    if (draft) await emit("composer-closed", { account: draft.account, outcome }).catch(() => {});
    await getCurrentWindow().destroy();
  }

  onMount(() => {
    (async () => {
      try {
        const [settings, list, found] = await Promise.all([api.getSettings(), api.listAccounts(), api.getDraft(id)]);
        applyTheme(settings.theme);
        accounts = list;
        draft = found;
      } catch (err) {
        error = errorText(err);
      }
    })();
    // The window's own close button saves what was typed, like Close does.
    const stop = getCurrentWindow().onCloseRequested(async (event) => {
      if (done) return;
      event.preventDefault();
      await composer?.flush().catch(() => {});
      await finish("saved");
    });
    return () => stop.then((unlisten) => unlisten());
  });
</script>

<main>
  {#if error}
    <p class="error">{error}</p>
  {:else if draft}
    {#key draft.id}
      <Composer bind:this={composer} {draft} {accounts} inWindow onClose={finish} onReplace={(next) => (draft = next)} />
    {/key}
  {:else}
    <div class="center"><span class="spinner"></span></div>
  {/if}
</main>
<TitleBar title={draft?.subject.trim() || "New message"} />
<Toasts />

<style>
  main {
    height: 100%;
  }
  .center {
    height: 100%;
    display: grid;
    place-items: center;
  }
  .error {
    margin: 16px;
  }
</style>
