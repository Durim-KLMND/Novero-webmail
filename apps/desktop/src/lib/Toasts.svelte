<script lang="ts">
  import { dismiss, toasts } from "./toasts.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each toasts as t (t.id)}
    <div class="toast" class:error={t.kind === "error"} role={t.kind === "error" ? "alert" : "status"}>
      <span>{t.text}</span>
      <button class="link" onclick={() => dismiss(t.id)} aria-label="Dismiss">✕</button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
    z-index: 20;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    max-width: 420px;
    display: flex;
    gap: 12px;
    align-items: center;
    background: var(--bg-side);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 14px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.18);
    animation: rise 0.15s ease-out;
  }
  .toast.error {
    background: var(--danger-bg);
    border-color: var(--danger-bg);
    color: var(--danger);
  }
  .toast.error .link {
    color: var(--danger);
  }
  @keyframes rise {
    from {
      transform: translateY(8px);
      opacity: 0;
    }
  }
</style>
