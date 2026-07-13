<script module lang="ts">
  /** One send in flight, mirrored from the backend's send-* events. */
  export interface OutboxEntry {
    id: number;
    subject: string;
    status: "sending" | "sent" | "failed";
    error: string | null;
  }
</script>

<script lang="ts">
  let {
    entries,
    onUndo,
  }: {
    entries: OutboxEntry[];
    onUndo: (id: number) => void;
  } = $props();

  function title(entry: OutboxEntry): string {
    const subject = entry.subject.trim();
    return subject === "" ? "(No subject)" : subject;
  }

  function label(entry: OutboxEntry): string {
    if (entry.status === "sending") return `Sending: ${title(entry)}`;
    if (entry.status === "sent") return `Sent: ${title(entry)}`;
    return `Couldn't send: ${title(entry)}`;
  }
</script>

{#if entries.length > 0}
  <!-- Floating badge stack at the bottom of the message list, one pill per
       send in flight — Gmail's "Sending… / Undo" pattern. -->
  <div class="outbox">
    {#each entries as entry (entry.id)}
      <div class="badge" class:failed={entry.status === "failed"} role="status">
        <span class="dot {entry.status}" aria-hidden="true"></span>
        <span class="text">
          {label(entry)}
          {#if entry.status === "failed" && entry.error}
            <span class="detail">{entry.error}</span>
          {/if}
        </span>
        {#if entry.status === "sending"}
          <button type="button" onclick={() => onUndo(entry.id)}>Undo</button>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .outbox {
    position: absolute;
    bottom: 12px;
    left: 12px;
    right: 12px;
    z-index: 5;
    display: flex;
    flex-direction: column;
    gap: 6px;
    pointer-events: none;
  }

  .badge {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: 10px;
    background: var(--bg-sidebar);
    box-shadow:
      0 0 0 1px var(--hairline),
      0 4px 16px rgba(0, 0, 0, 0.18);
    font-size: 12px;
    color: var(--text-primary);
    pointer-events: auto;
  }

  .dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .dot.sending {
    background: var(--accent);
    /* A quiet pulse says "still cancellable" without a spinner. */
    animation: pulse 1.2s ease-in-out infinite;
  }

  .dot.sent {
    background: #28a745;
  }

  .dot.failed {
    background: #d9302c;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail {
    margin-left: 6px;
    color: var(--text-secondary);
  }

  .badge.failed .text {
    color: #d9302c;
  }

  .badge.failed .detail {
    color: var(--text-secondary);
  }

  button {
    flex-shrink: 0;
    padding: 2px 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
    cursor: pointer;
  }

  button:hover {
    background: var(--bg-hover);
  }
</style>
