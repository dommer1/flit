<script lang="ts">
  import { listThread, setMessageRead, threadBodies } from "./api";
  import MessageCard from "./MessageCard.svelte";
  import type { MessageBody, MessageHeader } from "./types";

  let {
    message,
  }: {
    message: MessageHeader | null;
  } = $props();

  // The whole conversation of the selected row, oldest first. Falls back to
  // just the selected message so the viewer never comes up empty.
  let thread = $state<MessageHeader[]>([]);
  // Every message starts open (Canary-style stack); the user folds cards
  // they don't care about via the chevron.
  let collapsedIds = $state<Set<number>>(new Set());
  // Bodies arrive in one bulk call (single server connection for whatever
  // isn't cached), keyed by message id.
  let bodies = $state<Record<number, MessageBody>>({});
  let bodiesLoading = $state(false);
  let bodiesError = $state<string | null>(null);
  // why plain: only steers whether a reload resets the fold state — nothing
  // renders from it.
  let anchorId: number | null = null;

  $effect(() => {
    if (message === null) {
      thread = [];
      bodies = {};
      collapsedIds = new Set();
      anchorId = null;
      return;
    }
    const id = message.id;
    const fallback = message;
    listThread(id)
      .then((loaded) => {
        if (message?.id !== id) return;
        thread = loaded.length > 0 ? loaded : [fallback];
        if (anchorId !== id) {
          // A fresh selection: open everything, mark it read, load bodies.
          anchorId = id;
          collapsedIds = new Set();
          bodies = {};
          markThreadRead(thread);
          fetchBodies(id);
        } else if (thread.some((entry) => !(entry.id in bodies))) {
          // Same conversation refreshed and grew (a reply just synced in) —
          // top up the missing bodies without resetting the fold state.
          fetchBodies(id);
        }
      })
      .catch((err: unknown) => {
        console.error("failed to load thread:", err);
        if (message?.id === id) {
          thread = [fallback];
          if (anchorId !== id) {
            anchorId = id;
            collapsedIds = new Set();
            bodies = {};
            fetchBodies(id);
          }
        }
      });
  });

  function fetchBodies(anchor: number) {
    bodiesLoading = true;
    bodiesError = null;
    threadBodies(anchor)
      .then((loaded) => {
        if (anchorId === anchor) bodies = loaded;
      })
      .catch((err: unknown) => {
        if (anchorId === anchor) bodiesError = String(err);
      })
      .finally(() => {
        if (anchorId === anchor) bodiesLoading = false;
      });
  }

  // why: the whole conversation is visible at once, so opening it reads it —
  // same as Gmail. The backend clears the dots and pushes \Seen per message.
  function markThreadRead(entries: MessageHeader[]) {
    for (const entry of entries) {
      if (entry.read) continue;
      void setMessageRead(entry.id, true).catch((err: unknown) =>
        console.error("failed to mark thread message read:", err),
      );
    }
  }

  function toggle(entry: MessageHeader) {
    const next = new Set(collapsedIds);
    if (next.has(entry.id)) next.delete(entry.id);
    else next.add(entry.id);
    collapsedIds = next;
  }
</script>

<article>
  {#if message === null}
    <p class="empty">Select a message</p>
  {:else}
    <div class="thread">
      {#each thread as entry (entry.id)}
        <MessageCard
          message={entry}
          body={bodies[entry.id] ?? null}
          loading={bodiesLoading}
          error={bodiesError}
          collapsed={collapsedIds.has(entry.id)}
          single={thread.length <= 1}
          onToggle={() => toggle(entry)}
        />
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

  /* One scroll container for the whole stack — bodies are shown whole and
     auto-sized, the conversation scrolls as a single column. */
  .thread {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    /* why: overflow-y auto alone computes overflow-x to auto — a squeezed
       pane must clip sideways, never grow a horizontal scrollbar. */
    overflow-x: hidden;
  }
</style>
