<script lang="ts">
  import { listThread, setMessageRead, threadBodies } from "./api";
  import type { DraftKind } from "./draft";
  import MessageCard from "./MessageCard.svelte";
  import type { MessageBody, MessageHeader, ThreadOrder } from "./types";

  let {
    message,
    accountEmails = {},
    accountColors = {},
    threadOrder = "newestLast",
    onDraft,
    onEditDraft,
  }: {
    message: MessageHeader | null;
    /** accountId → address; marks a card's sender as "me". */
    accountEmails?: Record<number, string>;
    /** accountId → accent color for the "me" avatar. */
    accountColors?: Record<number, string | null>;
    /** Which end of the conversation the newest message renders at. */
    threadOrder?: ThreadOrder;
    /** Per-message reply actions in card footers. */
    onDraft?: (kind: DraftKind, message: MessageHeader) => void;
    /** A clicked draft card resumes editing in a compose window. */
    onEditDraft?: (id: number) => void;
  } = $props();

  // The whole conversation of the selected row, oldest first. Falls back to
  // just the selected message so the viewer never comes up empty.
  let thread = $state<MessageHeader[]>([]);
  // Like the design: the newest message starts open, older ones collapse to
  // a preview row; any number can be open at once.
  let expandedIds = $state<Set<number>>(new Set());
  // Bodies arrive in one bulk call (single server connection for whatever
  // isn't cached), keyed by message id.
  let bodies = $state<Record<number, MessageBody>>({});
  let bodiesLoading = $state(false);
  let bodiesError = $state<string | null>(null);
  // why plain: only steers whether a reload resets the open state — nothing
  // renders from it.
  let anchorId: number | null = null;

  $effect(() => {
    if (message === null) {
      thread = [];
      bodies = {};
      expandedIds = new Set();
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
          // A fresh selection: open the newest message, load all bodies.
          anchorId = id;
          bodies = {};
          openMessage(newestMessage(thread));
          fetchBodies(id);
        } else if (thread.some((entry) => !(entry.id in bodies))) {
          // Same conversation refreshed and grew (a reply just synced in) —
          // top up the missing bodies without resetting the open state.
          fetchBodies(id);
        }
      })
      .catch((err: unknown) => {
        console.error("failed to load thread:", err);
        if (message?.id === id) {
          thread = [fallback];
          if (anchorId !== id) {
            anchorId = id;
            bodies = {};
            openMessage(fallback);
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

  /** Open a card (fresh selections replace the set), marking it read —
   * same as opening it from the list. */
  function openMessage(entry: MessageHeader) {
    expandedIds = new Set([entry.id]);
    markRead(entry);
  }

  function markRead(entry: MessageHeader) {
    if (entry.read) return;
    void setMessageRead(entry.id, true).catch((err: unknown) =>
      console.error("failed to mark thread message read:", err),
    );
  }

  /** The newest real message — a trailing draft never auto-opens (it is
   * edited in a compose window, never read inline). Falls back to the last
   * entry so a draft-only thread still highlights something. */
  function newestMessage(entries: MessageHeader[]): MessageHeader {
    return (
      [...entries].reverse().find((entry) => !entry.isDraft) ??
      entries[entries.length - 1]
    );
  }

  // Display order only — `thread` stays oldest-first everywhere else
  // (newest lookup, default-open logic).
  let displayThread = $derived(
    threadOrder === "newestFirst" ? [...thread].reverse() : thread,
  );
  let newestId = $derived(
    thread.length > 0 ? newestMessage(thread).id : null,
  );
  // What the header counts: sent/received messages; a draft is not one yet.
  let messageCount = $derived(
    thread.filter((entry) => !entry.isDraft).length,
  );

  function toggle(entry: MessageHeader) {
    // Drafts have no reading view — the click resumes editing instead.
    if (entry.isDraft) {
      onEditDraft?.(entry.id);
      return;
    }
    const next = new Set(expandedIds);
    if (next.has(entry.id)) {
      next.delete(entry.id);
    } else {
      next.add(entry.id);
      markRead(entry);
    }
    expandedIds = next;
  }
</script>

<article>
  {#if message === null}
    <p class="empty">Select a message</p>
  {:else}
    <div class="thread">
      <div class="stack">
        <div class="thread-head">
          <h2 class="subject">{message.subject}</h2>
          <span class="count">
            {messageCount}
            {messageCount === 1 ? "message" : "messages"}
          </span>
        </div>
        {#each displayThread as entry (entry.id)}
          <MessageCard
            message={entry}
            body={bodies[entry.id] ?? null}
            loading={bodiesLoading}
            error={bodiesError}
            expanded={expandedIds.has(entry.id)}
            last={entry.id === newestId}
            ownEmail={accountEmails[entry.accountId] ?? null}
            accountColor={accountColors[entry.accountId] ?? null}
            onToggle={() => toggle(entry)}
            onDraft={(kind) => onDraft?.(kind, entry)}
          />
        {/each}
      </div>
    </div>
  {/if}
</article>

<style>
  article {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--bg-thread);
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
    padding: 18px 22px;
    overflow-y: auto;
    /* why: overflow-y auto alone computes overflow-x to auto — a squeezed
       pane must clip sideways, never grow a horizontal scrollbar. */
    overflow-x: hidden;
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 860px;
    margin: 0 auto;
  }

  .thread-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 4px;
  }

  .subject {
    overflow: hidden;
    margin: 0;
    font-size: 17px;
    font-weight: 700;
    color: var(--text-primary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-secondary);
  }
</style>
