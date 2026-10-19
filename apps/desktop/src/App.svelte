<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let version = $state("…");

  invoke<string>("app_version")
    .then((v) => (version = v))
    .catch(() => (version = "unavailable outside the desktop app"));
</script>

<main>
  <h1>Novero Mail</h1>
  <p>Core version: {version}</p>
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: "Segoe UI", system-ui, sans-serif;
  }
  main {
    padding: 2rem;
  }
</style>
