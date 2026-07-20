<script lang="ts">
  import { formatListDate, senderName } from "./format";
  import {
    accumulateOffset,
    actionFor,
    clampOffset,
    DEFAULT_SWIPE_ACTIONS,
    DRAG_SLOP,
    isHorizontal,
  } from "./swipe";
  import type {
    MessageHeader,
    SwipeAction,
    SwipeActions,
    ViewStatus,
  } from "./types";

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
    status = null,
    onLoadMore,
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
    /** Whole-view totals + backfill progress; null (e.g. search results)
     * falls back to counting the rows at hand. */
    status?: ViewStatus | null;
    /** Ask the parent to reveal more rows — fired near the list's bottom
     * while more rows exist than are loaded. */
    onLoadMore?: () => void;
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

  // Mouse/touch drag: pointerdown only arms a candidate — the swipe starts
  // once the pointer travels past DRAG_SLOP with horizontal dominating, so a
  // plain click still selects and a vertical move never grabs the row.
  let dragId: number | null = null;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragActive = false;
  let dragConsumedClick = false;

  function handlePointerDown(message: MessageHeader, event: PointerEvent) {
    if (event.button !== 0) return;
    dragId = message.id;
    dragStartX = event.clientX;
    dragStartY = event.clientY;
    dragActive = false;
  }

  function handlePointerMove(message: MessageHeader, event: PointerEvent) {
    if (dragId !== message.id) return;
    const dx = event.clientX - dragStartX;
    const dy = event.clientY - dragStartY;
    if (!dragActive) {
      if (Math.abs(dx) <= DRAG_SLOP || !isHorizontal(dx, dy)) return;
      dragActive = true;
      // why capture: move/up events keep hitting this row even when the
      // pointer leaves it mid-drag. Optional call — jsdom lacks it.
      (event.currentTarget as HTMLElement).setPointerCapture?.(
        event.pointerId,
      );
      // why: a pending wheel settle would reset the row mid-drag.
      clearTimeout(settleTimer);
      swipeId = message.id;
    }
    swipeOffset = clampOffset(dx, swipeActions);
  }

  function handlePointerUp(message: MessageHeader) {
    if (dragId !== message.id) return;
    dragId = null;
    if (!dragActive) return;
    dragActive = false;
    // why: the browser fires a click right after this pointerup — swallow
    // it so releasing a swipe never also selects the row.
    dragConsumedClick = true;
    // pointerup IS the release — settle now, no quiet-gap timer needed.
    settleSwipe(message);
  }

  function handlePointerCancel(message: MessageHeader) {
    if (dragId !== message.id) return;
    // The system took the pointer (e.g. a native gesture) — snap back.
    dragId = null;
    if (!dragActive) return;
    dragActive = false;
    swipeId = null;
    swipeOffset = 0;
  }

  function handleClick(message: MessageHeader) {
    if (dragConsumedClick) {
      dragConsumedClick = false;
      return;
    }
    onSelect(message.id);
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

  let unreadCount = $derived(
    status?.unread ?? messages.filter((m) => !m.read).length,
  );
  let totalRows = $derived(status?.listRows ?? messages.length);
  let hasMore = $derived(status !== null && messages.length < status.listRows);
  // Backfill progress, shown only while the server holds more than the
  // cache; disappears on its own once the mailbox is fully mirrored.
  let syncing = $derived(
    status !== null &&
      status.serverTotal !== null &&
      status.cached < status.serverTotal
      ? { cached: status.cached, total: status.serverTotal }
      : null,
  );

  /** How close to the bottom (px) the scroll gets before asking for more. */
  const LOAD_MORE_THRESHOLD = 300;

  function handleScroll() {
    if (!hasMore || !onLoadMore || listEl === null) return;
    const remaining =
      listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight;
    // Duplicate fires are fine — the parent's reveal logic is idempotent
    // until the longer list actually arrives.
    if (remaining < LOAD_MORE_THRESHOLD) onLoadMore();
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
      {totalRows}
      {totalRows === 1 ? "message" : "messages"}{unreadCount > 0
        ? `, ${unreadCount} unread`
        : ""}
    </p>
  </header>

  <div
    class="list"
    role="listbox"
    aria-label="Messages"
    bind:this={listEl}
    onscroll={handleScroll}
  >
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
        <!-- svelte-ignore a11y_no_static_element_interactions
             — the pointer handlers implement the swipe gesture; keyboard
             users act on rows through the buttons and shortcuts instead. -->
        <div
          class="swipe-row"
          onwheel={(e) => handleWheel(message, e)}
          onpointerdown={(e) => handlePointerDown(message, e)}
          onpointermove={(e) => handlePointerMove(message, e)}
          onpointerup={() => handlePointerUp(message)}
          onpointercancel={() => handlePointerCancel(message)}
        >
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
            onclick={() => handleClick(message)}
          >
            <span class="content">
              <span class="row">
                {#if !message.read || message.threadUnread}
                  <span class="dot" aria-hidden="true"></span>
                {/if}
                <span class="from">{senderName(message.from)}</span>
                <span class="end">
                  {#if message.hasAttachments}
                    <svg
                      class="clip"
                      viewBox="0 0 20 20"
                      width="12"
                      height="12"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.7"
                      aria-hidden="true"
                    >
                      <title>Has attachments</title>
                      <path
                        d="M15.5 9.5 9.9 15a3.5 3.5 0 0 1-5-5l6.4-6.3a2.3 2.3 0 0 1 3.3 3.3L8.3 13.2a1.2 1.2 0 0 1-1.7-1.7l5.2-5.1"
                      />
                    </svg>
                  {/if}
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

  {#if syncing}
    <footer class="syncing" aria-live="polite">
      <span class="syncing-row">
        <span>Syncing older messages…</span>
        <span>{syncing.cached.toLocaleString()} of {syncing.total.toLocaleString()}</span>
      </span>
      <span class="syncing-bar" aria-hidden="true">
        <span
          class="syncing-fill"
          style:width={`${Math.min(100, (syncing.cached / syncing.total) * 100)}%`}
        ></span>
      </span>
    </footer>
  {/if}
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
    overflow-y: auto;
    /* why: overflow-y auto alone computes overflow-x to auto — the list
       must clip sideways, never scroll (swipes translate rows, not panes). */
    overflow-x: hidden;
  }

  .empty {
    margin: auto;
    color: var(--text-tertiary);
  }

  /* Backfill progress: a quiet strip pinned under the list, gone once the
     mailbox is fully mirrored. */
  .syncing {
    flex-shrink: 0;
    padding: 6px 16px 8px;
    border-top: 1px solid var(--hairline);
    font-size: 11px;
    color: var(--text-secondary);
  }

  .syncing-row {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }

  .syncing-bar {
    display: block;
    margin-top: 5px;
    height: 2px;
    border-radius: 1px;
    background: var(--hairline);
    overflow: hidden;
  }

  .syncing-fill {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
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
    /* Flat full-width separators between rows (the new design). */
    border-bottom: 1px solid var(--hairline);
    /* why pan-y: on touch, vertical drags keep scrolling the list while
       horizontal ones reach the pointer handlers as a swipe. */
    touch-action: pan-y;
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
       backdrop. */
    position: relative;
    display: block;
    box-sizing: border-box;
    width: 100%;
    padding: 10px 16px;
    border: none;
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

  /* Hover on any row that isn't the selected one — the selected row keeps
     its accent fill. */
  button:not(.selected):hover {
    background: var(--bg-hover);
  }

  button.selected {
    background: var(--accent);
    color: var(--accent-text);
  }

  /* Unread marker, inline before the sender name. */
  .dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }

  .selected .dot {
    background: var(--accent-text);
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .from {
    flex: 1;
    overflow: hidden;
    font-weight: 700;
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
    width: 7px;
    height: 7px;
    flex-shrink: 0;
    border-radius: 50%;
  }

  .end .clip {
    flex-shrink: 0;
    color: var(--text-secondary);
  }

  .selected .end .clip {
    color: var(--accent-text);
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
    font-size: 12.5px;
    font-weight: 500;
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
