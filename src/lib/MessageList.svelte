<script lang="ts">
  import { formatListDate, senderName } from "./format";
  import { accumulateOffset, actionFor, isHorizontal } from "./swipe";
  import type { MessageHeader } from "./types";

  let {
    title,
    messages,
    accountColors = {},
    selectedId,
    onSelect,
    onCompose,
    onSearch,
    onToggleSidebar,
    onArchive,
    onSetRead,
  }: {
    title: string;
    messages: MessageHeader[];
    /** accountId → accent color (or null); drives the per-row color dot. */
    accountColors?: Record<number, string | null>;
    selectedId: number | null;
    onSelect: (id: number) => void;
    onCompose: () => void;
    onSearch: (query: string) => void;
    onToggleSidebar?: () => void;
    /** Fired by a full swipe left on a row (Apple Mail's archive gesture). */
    onArchive?: (id: number) => void;
    /** Fired by a full swipe right on a row: toggle read/unread. */
    onSetRead?: (id: number, read: boolean) => void;
  } = $props();

  let query = $state("");

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
    swipeOffset = accumulateOffset(swipeOffset, event.deltaX);
    // why a timer: DOM wheel streams have no "gesture ended" event — a quiet
    // gap longer than the ~10-20ms between trackpad pulses means release.
    clearTimeout(settleTimer);
    settleTimer = setTimeout(() => settleSwipe(message), 120);
  }

  function settleSwipe(message: MessageHeader) {
    if (swipeId !== message.id) return;
    const action = actionFor(swipeOffset);
    // Reset first so the row snaps back even when no action fired.
    swipeId = null;
    swipeOffset = 0;
    if (action === "archive") onArchive?.(message.id);
    else if (action === "toggleRead") onSetRead?.(message.id, !message.read);
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
    <div class="left">
      {#if onToggleSidebar}
        <button
          class="toggle"
          aria-label="Toggle sidebar"
          title="Toggle sidebar"
          onclick={onToggleSidebar}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <rect
              x="1.75"
              y="2.75"
              width="12.5"
              height="10.5"
              rx="1.75"
              fill="none"
              stroke="currentColor"
              stroke-width="1.2"
            />
            <line
              x1="6.25"
              y1="2.75"
              x2="6.25"
              y2="13.25"
              stroke="currentColor"
              stroke-width="1.2"
            />
          </svg>
        </button>
      {/if}
      <div class="titles">
        <h1>{title}</h1>
        <p class="count">
          {messages.length}
          {messages.length === 1 ? "message" : "messages"}
        </p>
      </div>
    </div>
    <button
      class="compose"
      aria-label="New Message"
      title="New Message"
      onclick={onCompose}
    >
      ✎
    </button>
  </header>

  <div class="search">
    <input
      type="search"
      placeholder="Search — from:… to:… subject:… is:unread"
      aria-label="Search messages"
      bind:value={query}
      oninput={() => onSearch(query)}
    />
  </div>

  <div class="list" role="listbox" aria-label="Messages" bind:this={listEl}>
    {#if messages.length === 0}
      <p class="empty">No Messages</p>
    {:else}
      {#each messages as message (message.id)}
        {@const color = accountColors[message.accountId] ?? null}
        {@const offset = swipeId === message.id ? swipeOffset : 0}
        <div class="swipe-row" onwheel={(e) => handleWheel(message, e)}>
          {#if offset < 0}
            <span class="swipe-bg archive" aria-hidden="true">Archive</span>
          {:else if offset > 0}
            <span class="swipe-bg read" aria-hidden="true">
              {message.read ? "Mark Unread" : "Mark Read"}
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
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-shrink: 0;
    padding: 12px 16px 8px;
    border-bottom: 1px solid var(--hairline);
  }

  .left {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--text-secondary);
    cursor: default;
  }

  .toggle:hover {
    background: var(--bg-hover);
  }

  .toggle svg {
    width: 16px;
    height: 16px;
  }

  .compose {
    padding: 3px 9px;
    border: 1px solid var(--hairline);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    font-size: 14px;
    color: var(--text-secondary);
    cursor: pointer;
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

  .search {
    flex-shrink: 0;
    padding: 8px 12px;
    border-bottom: 1px solid var(--hairline);
  }

  .search input {
    width: 100%;
    padding: 4px 8px;
    border: 1px solid var(--hairline);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    font-size: 12px;
    color: inherit;
  }

  .search input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
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
  }

  .swipe-bg {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    padding: 0 14px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 600;
    color: #ffffff;
  }

  .swipe-bg.archive {
    justify-content: flex-end;
    background: #4f7cf7;
  }

  .swipe-bg.read {
    justify-content: flex-start;
    background: #f0a132;
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
