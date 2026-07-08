<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let name = $state("");
  let greeting = $state("");

  async function greet(event: SubmitEvent) {
    event.preventDefault();
    greeting = await invoke<string>("greet", { name });
  }
</script>

<main>
  <h1>Flit</h1>

  <form onsubmit={greet}>
    <input placeholder="Enter a name…" bind:value={name} />
    <button type="submit">Greet</button>
  </form>

  {#if greeting}
    <p>{greeting}</p>
  {/if}
</main>

<style>
  main {
    max-width: 32rem;
    margin: 4rem auto;
    padding: 0 1rem;
    font-family: system-ui, sans-serif;
    text-align: center;
  }

  form {
    display: flex;
    gap: 0.5rem;
    justify-content: center;
  }

  input,
  button {
    padding: 0.5rem 0.75rem;
    font-size: 1rem;
    border: 1px solid #ccc;
    border-radius: 0.375rem;
  }

  button {
    cursor: pointer;
    background: #f5f5f5;
  }
</style>
