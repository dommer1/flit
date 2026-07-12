<script lang="ts">
  import { onMount } from "svelte";
  import {
    listAccounts,
    listMessages,
    onAccountsChanged,
    onMessagesChanged,
    openCompose,
    syncInbox,
  } from "./lib/api";
  import { replyDraft } from "./lib/draft";
  import type { Account, MessageHeader } from "./lib/types";
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

  let accounts = $state<Account[]>([]);
  let messages = $state<MessageHeader[]>([]);
  let selectedAccountId = $state<number | null>(null);
  let selectedMessageId = $state<number | null>(null);

  let selectedMessage = $derived(
    messages.find((m) => m.id === selectedMessageId) ?? null,
  );

  let listTitle = $derived(
    selectedAccountId === null
      ? "All Inboxes"
      : (accounts.find((a) => a.id === selectedAccountId)?.name ?? "Inbox"),
  );

  let paneWidths = $state<PaneWidths>(loadPaneWidths(localStorage));

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

  function openReply(message: MessageHeader, bodyText: string | null) {
    void openCompose(replyDraft(message, bodyText)).catch((err: unknown) =>
      console.error("failed to open reply:", err),
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

  async function selectAccount(accountId: number | null) {
    selectedAccountId = accountId;
    selectedMessageId = null;
    messages = await listMessages(accountId);
    startSync(accountId === null ? accounts.map((a) => a.id) : [accountId]);
  }

  // Re-query the current view without touching the selection — the derived
  // selectedMessage keeps pointing at the same id if it still exists.
  async function refreshMessages() {
    messages = await listMessages(selectedAccountId);
  }

  // why: fire-and-forget and parallel — every invoke runs as its own async
  // task in the backend, and the UI reads from the cache as each account's
  // messages-changed event lands. One slow server never delays the others.
  function startSync(accountIds: number[]) {
    for (const id of accountIds) {
      syncInbox(id).catch((err: unknown) => {
        console.error(`inbox sync failed for account ${id}:`, err);
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
    // why: if the selected account was deleted in the settings window, fall
    // back to the unified inbox instead of filtering by a dead account.
    if (
      selectedAccountId !== null &&
      !accounts.some((a) => a.id === selectedAccountId)
    ) {
      await selectAccount(null);
    }
  }

  onMount(() => {
    void (async () => {
      await refreshAccounts();
      await selectAccount(null);
    })();
    // why: account CRUD lives in the settings window (its own JS context) —
    // this window finds out through the backend's accounts-changed event.
    const unlistenAccounts = onAccountsChanged(() => void refreshAccounts());
    const unlistenMessages = onMessagesChanged(() => void refreshMessages());
    return () => {
      void unlistenAccounts.then((stop) => stop());
      void unlistenMessages.then((stop) => stop());
    };
  });
</script>

<!-- why: with titleBarStyle Overlay there is no native title bar left to grab,
     so the strip under the traffic lights becomes the window drag handle. -->
<div class="titlebar" data-tauri-drag-region></div>

<div
  class="layout"
  style:grid-template-columns={`${paneWidths.sidebar}px 1px minmax(0, 1fr)`}
>
  <aside>
    <Sidebar
      {accounts}
      selectedId={selectedAccountId}
      onSelect={selectAccount}
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
  <!-- The list and reading panes share one floating rounded card on top of
       the window's glass backdrop — the macOS Tahoe content-area look. -->
  <main
    class="card"
    style:grid-template-columns={`${paneWidths.list}px 1px minmax(0, 1fr)`}
  >
    <section class="list">
      <MessageList
        title={listTitle}
        {messages}
        selectedId={selectedMessageId}
        onSelect={(id) => (selectedMessageId = id)}
        onCompose={openNewMessage}
      />
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
      <MessageView message={selectedMessage} onReply={openReply} />
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

  .list {
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
