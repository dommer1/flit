<script lang="ts">
  import { onMount } from "svelte";
  import {
    addAccount,
    closeSettings,
    confirmAccountDeletion,
    deleteAccount,
    getRemoteImagePolicy,
    getSwipeActions,
    listAccounts,
    onAccountsChanged,
    setAccountColor,
    setRemoteImagePolicy,
    setSwipeActions,
    testConnection,
  } from "./api";
  import { DEFAULT_SWIPE_ACTIONS } from "./swipe";
  import type {
    Account,
    NewAccount,
    RemoteImagePolicy,
    SwipeAction,
    SwipeActions,
  } from "./types";
  import AccountsPane from "./AccountsPane.svelte";
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
  let lastError = $state<string | null>(null);
  let tab = $state<"accounts" | "signatures" | "swipes" | "privacy">(
    "accounts",
  );
  let policy = $state<RemoteImagePolicy>("ask");
  let swipes = $state<SwipeActions>(DEFAULT_SWIPE_ACTIONS);

  async function refresh() {
    accounts = await listAccounts();
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

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") void closeSettings();
  }

  onMount(() => {
    void refresh();
    void getRemoteImagePolicy().then((stored) => (policy = stored));
    void getSwipeActions().then((stored) => (swipes = stored));
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

  {#if tab === "accounts"}
    <AccountsPane
      {accounts}
      onAdd={handleAdd}
      onDelete={handleDelete}
      onSetColor={handleSetColor}
    />
  {:else if tab === "signatures"}
    <SignaturesPane {accounts} />
  {:else if tab === "swipes"}
    <section class="swipes">
      <fieldset>
        <legend>Swipe actions in the message list</legend>
        <p class="explain">
          What a two-finger swipe on a message row does. Pick None to turn a
          direction off.
        </p>
        {#each [{ side: "left", label: "Swipe left" }, { side: "right", label: "Swipe right" }] as const as row (row.side)}
          <div class="swipe-setting">
            <label for="swipe-{row.side}">{row.label}</label>
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
      </fieldset>
    </section>
  {:else}
    <section class="privacy">
      <fieldset>
        <legend>Remote images in messages</legend>
        <p class="explain">
          Remote images can tell the sender when, where and on what device a
          message was opened. Known tracking images are always removed.
          Images attached inside the message always show.
        </p>
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
      </fieldset>
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
  .window {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  header {
    display: flex;
    justify-content: center;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #e5e5e5;
  }

  .tab {
    padding: 0.375rem 0.75rem;
    border: none;
    border-radius: 0.375rem;
    background: none;
    font: inherit;
    cursor: pointer;
  }

  .tab.active {
    background: rgba(0, 0, 0, 0.08);
    font-weight: 600;
  }

  .privacy,
  .swipes {
    padding: 1rem 1.25rem;
    overflow-y: auto;
  }

  .swipe-setting {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.375rem 0;
  }

  .swipe-setting label {
    width: 6.5rem;
  }

  .swipe-setting select {
    min-width: 11rem;
    font: inherit;
  }

  fieldset {
    margin: 0;
    padding: 0;
    border: none;
  }

  legend {
    padding: 0;
    font-weight: 600;
  }

  .explain {
    margin: 0.375rem 0 0.75rem;
    max-width: 34rem;
    font-size: 0.8125rem;
    color: #6e6e73;
  }

  .choice {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    padding: 0.25rem 0;
  }

  .choice label {
    cursor: pointer;
  }

  .choice small {
    display: block;
    font-size: 0.75rem;
    color: #6e6e73;
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
