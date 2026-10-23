<script lang="ts">
  import { onDestroy } from "svelte";
  import type { Account } from "./api";
  import { errorText, isOffline, isSignInError } from "./api";
  import Icon from "./Icon.svelte";
  import type { SyncState } from "./Sidebar.svelte";

  // A thin bar along the bottom of the window. The left says how syncing is
  // going, the right whether every account can reach its mail servers. It
  // keeps the same height whatever it says, so nothing above it moves.

  interface Props {
    accounts: Account[];
    syncState: Record<number, SyncState>;
  }
  let { accounts, syncState }: Props = $props();

  let networkUp = $state(navigator.onLine);
  let open = $state(false);
  let root = $state<HTMLElement>();

  // Most syncs take a moment, so "Checking" shows only once one has run a
  // second, which keeps the bar from flickering on every new message.
  let checking = $state(false);
  let checkTimer: ReturnType<typeof setTimeout> | undefined;
  const running = $derived(accounts.some((a) => syncState[a.id]?.running));
  $effect(() => {
    clearTimeout(checkTimer);
    if (!running) checking = false;
    else if (!checking) checkTimer = setTimeout(() => (checking = true), 1000);
  });
  onDestroy(() => clearTimeout(checkTimer));

  type Health = "ok" | "offline" | "error";

  /** Incoming mail (IMAP): the last sync, and the last push of local changes. */
  function incoming(sync: SyncState | undefined): { health: Health; text: string } {
    const error = sync?.error ?? sync?.pendingError ?? null;
    if (!networkUp || isOffline(error)) return { health: "offline", text: "Can't reach the server" };
    if (isSignInError(error)) return { health: "error", text: "Needs you to sign in again" };
    if (error) return { health: "error", text: errorText(error) };
    if (sync?.running) return { health: "ok", text: "Checking for mail" };
    return { health: "ok", text: "Connected" };
  }

  /** Outgoing mail (SMTP): only known once something has been sent. */
  function outgoing(sync: SyncState | undefined): { health: Health; text: string } {
    const error = sync?.outboxError ?? null;
    if (!networkUp || isOffline(error)) return { health: "offline", text: "Can't reach the server" };
    if (isSignInError(error)) return { health: "error", text: "Needs you to sign in again" };
    if (error) return { health: "error", text: errorText(error) };
    if (sync?.queued) return { health: "ok", text: "Sending" };
    return { health: "ok", text: "Ready" };
  }

  const rows = $derived(
    accounts.map((a) => ({ account: a, imap: incoming(syncState[a.id]), smtp: outgoing(syncState[a.id]) })),
  );

  /** Errors outrank offline, which outranks fine. */
  const health = $derived.by((): Health => {
    const all = rows.flatMap((r) => [r.imap.health, r.smtp.health]);
    if (all.includes("error")) return "error";
    if (all.includes("offline")) return "offline";
    return "ok";
  });

  const pending = $derived(accounts.reduce((n, a) => n + (syncState[a.id]?.pending ?? 0), 0));
  const synced = $derived(Math.max(0, ...accounts.map((a) => syncState[a.id]?.synced ?? 0)) || null);

  function changes(n: number): string {
    return n === 1 ? "1 change" : `${n.toLocaleString()} changes`;
  }
  function time(ms: number): string {
    return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  function outside(event: PointerEvent) {
    if (open && root && !root.contains(event.target as Node)) open = false;
  }
  function key(event: KeyboardEvent) {
    if (open && event.key === "Escape") {
      open = false;
      event.stopPropagation();
    }
  }
</script>

<svelte:window
  ononline={() => (networkUp = true)}
  onoffline={() => (networkUp = false)}
  onpointerdown={outside}
  onkeydown={key}
/>

<footer class="status" bind:this={root}>
  <span class="sync quiet" role="status">
    {#if pending > 0}
      {changes(pending)} waiting to sync{health === "offline" ? ", sent when back online" : ""}
    {:else if checking}
      Checking for new mail…
    {:else if synced}
      <span title="Last checked at {time(synced)}">Up to date</span>
    {/if}
  </span>
  <button
    class="health {health}"
    aria-haspopup="dialog"
    aria-expanded={open}
    onclick={() => (open = !open)}
    title="Connection to your mail servers"
  >
    {#if health === "ok"}
      <Icon name="check" size={14} /> Online
    {:else if health === "offline"}
      <Icon name="offline" size={14} /> Offline
    {:else}
      <Icon name="alert" size={14} /> Connection problem
    {/if}
  </button>
  {#if open}
    <div class="panel" role="dialog" aria-label="Connections">
      {#each rows as { account, imap, smtp } (account.id)}
        <section>
          <strong class="selectable">{account.email}</strong>
          <dl>
            <dt>Incoming</dt>
            <dd class="is-{imap.health}">{imap.text}</dd>
            <dt>Outgoing</dt>
            <dd class="is-{smtp.health}">{smtp.text}</dd>
          </dl>
        </section>
      {/each}
    </div>
  {/if}
</footer>

<style>
  .status {
    grid-area: status;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    height: 24px;
    padding: 0 6px 0 12px;
    background: var(--bg-side);
    border-top: 1px solid var(--border);
    font-size: 12px;
    min-width: 0;
  }
  .sync {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .health {
    display: flex;
    align-items: center;
    gap: 5px;
    flex: none;
    border: none;
    background: none;
    padding: 2px 6px;
    border-radius: 4px;
    font: inherit;
    color: var(--text-quiet);
  }
  .health:hover {
    background: var(--bg-hover);
  }
  .health.ok :global(.icon) {
    color: var(--success);
  }
  .health.error {
    color: var(--danger);
  }
  .panel {
    position: absolute;
    right: 6px;
    bottom: 28px;
    z-index: 15;
    width: 300px;
    max-height: 60vh;
    overflow: auto;
    padding: 6px 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgb(0 0 0 / 18%);
  }
  section {
    padding: 6px 0;
  }
  section + section {
    border-top: 1px solid var(--border);
  }
  strong {
    display: block;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 10px;
    margin: 4px 0 0;
  }
  dt {
    color: var(--text-quiet);
  }
  dd {
    margin: 0;
  }
  dd.is-ok {
    color: var(--success);
  }
  dd.is-offline {
    color: var(--text-quiet);
  }
  dd.is-error {
    color: var(--danger);
  }
</style>
