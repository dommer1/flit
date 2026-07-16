<script lang="ts">
  import { formatListDate, senderName } from "./format";
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
  } = $props();

  let query = $state("");

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
        <button
          role="option"
          aria-selected={selectedId === message.id}
          class:selected={selectedId === message.id}
          class:unread={!message.read}
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

  button {
    display: flex;
    align-items: flex-start;
    padding: 7px 8px 7px 4px;
    border: none;
    border-radius: 7px;
    background: none;
    font: inherit;
    color: inherit;
    text-align: left;
    cursor: default;
  }

  /* Inset separators between rows, hidden around the selected one —
     the Apple Mail look. */
  button + button {
    position: relative;
  }

  button + button::before {
    content: "";
    position: absolute;
    top: -1px;
    left: 20px;
    right: 8px;
    height: 1px;
    background: var(--hairline);
  }

  button.selected + button::before,
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
