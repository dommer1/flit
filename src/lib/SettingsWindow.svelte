<script lang="ts">
  import { onMount } from "svelte";
  import {
    addAccount,
    addAlias,
    closeSettings,
    deleteAlias,
    listAliases,
    setDefaultAlias,
    updateAlias,
    confirmAccountDeletion,
    deleteAccount,
    getRemoteImagePolicy,
    getSwipeActions,
    getThreadOrder,
    listAccounts,
    onAccountsChanged,
    setAccountColor,
    setRemoteImagePolicy,
    setSwipeActions,
    setThreadOrder,
    testConnection,
  } from "./api";
  import { DEFAULT_SWIPE_ACTIONS } from "./swipe";
  import type {
    Account,
    Alias,
    NewAccount,
    RemoteImagePolicy,
    SwipeAction,
    SwipeActions,
    ThreadOrder,
  } from "./types";
  import AccountsPane from "./AccountsPane.svelte";
  import NotificationsPane from "./NotificationsPane.svelte";
  import SignaturesPane from "./SignaturesPane.svelte";

  const POLICIES: { value: RemoteImagePolicy; label: string; hint: string }[] =
    [
      {
        value: "block",
        label: "Never load",
        hint: "Messages show without their remote images.",
      },
      {
        value: "ask",
        label: "Ask for each message",
        hint: "A banner offers to load them, one message at a time.",
      },
      {
        value: "always",
        label: "Always load",
        hint: "Senders may learn when you open their mail.",
      },
    ];

  const SWIPE_OPTIONS: { value: SwipeAction; label: string }[] = [
    { value: "none", label: "None" },
    { value: "toggleRead", label: "Mark Read / Unread" },
    { value: "archive", label: "Archive" },
    { value: "trash", label: "Trash" },
    { value: "reply", label: "Reply" },
  ];

  let accounts = $state<Account[]>([]);
  let aliases = $state<Alias[]>([]);
  let lastError = $state<string | null>(null);
  let tab = $state<
    "general" | "accounts" | "signatures" | "notifications" | "swipes" | "privacy"
  >("accounts");
  let policy = $state<RemoteImagePolicy>("ask");
  let swipes = $state<SwipeActions>(DEFAULT_SWIPE_ACTIONS);
  let threadOrder = $state<ThreadOrder>("newestLast");

  async function refresh() {
    accounts = await listAccounts();
    aliases = await listAliases();
  }

  async function handleAdd(
    account: NewAccount,
    password: string,
  ): Promise<Account | null> {
    lastError = null;
    try {
      // why: Verify & Save — the account is only stored after both servers
      // accepted the credentials (decided 2026-07-08, see CLAUDE.md phase 2).
      await testConnection(account, password);
      const created = await addAccount(account, password);
      await refresh();
      return created;
    } catch (err) {
      lastError = String(err);
      return null;
    }
  }

  async function handleDelete(account: Account) {
    lastError = null;
    try {
      if (!(await confirmAccountDeletion(account))) return;
      await deleteAccount(account.id);
      await refresh();
    } catch (err) {
      lastError = String(err);
    }
  }

  async function handleSetColor(id: number, color: string | null) {
    lastError = null;
    try {
      await setAccountColor(id, color);
      await refresh();
    } catch (err) {
      lastError = String(err);
    }
  }

  async function handleAddAlias(
    accountId: number,
    name: string,
    email: string,
  ): Promise<Alias | null> {
    lastError = null;
    try {
      const created = await addAlias(accountId, name, email);
      await refresh();
      return created;
    } catch (err) {
      lastError = String(err);
      return null;
    }
  }

  async function handleUpdateAlias(id: number, name: string, email: string) {
    lastError = null;
    try {
      await updateAlias(id, name, email);
    } catch (err) {
      lastError = String(err);
    }
    // why: refresh either way — a rejected edit must snap the input back to
    // the stored value instead of showing text that was never saved.
    await refresh();
  }

  async function handleDeleteAlias(id: number) {
    lastError = null;
    try {
      await deleteAlias(id);
      await refresh();
    } catch (err) {
      lastError = String(err);
    }
  }

  async function handleSetDefaultAlias(
    accountId: number,
    aliasId: number | null,
  ) {
    lastError = null;
    try {
      await setDefaultAlias(accountId, aliasId);
      await refresh();
    } catch (err) {
      lastError = String(err);
    }
  }

  async function selectPolicy(next: RemoteImagePolicy) {
    lastError = null;
    const previous = policy;
    policy = next;
    try {
      await setRemoteImagePolicy(next);
    } catch (err) {
      // why: the radio must not lie — a failed save rolls the dot back.
      policy = previous;
      lastError = String(err);
    }
  }

  async function selectSwipe(side: "left" | "right", action: SwipeAction) {
    lastError = null;
    const previous = swipes;
    swipes = { ...previous, [side]: action };
    try {
      await setSwipeActions(swipes);
    } catch (err) {
      // why: the select must not lie — a failed save rolls the value back.
      swipes = previous;
      lastError = String(err);
    }
  }

  async function selectThreadOrder(next: ThreadOrder) {
    lastError = null;
    const previous = threadOrder;
    threadOrder = next;
    try {
      await setThreadOrder(next);
    } catch (err) {
      // why: the select must not lie — a failed save rolls the value back.
      threadOrder = previous;
      lastError = String(err);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") void closeSettings();
  }

  onMount(() => {
    void refresh();
    void getRemoteImagePolicy().then((stored) => (policy = stored));
    void getSwipeActions().then((stored) => (swipes = stored));
    void getThreadOrder().then((stored) => (threadOrder = stored));
    // why: refreshes also cover changes made elsewhere (a future main-window
    // action, another settings session) — the backend broadcasts the event.
    const unlisten = onAccountsChanged(() => void refresh());
    return () => {
      void unlisten.then((stop) => stop());
    };
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="window">
  <header>
    <button
      class="tab"
      class:active={tab === "general"}
      onclick={() => (tab = "general")}
    >
      General
    </button>
    <button
      class="tab"
      class:active={tab === "accounts"}
      onclick={() => (tab = "accounts")}
    >
      Accounts
    </button>
    <button
      class="tab"
      class:active={tab === "signatures"}
      onclick={() => (tab = "signatures")}
    >
      Signatures
    </button>
    <button
      class="tab"
      class:active={tab === "notifications"}
      onclick={() => (tab = "notifications")}
    >
      Notifications
    </button>
    <button
      class="tab"
      class:active={tab === "swipes"}
      onclick={() => (tab = "swipes")}
    >
      Swipes
    </button>
    <button
      class="tab"
      class:active={tab === "privacy"}
      onclick={() => (tab = "privacy")}
    >
      Privacy
    </button>
  </header>

  {#if tab === "general"}
    <section class="content">
      <div class="section-label">Conversations</div>
      <div class="group">
        <div class="row">
          <label class="row-label" for="thread-order">Message order</label>
          <select
            id="thread-order"
            value={threadOrder}
            onchange={(e) =>
              void selectThreadOrder(e.currentTarget.value as ThreadOrder)}
          >
            <option value="newestLast">Newest at the bottom</option>
            <option value="newestFirst">Newest on top</option>
          </select>
        </div>
      </div>
      <p class="explain">
        How messages inside a conversation are ordered when you open it.
      </p>
    </section>
  {:else if tab === "accounts"}
    <AccountsPane
      {accounts}
      {aliases}
      onAdd={handleAdd}
      onDelete={handleDelete}
      onSetColor={handleSetColor}
      onAddAlias={handleAddAlias}
      onUpdateAlias={handleUpdateAlias}
      onDeleteAlias={handleDeleteAlias}
      onSetDefaultAlias={handleSetDefaultAlias}
    />
  {:else if tab === "signatures"}
    <SignaturesPane {accounts} />
  {:else if tab === "notifications"}
    <NotificationsPane {accounts} />
  {:else if tab === "swipes"}
    <section class="content">
      <div class="section-label">Swipe actions in the message list</div>
      <div class="group">
        {#each [{ side: "left", label: "Swipe left" }, { side: "right", label: "Swipe right" }] as const as row (row.side)}
          <div class="row">
            <label class="row-label" for="swipe-{row.side}">{row.label}</label>
            <select
              id="swipe-{row.side}"
              value={swipes[row.side]}
              onchange={(e) =>
                void selectSwipe(
                  row.side,
                  e.currentTarget.value as SwipeAction,
                )}
            >
              {#each SWIPE_OPTIONS as option (option.value)}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
          </div>
        {/each}
      </div>
      <p class="explain">
        What a two-finger swipe on a message row does. Pick None to turn a
        direction off.
      </p>
    </section>
  {:else}
    <section class="content">
      <div class="section-label">Remote images in messages</div>
      <div class="group">
        {#each POLICIES as option (option.value)}
          <div class="choice">
            <input
              type="radio"
              id="policy-{option.value}"
              name="remote-images"
              value={option.value}
              checked={policy === option.value}
              onchange={() => void selectPolicy(option.value)}
            />
            <span>
              <label for="policy-{option.value}">{option.label}</label>
              <small>{option.hint}</small>
            </span>
          </div>
        {/each}
      </div>
      <p class="explain">
        Remote images can tell the sender when, where and on what device a
        message was opened. Known tracking images are always removed. Images
        attached inside the message always show.
      </p>
    </section>
  {/if}
</div>

{#if lastError}
  <div class="error-banner" role="alert">
    <span>{lastError}</span>
    <button aria-label="Dismiss error" onclick={() => (lastError = null)}>
      ×
    </button>
  </div>
{/if}

<style>
  /* The grouped-settings canvas the white group boxes sit on. */
  .window {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: #f5f5f7;
  }

  @media (prefers-color-scheme: dark) {
    .window {
      background: #232326;
    }
  }

  /* macOS-preferences chrome: same gradient strip as the main toolbar,
     with the centered tab pills. */
  header {
    display: flex;
    justify-content: center;
    gap: 4px;
    flex-shrink: 0;
    padding: 8px 12px;
    background: linear-gradient(
      var(--bg-toolbar-top),
      var(--bg-toolbar-bottom)
    );
    border-bottom: 1px solid var(--border-chrome);
  }

  .tab {
    padding: 5px 14px;
    border: none;
    border-radius: 7px;
    background: none;
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text-secondary);
    cursor: default;
  }

  .tab:hover {
    background: var(--bg-hover);
  }

  .tab.active {
    background: var(--bg-selected-muted);
    color: var(--text-primary);
  }

  .content {
    padding: 20px 26px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .section-label {
    margin-bottom: 7px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  /* A white grouped box of setting rows. */
  .group {
    display: flex;
    flex-direction: column;
    max-width: 520px;
    padding: 3px 0;
    border: 1px solid var(--hairline);
    border-radius: 9px;
    background: var(--bg-window);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 8px 12px;
  }

  .row-label {
    font-size: 13px;
    font-weight: 500;
  }

  .row select {
    max-width: 220px;
    padding: 3px 8px;
    border: 1px solid var(--border-chrome);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    font-size: 12.5px;
  }

  .explain {
    margin: 8px 2px 0;
    max-width: 520px;
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .choice {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 9px 12px;
  }

  .choice label {
    font-size: 13px;
    font-weight: 600;
  }

  .choice small {
    display: block;
    margin-top: 1px;
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .error-banner {
    position: fixed;
    top: 0.75rem;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border: 1px solid #f0c0c0;
    border-radius: 0.375rem;
    background: #fdf1f1;
    color: #8a1f1f;
  }

  .error-banner button {
    border: none;
    background: none;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }
</style>
