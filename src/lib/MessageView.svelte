<script lang="ts">
  import { listThread, setMessageRead } from "./api";
  import MessageCard from "./MessageCard.svelte";
  import type { MessageHeader } from "./types";

  let {
    message,
  }: {
    message: MessageHeader | null;
  } = $props();

  // The whole conversation of the selected row, oldest first. Falls back to
  // just the selected message so the viewer never comes up empty.
  let thread = $state<MessageHeader[]>([]);
  // Accordion: exactly one card open at a time. This keeps every mail body
  // in a full-height sandboxed iframe (hard rule: sandbox stays empty, so
  // the frame's height cannot be measured from outside).
  let expandedId = $state<number | null>(null);
  // why plain: only steers whether a reload resets the accordion — nothing
  // renders from it.
  let anchorId: number | null = null;

  $effect(() => {
    if (message === null) {
      thread = [];
      expandedId = null;
      anchorId = null;
      return;
    }
    const id = message.id;
    const fallback = message;
    listThread(id)
      .then((loaded) => {
        if (message?.id !== id) return;
        thread = loaded.length > 0 ? loaded : [fallback];
        // A fresh selection opens the newest message; refreshes of the same
        // conversation (new reply, read-flag sync) keep the user's spot.
        if (anchorId !== id) {
          anchorId = id;
          expand(thread[thread.length - 1]);
        }
      })
      .catch((err: unknown) => {
        console.error("failed to load thread:", err);
        if (message?.id === id) {
          thread = [fallback];
          if (anchorId !== id) {
            anchorId = id;
            expand(fallback);
          }
        }
      });
  });

  function expand(entry: MessageHeader) {
    expandedId = entry.id;
    // why: opening a message marks it read, exactly like selecting it in the
    // list — the backend clears the dot and pushes \Seen to the server.
    if (!entry.read) {
      void setMessageRead(entry.id, true).catch((err: unknown) =>
        console.error("failed to mark thread message read:", err),
      );
    }
  }
</script>

<article>
  {#if message === null}
    <p class="empty">Select a message</p>
  {:else}
    <div class="thread">
      {#each thread as entry (entry.id)}
        <div class="slot" class:grow={expandedId === entry.id}>
          <MessageCard
            message={entry}
            expanded={expandedId === entry.id}
            onExpand={() => expand(entry)}
          />
        </div>
      {/each}
    </div>
  {/if}
</article>

<style>
  article {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--bg-window);
  }

  .empty {
    margin: auto;
    color: var(--text-tertiary);
  }

  .thread {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    /* why: overflow-y auto alone computes overflow-x to auto — a squeezed
       pane must clip sideways, never grow a horizontal scrollbar. */
    overflow-x: hidden;
  }

  .slot {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
  }

  /* The open card takes all leftover height; with many collapsed rows the
     column scrolls, but the body always gets a readable minimum. */
  .slot.grow {
    flex: 1 0 auto;
    min-height: 420px;
  }
</style>
