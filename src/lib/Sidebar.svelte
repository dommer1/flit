<script lang="ts">
  import type { Account } from "./types";

  let {
    accounts,
    selectedId,
    onSelect,
    onOpenSettings,
  }: {
    accounts: Account[];
    selectedId: number | null;
    onSelect: (id: number | null) => void;
    onOpenSettings: () => void;
  } = $props();
</script>

<nav>
  <button
    class="row"
    class:active={selectedId === null}
    onclick={() => onSelect(null)}
  >
    All Inboxes
  </button>

  {#each accounts as account (account.id)}
    <button
      class="row"
      class:active={selectedId === account.id}
      onclick={() => onSelect(account.id)}
    >
      <span class="name">{account.name}</span>
      <span class="email">{account.email}</span>
    </button>
  {/each}

  <button class="settings" onclick={onOpenSettings}>Settings</button>
</nav>

<style>
  nav {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    box-sizing: border-box;
    height: 100%;
    padding: 0.5rem;
  }

  button {
    border: none;
    border-radius: 0.375rem;
    background: none;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .row {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.125rem;
    padding: 0.5rem 0.625rem;
  }

  .row:hover {
    background: rgba(0, 0, 0, 0.05);
  }

  .row.active {
    background: rgba(0, 0, 0, 0.08);
    font-weight: 600;
  }

  .settings {
    margin-top: auto;
    padding: 0.5rem 0.625rem;
    color: #666;
    font-size: 0.75rem;
  }

  .settings:hover {
    background: rgba(0, 0, 0, 0.05);
  }

  .email {
    font-size: 0.75rem;
    color: #666;
  }
</style>
