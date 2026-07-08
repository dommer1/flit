<script lang="ts">
  import { onMount } from "svelte";
  import {
    addAccount,
    deleteAccount,
    listAccounts,
    listMessages,
  } from "./lib/api";
  import type { Account, MessageHeader, NewAccount } from "./lib/types";
  import AddAccountForm from "./lib/AddAccountForm.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import MessageList from "./lib/MessageList.svelte";
  import MessageView from "./lib/MessageView.svelte";

  let accounts = $state<Account[]>([]);
  let messages = $state<MessageHeader[]>([]);
  let selectedAccountId = $state<number | null>(null);
  let selectedMessageId = $state<number | null>(null);
  let showAddForm = $state(false);
  let lastError = $state<string | null>(null);

  let selectedMessage = $derived(
    messages.find((m) => m.id === selectedMessageId) ?? null,
  );

  async function selectAccount(accountId: number | null) {
    selectedAccountId = accountId;
    selectedMessageId = null;
    messages = await listMessages(accountId);
  }

  async function handleAddAccount(account: NewAccount, password: string) {
    lastError = null;
    try {
      const created = await addAccount(account, password);
      accounts = [...accounts, created];
      showAddForm = false;
    } catch (err) {
      lastError = String(err);
    }
  }

  async function handleDeleteAccount(id: number) {
    lastError = null;
    try {
      await deleteAccount(id);
      accounts = accounts.filter((a) => a.id !== id);
      if (selectedAccountId === id) await selectAccount(null);
    } catch (err) {
      lastError = String(err);
    }
  }

  onMount(async () => {
    accounts = await listAccounts();
    await selectAccount(null);
  });
</script>

<div class="layout">
  <aside>
    <Sidebar
      {accounts}
      selectedId={selectedAccountId}
      onSelect={selectAccount}
      onAdd={() => (showAddForm = true)}
      onDelete={handleDeleteAccount}
    />
  </aside>
  <section class="list">
    <MessageList
      {messages}
      selectedId={selectedMessageId}
      onSelect={(id) => (selectedMessageId = id)}
    />
  </section>
  <section class="view">
    <MessageView message={selectedMessage} />
  </section>
</div>

{#if lastError}
  <div class="error-banner" role="alert">
    <span>{lastError}</span>
    <button aria-label="Dismiss error" onclick={() => (lastError = null)}>
      ×
    </button>
  </div>
{/if}

{#if showAddForm}
  <div class="overlay">
    <AddAccountForm
      onSubmit={handleAddAccount}
      onCancel={() => (showAddForm = false)}
    />
  </div>
{/if}

<style>
  :global(body) {
    margin: 0;
    font-family: system-ui, sans-serif;
    font-size: 0.875rem;
    color: #1a1a1a;
  }

  .layout {
    display: grid;
    grid-template-columns: 13rem 22rem 1fr;
    height: 100vh;
  }

  aside {
    border-right: 1px solid #e5e5e5;
    background: #fafafa;
    overflow-y: auto;
  }

  .list {
    border-right: 1px solid #e5e5e5;
    overflow-y: auto;
  }

  .view {
    display: flex;
    flex-direction: column;
    overflow-y: auto;
  }

  .overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.25);
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
