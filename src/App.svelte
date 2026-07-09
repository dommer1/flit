<script lang="ts">
  import { onMount } from "svelte";
  import {
    listAccounts,
    listMessages,
    onAccountsChanged,
    onMessagesChanged,
    syncInbox,
  } from "./lib/api";
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

  let paneWidths = $state<PaneWidths>(loadPaneWidths(localStorage));

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

  // why: fire-and-forget and sequential on purpose — the UI reads from the
  // cache and updates via messages-changed events, and phase 3 turns this
  // loop into parallel per-account tasks.
  function startSync(accountIds: number[]) {
    void (async () => {
      for (const id of accountIds) {
        try {
          await syncInbox(id);
        } catch (err) {
          console.error(`inbox sync failed for account ${id}:`, err);
        }
      }
    })();
  }

  async function refreshAccounts() {
    accounts = await listAccounts();
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

<div
  class="layout"
  style:grid-template-columns={`${paneWidths.sidebar}px 1px ${paneWidths.list}px 1px minmax(0, 1fr)`}
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
  <section class="list">
    <MessageList
      {messages}
      selectedId={selectedMessageId}
      onSelect={(id) => (selectedMessageId = id)}
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
    <MessageView message={selectedMessage} />
  </section>
</div>

<style>
  .layout {
    display: grid;
    height: 100vh;
  }

  aside {
    background: var(--bg-sidebar);
    overflow-y: auto;
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
