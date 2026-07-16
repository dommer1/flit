<script lang="ts">
  import { onMount } from "svelte";
  import {
    archiveMessage,
    listAccounts,
    listMailboxes,
    listMessages,
    moveMessage,
    moveToTrash,
    onAccountsChanged,
    onMessagesChanged,
    onSendFinished,
    onSendQueued,
    onSendUndone,
    openCompose,
    searchMessages,
    setMessageRead,
    syncAccount,
    undoSend,
  } from "./lib/api";
  import { debounce } from "./lib/debounce";
  import {
    forwardDraft,
    replyAllDraft,
    replyDraft,
    type DraftKind,
  } from "./lib/draft";
  import type { Account, Mailbox, MessageHeader } from "./lib/types";
  import { neighborId, nextMessageId, type NavDelta } from "./lib/messageNav";
  import {
    clampPaneWidth,
    loadPaneWidths,
    PANE_LIMITS,
    savePaneWidths,
    type PaneWidths,
  } from "./lib/paneSizes";
  import Sidebar from "./lib/Sidebar.svelte";
  import MessageList from "./lib/MessageList.svelte";
  import MessageView from "./lib/MessageView.svelte";
  import Outbox, { type OutboxEntry } from "./lib/Outbox.svelte";

  let accounts = $state<Account[]>([]);
  let mailboxesByAccount = $state<Record<number, Mailbox[]>>({});
  let messages = $state<MessageHeader[]>([]);
  let selectedAccountId = $state<number | null>(null);
  let selectedMailbox = $state("INBOX");
  let selectedMessageId = $state<number | null>(null);

  let selectedMessage = $derived(
    messages.find((m) => m.id === selectedMessageId) ?? null,
  );

  // accountId → color, so the list can dot each row with its account's color
  // (most useful in the unified inbox, where accounts are interleaved).
  let accountColors = $derived(
    Object.fromEntries(accounts.map((a) => [a.id, a.color])),
  );

  function selectMessage(id: number) {
    selectedMessageId = id;
    // why: opening an unread message marks it read (like Apple Mail) — the
    // backend clears the local dot and pushes \Seen to the server.
    const message = messages.find((m) => m.id === id);
    if (message && !message.read) handleSetRead(id, true);
  }

  // why: the backend updates the cache and emits messages-changed, so the
  // list refreshes on its own — here we only fire the command and log a
  // failure (a stale flag heals on the next sync).
  function handleSetRead(id: number, read: boolean) {
    void setMessageRead(id, read).catch((err: unknown) =>
      console.error("failed to set read state:", err),
    );
  }

  // Move to Trash, then step the selection to the neighbour the removed
  // message leaves behind (like Apple Mail). The backend emits
  // messages-changed once the server confirms, dropping the row from the list.
  // Optimistically drop a message from the list and run `action` (trash /
  // archive) on the server, so it feels instant. Selection steps to the
  // neighbour it leaves behind; a server failure re-queries to bring the
  // message back rather than leave the list lying.
  function evictMessage(id: number, action: (id: number) => Promise<void>) {
    const next = neighborId(
      messages.map((m) => m.id),
      id,
    );
    messages = messages.filter((m) => m.id !== id);
    if (next === null) selectedMessageId = null;
    else selectMessage(next);
    void action(id).catch((err: unknown) => {
      console.error("message action failed:", err);
      void refreshMessages();
    });
  }

  function handleTrash(id: number) {
    evictMessage(id, moveToTrash);
  }

  function handleArchive(id: number) {
    evictMessage(id, archiveMessage);
  }

  function handleMove(id: number, mailbox: string) {
    evictMessage(id, (messageId) => moveMessage(messageId, mailbox));
  }

  // Arrow Up / Down walk the list. Ignored while typing in a field (search,
  // etc.) or with a modifier held, so system/app shortcuts aren't hijacked.
  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const target = event.target as HTMLElement | null;
    if (
      target?.tagName === "INPUT" ||
      target?.tagName === "TEXTAREA" ||
      target?.isContentEditable
    ) {
      return;
    }
    event.preventDefault();
    const delta: NavDelta = event.key === "ArrowDown" ? 1 : -1;
    const next = nextMessageId(
      messages.map((m) => m.id),
      selectedMessageId,
      delta,
    );
    if (next !== null) selectMessage(next);
  }

  let listTitle = $derived(
    selectedAccountId === null
      ? "All Inboxes"
      : selectedMailbox !== "INBOX"
        ? (mailboxesByAccount[selectedAccountId]?.find(
            (m) => m.name === selectedMailbox,
          )?.displayName ?? selectedMailbox)
        : (accounts.find((a) => a.id === selectedAccountId)?.name ?? "Inbox"),
  );

  let paneWidths = $state<PaneWidths>(loadPaneWidths(localStorage));

  const SIDEBAR_COLLAPSED_KEY = "flit.sidebar.collapsed";
  let sidebarCollapsed = $state(
    localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "true",
  );

  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed;
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(sidebarCollapsed));
  }

  // Sends riding out their undo window, mirrored from backend events.
  let outbox = $state<OutboxEntry[]>([]);
  /** How long a resolved badge lingers: long enough to read, short for ✓. */
  const SENT_BADGE_MS = 2500;
  const FAILED_BADGE_MS = 6000;

  function badgeQueued(id: number, subject: string, undoMs: number) {
    outbox = [...outbox, { id, subject, status: "sending", error: null, undoMs }];
  }

  function badgeFinished(id: number, error: string | null) {
    outbox = outbox.map((b) =>
      b.id === id ? { ...b, status: error ? "failed" : "sent", error } : b,
    );
    setTimeout(
      () => (outbox = outbox.filter((b) => b.id !== id)),
      error ? FAILED_BADGE_MS : SENT_BADGE_MS,
    );
  }

  function badgeUndone(id: number) {
    outbox = outbox.filter((b) => b.id !== id);
  }

  // why: the badge disappears on the backend's send-undone event, not here —
  // if undo lost the race the send is in flight and the badge must resolve.
  function handleUndo(id: number) {
    void undoSend(id).catch((err: unknown) =>
      console.error("undo send failed:", err),
    );
  }

  function openNewMessage() {
    const fallback = accounts[0];
    if (!fallback) return;
    // why: new mail goes from the account being viewed; on the unified inbox
    // the first account acts as the default sender.
    void openCompose({
      accountId: selectedAccountId ?? fallback.id,
      to: "",
      subject: "",
      body: "",
    }).catch((err: unknown) => console.error("failed to open compose:", err));
  }

  function openDraft(
    kind: DraftKind,
    message: MessageHeader,
    bodyText: string | null,
  ) {
    // why: reply-all must not echo mail back to the mailbox it landed in —
    // the receiving account's address is the one to drop from recipients.
    const ownEmail =
      accounts.find((a) => a.id === message.accountId)?.email ?? "";
    const draft =
      kind === "reply"
        ? replyDraft(message, bodyText)
        : kind === "reply-all"
          ? replyAllDraft(message, bodyText, ownEmail)
          : forwardDraft(message, bodyText);
    void openCompose(draft).catch((err: unknown) =>
      console.error(`failed to open ${kind}:`, err),
    );
  }

  function startPaneResize(pane: keyof PaneWidths, event: PointerEvent) {
    event.preventDefault();
    const divider = event.currentTarget as HTMLElement;
    const startX = event.clientX;
    const startWidth = paneWidths[pane];
    // why pointer capture: move/up events keep hitting the divider even when
    // the cursor leaves it mid-drag, so no window-level listeners are needed.
    // Optional call — jsdom doesn't implement it.
    divider.setPointerCapture?.(event.pointerId);
    const onMove = (e: PointerEvent) => {
      paneWidths[pane] = clampPaneWidth(pane, startWidth + e.clientX - startX);
    };
    const onUp = () => {
      divider.removeEventListener("pointermove", onMove);
      divider.removeEventListener("pointerup", onUp);
      savePaneWidths(localStorage, paneWidths);
    };
    divider.addEventListener("pointermove", onMove);
    divider.addEventListener("pointerup", onUp);
  }

  function nudgePane(pane: keyof PaneWidths, event: KeyboardEvent) {
    const delta =
      event.key === "ArrowLeft" ? -16 : event.key === "ArrowRight" ? 16 : 0;
    if (delta === 0) return;
    event.preventDefault();
    paneWidths[pane] = clampPaneWidth(pane, paneWidths[pane] + delta);
    savePaneWidths(localStorage, paneWidths);
  }

  // why: plain variable, not $state — the input's value lives in MessageList;
  // this only steers which query refreshMessages runs.
  let searchQuery = "";

  async function selectMailbox(accountId: number | null, mailbox = "INBOX") {
    selectedAccountId = accountId;
    selectedMailbox = mailbox;
    selectedMessageId = null;
    await refreshMessages();
    startSync(accountId === null ? accounts.map((a) => a.id) : [accountId]);
  }

  // Re-query the current view without touching the selection — the derived
  // selectedMessage keeps pointing at the same id if it still exists. With a
  // search active, "the current view" is the result list (across all folders,
  // like Gmail), so a sync landing mid-search refreshes the hits instead of
  // yanking the full list back.
  async function refreshMessages() {
    const query = searchQuery.trim();
    messages = query
      ? await searchMessages(selectedAccountId, query)
      : await listMessages(selectedAccountId, selectedMailbox);
  }

  // The sidebar's folder lists, mirrored per account. Refreshed alongside
  // messages because folders first appear when an account's sync lands.
  async function refreshMailboxes() {
    const entries = await Promise.all(
      accounts.map(
        async (a) => [a.id, await listMailboxes(a.id)] as const,
      ),
    );
    mailboxesByAccount = Object.fromEntries(entries);
  }

  // why 200ms: long enough to collapse a typing burst into one query, short
  // enough that results still feel live (search is a local SQLite hit).
  const refreshDebounced = debounce(() => void refreshMessages(), 200);

  function handleSearch(query: string) {
    searchQuery = query;
    refreshDebounced();
  }

  // why: fire-and-forget and parallel — every invoke runs as its own async
  // task in the backend, and the UI reads from the cache as each account's
  // messages-changed event lands. One slow server never delays the others.
  function startSync(accountIds: number[]) {
    for (const id of accountIds) {
      syncAccount(id).catch((err: unknown) => {
        console.error(`account sync failed for account ${id}:`, err);
      });
    }
  }

  // why: plain variable, not $state — nothing renders from it; it only stops
  // the very first refresh from double-syncing what onMount already syncs.
  let accountsLoaded = false;

  async function refreshAccounts() {
    const known = new Set(accounts.map((a) => a.id));
    accounts = await listAccounts();
    // why: an account just added in settings syncs right away, so its
    // messages and connection status appear without waiting for a selection.
    if (accountsLoaded) {
      const added = accounts.filter((a) => !known.has(a.id));
      if (added.length > 0) startSync(added.map((a) => a.id));
    }
    accountsLoaded = true;
    await refreshMailboxes();
    // why: if the selected account was deleted in the settings window, fall
    // back to the unified inbox instead of filtering by a dead account.
    if (
      selectedAccountId !== null &&
      !accounts.some((a) => a.id === selectedAccountId)
    ) {
      await selectMailbox(null);
    }
  }

  onMount(() => {
    void (async () => {
      await refreshAccounts();
      await selectMailbox(null);
    })();
    // why: account CRUD lives in the settings window (its own JS context) —
    // this window finds out through the backend's accounts-changed event.
    const unlistenAccounts = onAccountsChanged(() => void refreshAccounts());
    const unlistenMessages = onMessagesChanged(() => {
      void refreshMessages();
      void refreshMailboxes();
    });
    // why: compose windows queue sends in the backend; this window only
    // mirrors the send-* events into badges.
    const unlistenQueued = onSendQueued((e) =>
      badgeQueued(e.id, e.subject, e.undoMs),
    );
    const unlistenFinished = onSendFinished((e) => badgeFinished(e.id, e.error));
    const unlistenUndone = onSendUndone((e) => badgeUndone(e.id));
    return () => {
      void unlistenAccounts.then((stop) => stop());
      void unlistenMessages.then((stop) => stop());
      void unlistenQueued.then((stop) => stop());
      void unlistenFinished.then((stop) => stop());
      void unlistenUndone.then((stop) => stop());
    };
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- why: with titleBarStyle Overlay there is no native title bar left to grab,
     so the strip under the traffic lights becomes the window drag handle. -->
<div class="titlebar" data-tauri-drag-region></div>

<div
  class="layout"
  style:grid-template-columns={sidebarCollapsed
    ? "minmax(0, 1fr)"
    : `${paneWidths.sidebar}px 1px minmax(0, 1fr)`}
>
  {#if !sidebarCollapsed}
    <aside>
      <Sidebar
        {accounts}
        mailboxes={mailboxesByAccount}
        selectedAccountId={selectedAccountId}
        selectedMailbox={selectedMailbox}
        onSelect={selectMailbox}
      />
    </aside>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions
         — a focusable separator is the ARIA "window splitter" widget; Svelte's
         checker only knows the static (non-focusable) separator variant. -->
    <div
      class="divider ghost"
      role="separator"
      tabindex="0"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      aria-valuenow={paneWidths.sidebar}
      aria-valuemin={PANE_LIMITS.sidebar.min}
      aria-valuemax={PANE_LIMITS.sidebar.max}
      onpointerdown={(e) => startPaneResize("sidebar", e)}
      onkeydown={(e) => nudgePane("sidebar", e)}
    ></div>
  {/if}
  <!-- The list and reading panes share one floating rounded card on top of
       the window's glass backdrop — the macOS Tahoe content-area look. -->
  <main
    class="card"
    class:collapsed={sidebarCollapsed}
    style:grid-template-columns={`${paneWidths.list}px 1px minmax(0, 1fr)`}
  >
    <section class="list">
      <MessageList
        title={listTitle}
        {messages}
        {accountColors}
        selectedId={selectedMessageId}
        onSelect={selectMessage}
        onCompose={openNewMessage}
        onSearch={handleSearch}
        onToggleSidebar={toggleSidebar}
        onArchive={handleArchive}
        onSetRead={handleSetRead}
      />
      <Outbox entries={outbox} onUndo={handleUndo} />
    </section>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions
         — a focusable separator is the ARIA "window splitter" widget; Svelte's
         checker only knows the static (non-focusable) separator variant. -->
    <div
      class="divider"
      role="separator"
      tabindex="0"
      aria-orientation="vertical"
      aria-label="Resize message list"
      aria-valuenow={paneWidths.list}
      aria-valuemin={PANE_LIMITS.list.min}
      aria-valuemax={PANE_LIMITS.list.max}
      onpointerdown={(e) => startPaneResize("list", e)}
      onkeydown={(e) => nudgePane("list", e)}
    ></div>
    <section class="view">
      <MessageView
        message={selectedMessage}
        mailboxes={selectedMessage
          ? (mailboxesByAccount[selectedMessage.accountId] ?? [])
          : []}
        onDraft={openDraft}
        onSetRead={handleSetRead}
        onArchive={handleArchive}
        onTrash={handleTrash}
        onMove={handleMove}
      />
    </section>
  </main>
</div>

<style>
  /* why: the vibrancy NSVisualEffectView sits behind the webview — the body
     must not paint over it. Only this window applies vibrancy, so the rule
     lives here rather than in the shared stylesheet. */
  :global(body) {
    background: transparent;
  }

  .titlebar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: calc(var(--titlebar-inset) - 6px);
    z-index: 10;
  }

  .layout {
    display: grid;
    height: 100vh;
  }

  aside {
    padding-top: var(--titlebar-inset);
    overflow-y: auto;
  }

  .card {
    display: grid;
    min-width: 0;
    margin: calc(var(--titlebar-inset) - 6px) 10px 10px 0;
    border-radius: 10px;
    background: var(--bg-window);
    box-shadow:
      0 0 0 1px var(--hairline),
      0 8px 28px rgba(0, 0, 0, 0.14);
    overflow: hidden;
  }

  /* With the sidebar hidden the card spans the window, but keeps a small
     left inset so it still reads as a floating card, not a flush panel. */
  .card.collapsed {
    margin-left: 10px;
  }

  .list {
    /* why relative: the Outbox badges anchor to this pane's bottom edge. */
    position: relative;
    overflow-y: auto;
  }

  /* Replaces the old border-right lines: a 1px grid column that doubles as
     a drag handle, with a wider invisible grab area via the ::after overlay. */
  .divider {
    position: relative;
    background: var(--divider);
    cursor: col-resize;
    touch-action: none;
  }

  .divider::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: -3px;
    right: -3px;
  }

  /* The sidebar handle is invisible — on the glass backdrop the card edge
     is the visual boundary, but the grab area stays. */
  .divider.ghost {
    background: transparent;
  }

  .divider:focus-visible {
    outline: none;
    background: var(--accent);
  }

  .view {
    display: flex;
    flex-direction: column;
    overflow-y: auto;
  }
</style>
