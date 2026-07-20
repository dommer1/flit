<script lang="ts">
  import { onMount } from "svelte";
  import {
    archiveMessage,
    archiveThread,
    cancelScheduled,
    getMessageBody,
    getMessageQuote,
    getSwipeActions,
    getThreadOrder,
    listAccounts,
    listAliases,
    listMailboxes,
    listMessages,
    listThread,
    listScheduled,
    moveMessage,
    moveThread,
    moveToTrash,
    trashThread,
    onAccountsChanged,
    onMessagesChanged,
    onSettingsChanged,
    onScheduledChanged,
    onScheduledMissed,
    onSendFinished,
    onSendQueued,
    onSendUndone,
    openCompose,
    openDraft as openServerDraft,
    openSettings,
    searchMessages,
    sendScheduledNow,
    setMessageRead,
    syncAccount,
    undoSend,
    viewStatus,
  } from "./lib/api";
  import { debounce } from "./lib/debounce";
  import {
    forwardDraft,
    replyAllDraft,
    replyDraft,
    type DraftKind,
  } from "./lib/draft";
  import type {
    Account,
    Alias,
    Mailbox,
    MessageHeader,
    MessageQuote,
    ScheduledMessage,
    SwipeActions,
    ThreadOrder,
    ViewStatus,
  } from "./lib/types";
  import { DEFAULT_SWIPE_ACTIONS } from "./lib/swipe";
  import { neighborId, nextMessageId, type NavDelta } from "./lib/messageNav";
  import {
    clampPaneWidth,
    loadPaneWidths,
    PANE_LIMITS,
    savePaneWidths,
    type PaneWidths,
  } from "./lib/paneSizes";
  import Sidebar from "./lib/Sidebar.svelte";
  import Toolbar from "./lib/Toolbar.svelte";
  import MessageList from "./lib/MessageList.svelte";
  import MessageView from "./lib/MessageView.svelte";
  import Outbox, { type OutboxEntry } from "./lib/Outbox.svelte";
  import MissedSends from "./lib/MissedSends.svelte";

  let accounts = $state<Account[]>([]);
  let aliases = $state<Alias[]>([]);
  let mailboxesByAccount = $state<Record<number, Mailbox[]>>({});
  let messages = $state<MessageHeader[]>([]);
  let selectedAccountId = $state<number | null>(null);
  let selectedMailbox = $state("INBOX");
  let selectedMessageId = $state<number | null>(null);

  // Rows revealed per "page" of the list — the cache may mirror tens of
  // thousands, but the DOM only grows as the user actually scrolls.
  const LIST_PAGE = 500;
  let visibleLimit = $state(LIST_PAGE);
  // Whole-view totals + backfill progress; null while a search is active.
  let listStatus = $state<ViewStatus | null>(null);

  function handleLoadMore() {
    // Fires repeatedly while the user sits near the bottom — bump once and
    // ignore the rest until the longer list has actually arrived.
    if (messages.length < visibleLimit) return;
    visibleLimit += LIST_PAGE;
    void refreshMessages().catch((err: unknown) =>
      console.error("failed to load more messages:", err),
    );
  }

  let selectedMessage = $derived(
    messages.find((m) => m.id === selectedMessageId) ?? null,
  );

  // accountId → color, so the list can dot each row with its account's color
  // (most useful in the unified inbox, where accounts are interleaved).
  let accountColors = $derived(
    Object.fromEntries(accounts.map((a) => [a.id, a.color])),
  );

  // accountId → address, so the conversation view can label own messages.
  let accountEmails = $derived(
    Object.fromEntries(accounts.map((a) => [a.id, a.email])),
  );

  // A message living in a Drafts folder is unfinished work — opening it
  // resumes editing in a compose window instead of a reading view. Used by
  // the list (drafts folder rows) and the conversation view (draft cards).
  function editDraft(id: number) {
    void openServerDraft(id).catch((err: unknown) =>
      console.error("failed to open draft:", err),
    );
  }

  function selectMessage(id: number) {
    const message = messages.find((m) => m.id === id);
    if (message && isDraft(message)) {
      editDraft(id);
      return;
    }
    selectedMessageId = id;
    // why: opening an unread message marks it read (like Apple Mail) — the
    // backend clears the local dot and pushes \Seen to the server.
    if (message && !message.read) handleSetRead(id, true);
  }

  function isDraft(message: MessageHeader): boolean {
    return (mailboxesByAccount[message.accountId] ?? []).some(
      (m) => m.name === message.mailbox && m.role === "drafts",
    );
  }

  // why: the backend updates the cache and emits messages-changed, so the
  // list refreshes on its own — here we only fire the command and log a
  // failure (a stale flag heals on the next sync).
  function handleSetRead(id: number, read: boolean) {
    void setMessageRead(id, read).catch((err: unknown) =>
      console.error("failed to set read state:", err),
    );
  }

  // Rows optimistically dropped whose server move is still in flight. The
  // backend deletes a cached row only after the server confirms the move,
  // so a background messages-changed landing mid-move would resurrect the
  // row via refreshMessages — the filter there keeps it out until the
  // action settles. Plain Set, not $state: nothing renders from it, it is
  // only read inside refreshMessages, which runs after every mutation.
  const pendingEvictions = new Set<number>();

  // Optimistically drop a message from the list and run `action` (trash /
  // archive) on the server, so it feels instant. Selection steps to the
  // neighbour it leaves behind; a server failure re-queries to bring the
  // message back rather than leave the list lying.
  function evictMessage(id: number, action: (id: number) => Promise<void>) {
    const next = neighborId(
      messages.map((m) => m.id),
      id,
    );
    pendingEvictions.add(id);
    messages = messages.filter((m) => m.id !== id);
    if (next === null) selectedMessageId = null;
    else selectMessage(next);
    void action(id).then(
      // Success needs no refresh of its own: the backend deleted the row
      // before resolving and emits messages-changed, which refreshes.
      () => pendingEvictions.delete(id),
      (err: unknown) => {
        console.error("message action failed:", err);
        // why: un-pend before the refresh — the row is still cached after
        // a failure and must come back into the list.
        pendingEvictions.delete(id);
        void refreshMessages();
      },
    );
  }

  // Thread rows (a grouped conversation) act on every member in the
  // current folder; single rows — including all rows of the flat
  // trash/junk/drafts views, where threadCount is 0 — act on one message.
  function isThreadRow(id: number): boolean {
    return (messages.find((m) => m.id === id)?.threadCount ?? 0) > 1;
  }

  function handleTrash(id: number) {
    evictMessage(id, isThreadRow(id) ? trashThread : moveToTrash);
  }

  // What the list's swipe gesture does per direction — user-configurable in
  // the settings window, so it loads on start and re-loads on settings-changed.
  let swipeActions = $state<SwipeActions>(DEFAULT_SWIPE_ACTIONS);

  async function refreshSwipeActions() {
    swipeActions = await getSwipeActions();
  }

  // How the conversation view orders its cards — user-configurable in the
  // settings window, reloaded on settings-changed like the swipe actions.
  let threadOrder = $state<ThreadOrder>("newestLast");

  async function refreshThreadOrder() {
    threadOrder = await getThreadOrder();
  }

  // why fetch first: neither the list rows nor the toolbar hold the body —
  // replies fetch quote material, forwards the plain text. Both are local
  // cache hits, and a failure still opens the compose window, just without
  // the quoted content.
  function openDraftWithBody(kind: DraftKind, message: MessageHeader) {
    const open =
      kind === "forward"
        ? getMessageBody(message.id).then((body) =>
            openDraft(kind, message, null, body.text),
          )
        : getMessageQuote(message.id).then((quote) =>
            openDraft(kind, message, quote, null),
          );
    void open.catch((err: unknown) => {
      console.error("failed to load content for draft:", err);
      openDraft(kind, message, null, null);
    });
  }

  function handleSwipeReply(id: number) {
    const message = messages.find((m) => m.id === id);
    if (message) void openDraftOnLatest("reply", message);
  }

  // why resolve first: a list row only represents the newest message in the
  // open folder — after my own reply (filed in Sent) the conversation's true
  // latest message lives elsewhere, and that's the one a follow-up quotes.
  // listThread is a local cache hit; on failure the row itself still works.
  async function openDraftOnLatest(kind: DraftKind, message: MessageHeader) {
    let target = message;
    try {
      const thread = await listThread(message.id);
      target = thread[thread.length - 1] ?? message;
    } catch (err) {
      console.error("failed to resolve the thread's latest message:", err);
    }
    openDraftWithBody(kind, target);
  }

  // The account's archive folder wire name; Gmail has no \Archive, so its
  // All Mail (\All) counts — mirrors the backend's archive fallback order.
  function archiveNameFor(accountId: number): string | null {
    const folders = mailboxesByAccount[accountId] ?? [];
    const archive =
      folders.find((m) => m.role === "archive") ??
      folders.find((m) => m.role === "all");
    return archive?.name ?? null;
  }

  function isArchived(message: MessageHeader): boolean {
    return message.mailbox === archiveNameFor(message.accountId);
  }

  // Archive flips to unarchive on an already-archived message — re-archiving
  // would be a silent server no-op while the row vanished from the list.
  function handleArchive(id: number) {
    const message = messages.find((m) => m.id === id);
    const thread = isThreadRow(id);
    if (message && isArchived(message)) {
      const inbox =
        (mailboxesByAccount[message.accountId] ?? []).find(
          (m) => m.role === "inbox",
        )?.name ?? "INBOX";
      handleMove(id, inbox);
    } else {
      evictMessage(id, thread ? archiveThread : archiveMessage);
    }
  }

  function handleMove(id: number, mailbox: string) {
    const move = isThreadRow(id) ? moveThread : moveMessage;
    evictMessage(id, (messageId) => move(messageId, mailbox));
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

  // Send-later messages parked in the backend: pending ones feed the
  // sidebar's Scheduled section; missed ones (due while the app was off or
  // asleep past the grace window) are never auto-sent — the catch-up dialog
  // asks per message.
  let scheduledSends = $state<ScheduledMessage[]>([]);
  let missedSends = $state<ScheduledMessage[]>([]);
  // "Decide later" hides these ids until the next launch; the rows stay
  // missed in the DB, so a restart asks again.
  const dismissedMissed = new Set<number>();

  async function refreshScheduled() {
    const scheduled = await listScheduled();
    scheduledSends = scheduled.filter((m) => m.status === "pending");
    missedSends = scheduled.filter(
      (m) => m.status === "missed" && !dismissedMissed.has(m.id),
    );
  }

  // why remove-first: the row is gone from the DB either way (take() is the
  // claim); keeping it in the dialog would just offer a dead button.
  function missedSendNow(id: number) {
    missedSends = missedSends.filter((m) => m.id !== id);
    void sendScheduledNow(id).catch((err: unknown) =>
      console.error("send scheduled now failed:", err),
    );
  }

  function missedOpenDraft(id: number) {
    missedSends = missedSends.filter((m) => m.id !== id);
    void cancelScheduled(id).catch((err: unknown) =>
      console.error("cancel scheduled failed:", err),
    );
  }

  function missedDismiss() {
    for (const entry of missedSends) dismissedMissed.add(entry.id);
    missedSends = [];
  }

  // why remove-first: like the dialog's actions, the row is already claimed
  // in the backend; the scheduled-changed event re-syncs the list after.
  function handleCancelScheduled(id: number) {
    scheduledSends = scheduledSends.filter((m) => m.id !== id);
    void cancelScheduled(id).catch((err: unknown) =>
      console.error("cancel scheduled failed:", err),
    );
  }

  function openNewMessage() {
    const fallback = accounts[0];
    if (!fallback) return;
    // why: new mail goes from the account being viewed; on the unified inbox
    // the first account acts as the default sender.
    const sender = accounts.find((a) => a.id === selectedAccountId) ?? fallback;
    void openCompose({
      accountId: sender.id,
      // The identity marked Default in the account's alias settings.
      aliasId: sender.defaultAliasId ?? undefined,
      to: "",
      subject: "",
      body: "",
    }).catch((err: unknown) => console.error("failed to open compose:", err));
  }

  function openDraft(
    kind: DraftKind,
    message: MessageHeader,
    quote: MessageQuote | null,
    bodyText: string | null,
  ) {
    // why: reply-all must not echo mail back to the mailbox it landed in —
    // the receiving account's address is the one to drop from recipients.
    const ownEmail =
      accounts.find((a) => a.id === message.accountId)?.email ?? "";
    const draft =
      kind === "reply"
        ? replyDraft(message, quote, aliases, ownEmail)
        : kind === "reply-all"
          ? replyAllDraft(message, quote, ownEmail, aliases)
          : forwardDraft(message, bodyText, aliases);
    void openCompose(draft).catch((err: unknown) =>
      console.error(`failed to open ${kind}:`, err),
    );
  }

  // The toolbar's reply/forward buttons act on the open conversation.
  function openDraftFromSelection(kind: DraftKind) {
    if (selectedMessage) void openDraftOnLatest(kind, selectedMessage);
  }

  // Move to targets for the toolbar menu: the selection's account folders,
  // minus the folder the message already sits in (a self-move is a no-op).
  let moveTargets = $derived(
    selectedMessage === null
      ? []
      : (mailboxesByAccount[selectedMessage.accountId] ?? []).filter(
          (m) => m.name !== selectedMessage?.mailbox,
        ),
  );

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

  // why: plain variable, not $state — the input's value lives in Toolbar;
  // this only steers which query refreshMessages runs.
  let searchQuery = "";

  // Manual "check for new mail": re-sync what the user is looking at — all
  // accounts on the unified inbox, otherwise just the selected one.
  function handleRefresh() {
    startSync(
      selectedAccountId === null ? accounts.map((a) => a.id) : [selectedAccountId],
    );
  }

  // Pull-to-refresh: the same re-sync, but awaited (unlike startSync) so the
  // list's drawer can stay open until every account's sync lands.
  let pullRefreshing = $state(false);

  async function handlePullRefresh() {
    if (pullRefreshing) return;
    pullRefreshing = true;
    const ids =
      selectedAccountId === null
        ? accounts.map((a) => a.id)
        : [selectedAccountId];
    // why allSettled: one account's dead server must neither hide the other
    // syncs' results nor leave the drawer spinning forever.
    const results = await Promise.allSettled(ids.map((id) => syncAccount(id)));
    results.forEach((result, i) => {
      if (result.status === "rejected")
        console.error(
          `account sync failed for account ${ids[i]}:`,
          result.reason,
        );
    });
    pullRefreshing = false;
  }

  function handleOpenSettings() {
    void openSettings().catch((err: unknown) =>
      console.error("failed to open settings:", err),
    );
  }

  async function selectMailbox(accountId: number | null, mailbox = "INBOX") {
    selectedAccountId = accountId;
    selectedMailbox = mailbox;
    selectedMessageId = null;
    // A fresh view starts at the first page again.
    visibleLimit = LIST_PAGE;
    await refreshMessages();
    startSync(accountId === null ? accounts.map((a) => a.id) : [accountId]);
  }

  // Re-query the current view without touching the selection — the derived
  // selectedMessage keeps pointing at the same id if it still exists. With a
  // search active, "the current view" is the result list (across all folders,
  // like Gmail), so a sync landing mid-search refreshes the hits instead of
  // yanking the full list back.
  // why filter at assignment (after the await): a refresh already in
  // flight when an eviction starts still applies the filter when it lands,
  // so even a stale query cannot bring the row back.
  function withoutPending(list: MessageHeader[]): MessageHeader[] {
    return list.filter((m) => !pendingEvictions.has(m.id));
  }

  async function refreshMessages() {
    const query = searchQuery.trim();
    if (query) {
      messages = withoutPending(await searchMessages(selectedAccountId, query));
      // Search results are their own universe — whole-view totals and the
      // load-more trigger don't apply to them.
      listStatus = null;
      return;
    }
    const [list, status] = await Promise.all([
      listMessages(selectedAccountId, selectedMailbox, visibleLimit),
      viewStatus(selectedAccountId, selectedMailbox),
    ]);
    messages = withoutPending(list);
    listStatus = status;
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
    // why here: alias edits broadcast accounts-changed too, so reply
    // matching always works against the current alias list.
    aliases = await listAliases();
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
    const refreshSwipeLogged = () => {
      void refreshSwipeActions().catch((err: unknown) =>
        console.error("failed to load swipe actions:", err),
      );
      void refreshThreadOrder().catch((err: unknown) =>
        console.error("failed to load thread order:", err),
      );
    };
    refreshSwipeLogged();
    // why: the settings window mutates the config in its own JS context —
    // this window finds out through the backend's settings-changed event.
    const unlistenSettings = onSettingsChanged(refreshSwipeLogged);
    // why separate: scheduled/missed sends must surface even if account sync
    // fails — they are local rows, not server state.
    const refreshScheduledLogged = () =>
      void refreshScheduled().catch((err: unknown) =>
        console.error("failed to load scheduled sends:", err),
      );
    refreshScheduledLogged();
    const unlistenMissed = onScheduledMissed(refreshScheduledLogged);
    const unlistenScheduled = onScheduledChanged(refreshScheduledLogged);
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
      void unlistenMissed.then((stop) => stop());
      void unlistenScheduled.then((stop) => stop());
      void unlistenSettings.then((stop) => stop());
    };
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<Toolbar
  {sidebarCollapsed}
  sidebarWidth={paneWidths.sidebar}
  onToggleSidebar={toggleSidebar}
  onRefresh={handleRefresh}
  onCompose={openNewMessage}
  onSearch={handleSearch}
  onOpenSettings={handleOpenSettings}
  selected={selectedMessage}
  archived={selectedMessage ? isArchived(selectedMessage) : false}
  {moveTargets}
  onDraft={openDraftFromSelection}
  onSetRead={handleSetRead}
  onArchive={handleArchive}
  onTrash={handleTrash}
  onMove={handleMove}
/>

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
        scheduled={scheduledSends}
        onCancelScheduled={handleCancelScheduled}
      />
    </aside>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions
         — a focusable separator is the ARIA "window splitter" widget; Svelte's
         checker only knows the static (non-focusable) separator variant. -->
    <div
      class="divider"
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
  <main
    style:grid-template-columns={`${paneWidths.list}px 1px minmax(0, 1fr)`}
  >
    <section class="list">
      <MessageList
        title={listTitle}
        {messages}
        {accountColors}
        selectedId={selectedMessageId}
        onSelect={selectMessage}
        {swipeActions}
        onArchive={handleArchive}
        onSetRead={handleSetRead}
        {isArchived}
        onTrash={handleTrash}
        onReply={handleSwipeReply}
        status={listStatus}
        onLoadMore={handleLoadMore}
        onRefresh={handlePullRefresh}
        refreshing={pullRefreshing}
      />
      <Outbox entries={outbox} onUndo={handleUndo} />
    </section>
    <MissedSends
      entries={missedSends}
      onSendNow={missedSendNow}
      onOpenDraft={missedOpenDraft}
      onDismiss={missedDismiss}
    />
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
        {accountEmails}
        {accountColors}
        {threadOrder}
        onDraft={openDraftWithBody}
        onEditDraft={editDraft}
      />
    </section>
  </main>
</div>

<style>
  .layout {
    display: grid;
    height: calc(100vh - var(--toolbar-height));
    /* why: implicit (auto) grid rows size to their content, so a tall
       conversation stack grew the row past the viewport and the whole app
       scrolled. Pin the single row to the container's height — panes clip
       and scroll themselves instead. */
    grid-template-rows: 100%;
  }

  /* why overflow-x hidden everywhere: overflow-y auto alone computes
     overflow-x to auto, so a squeezed pane (narrow window, panes dragged
     wide) grows a horizontal scrollbar — panes must only ever clip. */
  aside {
    overflow-y: auto;
    overflow-x: hidden;
    background: var(--bg-sidebar);
  }

  main {
    display: grid;
    /* Same row pinning as .layout — main is its own grid. */
    grid-template-rows: 100%;
    min-width: 0;
    min-height: 0;
    background: var(--bg-window);
  }

  .list {
    /* why relative: the Outbox badges anchor to this pane's bottom edge. */
    position: relative;
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* Replaces the old border-right lines: a 1px grid column that doubles as
     a drag handle, with a wider invisible grab area via the ::after overlay. */
  .divider {
    position: relative;
    background: var(--border-chrome);
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

  .divider:focus-visible {
    outline: none;
    background: var(--accent);
  }

  .view {
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    overflow-x: hidden;
  }
</style>
