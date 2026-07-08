<script lang="ts">
  import type { MessageHeader } from "./types";

  let { message }: { message: MessageHeader | null } = $props();
</script>

<article>
  {#if message === null}
    <p class="empty">Select a message</p>
  {:else}
    <header>
      <h2>{message.subject}</h2>
      <p class="meta">
        <span class="from">{message.from}</span>
        <span class="date">{message.date}</span>
      </p>
    </header>
    <!-- Phase 0: plain-text placeholder only. Real bodies arrive in Phase 2
         and MUST go through the sandboxed renderer (see Hard rules). -->
    <p class="body">{message.snippet}</p>
  {/if}
</article>

<style>
  article {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding: 1.25rem 1.5rem;
    overflow-y: auto;
  }

  .empty {
    margin: auto;
    color: #888;
  }

  h2 {
    margin: 0 0 0.375rem;
    font-size: 1.125rem;
  }

  .meta {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    margin: 0;
    font-size: 0.8125rem;
    color: #666;
  }

  .body {
    margin: 0;
    line-height: 1.5;
  }
</style>
