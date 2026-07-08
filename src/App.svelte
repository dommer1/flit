<script lang="ts">
  import { onMount } from "svelte";
  import { listAccounts, listMessages } from "./lib/api";
  import type { Account, MessageHeader } from "./lib/types";
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

  async function selectAccount(accountId: number | null) {
    selectedAccountId = accountId;
    selectedMessageId = null;
    messages = await listMessages(accountId);
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
</style>
