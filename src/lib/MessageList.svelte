<script lang="ts">
  import { formatListDate, senderName } from "./format";
  import {
    accumulateOffset,
    actionFor,
    DEFAULT_SWIPE_ACTIONS,
    isHorizontal,
  } from "./swipe";
  import type { MessageHeader, SwipeAction, SwipeActions } from "./types";

  let {
    title,
    messages,
    accountColors = {},
    selectedId,
    onSelect,
    swipeActions = DEFAULT_SWIPE_ACTIONS,
    onArchive,
    onSetRead,
    isArchived,
    onTrash,
    onReply,
  }: {
    title: string;
    messages: MessageHeader[];
    /** accountId → accent color (or null); drives the per-row color dot. */
    accountColors?: Record<number, string | null>;
    selectedId: number | null;
    onSelect: (id: number) => void;
    /** Which action a full swipe in each direction fires. */
    swipeActions?: SwipeActions;
    onArchive?: (id: number) => void;
    onSetRead?: (id: number, read: boolean) => void;
    /** Row already sits in its archive folder — the swipe backdrop
     * reads "Move to Inbox" (onArchive still fires; the parent routes). */
    isArchived?: (message: MessageHeader) => boolean;
    onTrash?: (id: number) => void;
    onReply?: (id: number) => void;
  } = $props();

  /** Strip color + label for each swipe action ("none" never renders). */
  const SWIPE_STRIPS: Record<
    Exclude<SwipeAction, "none">,
    { color: string; label: (message: MessageHeader) => string }
  > = {
    archive: {
      color: "#4f7cf7",
      label: (m) => (isArchived?.(m) ? "Move to Inbox" : "Archive"),
    },
    toggleRead: {
      color: "#f0a132",
      label: (m) => (m.read ? "Mark Unread" : "Mark Read"),
    },
    trash: { color: "#e5484d", label: () => "Trash" },
    reply: { color: "#7a5af8", label: () => "Reply" },
  };

  // The row currently under a two-finger swipe and how far it has traveled.
  // One gesture at a time — trackpads can't swipe two rows at once.
  let swipeId = $state<number | null>(null);
  let swipeOffset = $state(0);
  let settleTimer: ReturnType<typeof setTimeout> | undefined;

  function handleWheel(message: MessageHeader, event: WheelEvent) {
    if (!isHorizontal(event.deltaX, event.deltaY)) return;
    event.preventDefault();
    if (swipeId !== message.id) {
      swipeId = message.id;
      swipeOffset = 0;
    }
    swipeOffset = accumulateOffset(swipeOffset, event.deltaX, swipeActions);
    // why a timer: DOM wheel streams have no "gesture ended" event — a quiet
    // gap longer than the ~10-20ms between trackpad pulses means release.
    clearTimeout(settleTimer);
    settleTimer = setTimeout(() => settleSwipe(message), 120);
  }

  function settleSwipe(message: MessageHeader) {
    if (swipeId !== message.id) return;
    const action = actionFor(swipeOffset, swipeActions);
    // Reset first so the row snaps back even when no action fired.
    swipeId = null;
    swipeOffset = 0;
    if (action === "archive") onArchive?.(message.id);
    else if (action === "toggleRead") onSetRead?.(message.id, !message.read);
    else if (action === "trash") onTrash?.(message.id);
    else if (action === "reply") onReply?.(message.id);
  }

  // why: keyboard navigation moves the selection without scrolling — keep the
  // selected row in view so arrowing through a long list stays usable.
  let listEl = $state<HTMLElement | null>(null);
  $effect(() => {
    if (selectedId === null || listEl === null) return;
    listEl
      .querySelector<HTMLElement>(".selected")
      ?.scrollIntoView({ block: "nearest" });
  });
</script>

<div class="pane">
  <header>
    <h1>{title}</h1>
    <p class="count">
      {messages.length}
      {messages.length === 1 ? "message" : "messages"}
    </p>
  </header>

  <div class="list" role="listbox" aria-label="Messages" bind:this={listEl}>
    {#if messages.length === 0}
      <p class="empty">No Messages</p>
    {:else}
      {#each messages as message (message.id)}
        {@const color = accountColors[message.accountId] ?? null}
        {@const offset = swipeId === message.id ? swipeOffset : 0}
        {@const stripAction =
          offset < 0
            ? swipeActions.left
            : offset > 0
              ? swipeActions.right
              : "none"}
        <div class="swipe-row" onwheel={(e) => handleWheel(message, e)}>
          <!-- why the width style: the backdrop spans exactly the revealed
               strip, so it can never show through the row content above it
               (hover tints the row translucent). -->
          {#if stripAction !== "none"}
            <span
              class="swipe-bg"
              class:trailing={offset < 0}
              class:leading={offset > 0}
              aria-hidden="true"
              style:width={`${Math.abs(offset)}px`}
              style:background={SWIPE_STRIPS[stripAction].color}
            >
              {SWIPE_STRIPS[stripAction].label(message)}
            </span>
          {/if}
          <button
            role="option"
            aria-selected={selectedId === message.id}
            class:selected={selectedId === message.id}
            class:unread={!message.read}
            class:swiping={offset !== 0}
            style:transform={offset === 0
              ? undefined
              : `translateX(${offset}px)`}
            onclick={() => onSelect(message.id)}
          >
            <span class="dot" aria-hidden="true"></span>
            <span class="content">
              <span class="row">
                <span class="from">{senderName(message.from)}</span>
                <span class="end">
                  {#if color}
                    <span
                      class="account-dot"
                      aria-hidden="true"
                      style:background={color}
                    ></span>
                  {/if}
                  <span class="date">{formatListDate(message.date)}</span>
                </span>
              </span>
              <span class="subject">{message.subject}</span>
              <span class="snippet">{message.snippet}</span>
            </span>
          </button>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-window);
  }

  header {
    flex-shrink: 0;
    padding: 12px 16px 8px;
    border-bottom: 1px solid var(--hairline);
  }

  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
  }

  .count {
    margin: 1px 0 0;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .list {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 4px 6px;
    overflow-y: auto;
  }

  .empty {
    margin: auto;
    color: var(--text-tertiary);
  }

  /* Each row: an absolutely-positioned action backdrop underneath, the row
     button sliding over it. Clip so a mid-swipe row can't poke into its
     neighbours. */
  .swipe-row {
    position: relative;
    overflow: hidden;
    /* why: overflow != visible drops a flex item's automatic minimum size
       to 0, so rows would squash vertically to fit the pane instead of
       scrolling — pin them at their content height. */
    flex-shrink: 0;
  }

  .swipe-bg {
    /* why border-box: width comes in as the revealed offset in px — the
       padding must eat into it, not widen the strip past the reveal. */
    box-sizing: border-box;
    position: absolute;
    top: 0;
    bottom: 0;
    display: flex;
    align-items: center;
    padding: 0 14px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 600;
    color: #ffffff;
    /* The label pins to the outer edge and clips while the strip is still
       narrow, instead of overflowing into the row. */
    overflow: hidden;
    white-space: nowrap;
  }

  /* Anchoring by direction; the action's color arrives as an inline style. */
  .swipe-bg.trailing {
    right: 0;
    justify-content: flex-end;
  }

  .swipe-bg.leading {
    left: 0;
    justify-content: flex-start;
  }

  .swipe-row button {
    /* why relative: keeps the button painting above the positioned swipe
       backdrop; doubles as the anchor for the ::before separator. */
    position: relative;
    display: flex;
    align-items: flex-start;
    width: 100%;
    padding: 7px 8px 7px 4px;
    border: none;
    border-radius: 7px;
    background: var(--bg-window);
    font: inherit;
    color: inherit;
    text-align: left;
    cursor: default;
    /* Snap-back after release; .swiping turns it off so the row follows
       the fingers with no lag. */
    transition: transform 0.18s ease;
  }

  .swipe-row button.swiping {
    transition: none;
  }

  /* Inset separators between rows, hidden around the selected one —
     the Apple Mail look. */
  .swipe-row + .swipe-row button::before {
    content: "";
    position: absolute;
    top: -1px;
    left: 20px;
    right: 8px;
    height: 1px;
    background: var(--hairline);
  }

  .swipe-row:has(button.selected) + .swipe-row button::before,
  button.selected::before {
    background: transparent;
  }

  /* Hover on any row that isn't the selected one — the selected row keeps
     its accent fill. */
  button:not(.selected):hover {
    background: var(--bg-hover);
  }

  button.selected {
    background: var(--accent);
    color: var(--accent-text);
  }

  .dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    margin: 5px 4px 0 0;
    border-radius: 50%;
  }

  .unread .dot {
    background: var(--accent);
  }

  .selected.unread .dot {
    background: var(--accent-text);
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    flex: 1;
  }

  .row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
  }

  .from {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Right end of the top row: a small account-color dot next to the date. */
  .end {
    display: flex;
    align-items: center;
    gap: 5px;
    flex-shrink: 0;
  }

  .account-dot {
    width: 6px;
    height: 6px;
    flex-shrink: 0;
    border-radius: 50%;
  }

  .date {
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .selected .date {
    color: var(--accent-text);
    opacity: 0.85;
  }

  .subject {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .snippet {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    font-size: 12px;
    line-height: 1.3;
    color: var(--text-secondary);
  }

  .selected .snippet {
    color: var(--accent-text);
    opacity: 0.85;
  }
</style>
