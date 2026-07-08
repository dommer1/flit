<script lang="ts">
  import { onMount } from "svelte";
  import {
    addAccount,
    closeSettings,
    deleteAccount,
    listAccounts,
    onAccountsChanged,
  } from "./api";
  import type { Account, NewAccount } from "./types";
  import AccountsPane from "./AccountsPane.svelte";

  let accounts = $state<Account[]>([]);
  let lastError = $state<string | null>(null);

  async function refresh() {
    accounts = await listAccounts();
  }

  async function handleAdd(
    account: NewAccount,
    password: string,
  ): Promise<Account | null> {
    lastError = null;
    try {
      const created = await addAccount(account, password);
      await refresh();
      return created;
    } catch (err) {
      lastError = String(err);
      return null;
    }
  }

  async function handleDelete(id: number) {
    lastError = null;
    try {
      await deleteAccount(id);
      await refresh();
    } catch (err) {
      lastError = String(err);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") void closeSettings();
  }

  onMount(() => {
    void refresh();
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
    <button class="tab active">Accounts</button>
  </header>

  <AccountsPane {accounts} onAdd={handleAdd} onDelete={handleDelete} />
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
