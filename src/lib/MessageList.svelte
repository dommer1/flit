<script lang="ts">
  import type { MessageHeader } from "./types";

  let {
    messages,
    selectedId,
    onSelect,
  }: {
    messages: MessageHeader[];
    selectedId: number | null;
    onSelect: (id: number) => void;
  } = $props();
</script>

<div class="list" role="listbox" aria-label="Messages">
  {#if messages.length === 0}
    <p class="empty">No messages</p>
  {:else}
    {#each messages as message (message.id)}
      <button
        role="option"
        aria-selected={selectedId === message.id}
        class:selected={selectedId === message.id}
        class:unread={!message.read}
        onclick={() => onSelect(message.id)}
      >
        <span class="row">
          <span class="from">{message.from}</span>
          <span class="date">{message.date.slice(0, 10)}</span>
        </span>
        <span class="subject">{message.subject}</span>
        <span class="snippet">{message.snippet}</span>
      </button>
    {/each}
  {/if}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    overflow-y: auto;
  }

  .empty {
    padding: 1rem;
    color: #888;
    text-align: center;
  }

  button {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    padding: 0.625rem 0.75rem;
    border: none;
    border-bottom: 1px solid #eee;
    background: none;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  button:hover {
    background: rgba(0, 0, 0, 0.03);
  }

  button.selected {
    background: rgba(0, 0, 0, 0.07);
  }

  .row {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    width: 100%;
  }

  .from {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .unread .from,
  .unread .subject {
    font-weight: 600;
  }

  .date {
    flex-shrink: 0;
    font-size: 0.75rem;
    color: #888;
  }

  .subject {
    font-size: 0.875rem;
  }

  .snippet {
    overflow: hidden;
    font-size: 0.8125rem;
    color: #777;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
</style>
