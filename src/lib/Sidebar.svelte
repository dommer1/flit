<script lang="ts">
  import type { Account } from "./types";

  let {
    accounts,
    selectedId,
    onSelect,
    onAdd,
    onDelete,
    onOpenSettings,
  }: {
    accounts: Account[];
    selectedId: number | null;
    onSelect: (id: number | null) => void;
    onAdd: () => void;
    onDelete: (id: number) => void;
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
    <div class="account">
      <button
        class="row"
        class:active={selectedId === account.id}
        onclick={() => onSelect(account.id)}
      >
        <span class="name">{account.name}</span>
        <span class="email">{account.email}</span>
      </button>
      <button
        class="remove"
        aria-label={`Delete ${account.name}`}
        onclick={() => onDelete(account.id)}
      >
        ×
      </button>
    </div>
  {/each}

  <button class="add" onclick={onAdd}>+ Add account</button>

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
    flex: 1;
    min-width: 0;
    padding: 0.5rem 0.625rem;
  }

  .row:hover {
    background: rgba(0, 0, 0, 0.05);
  }

  .row.active {
    background: rgba(0, 0, 0, 0.08);
    font-weight: 600;
  }

  .account {
    display: flex;
    align-items: center;
    gap: 0.125rem;
  }

  /* why: opacity (not visibility/display) — stays clickable for tests and
     assistive tech while visually appearing only on hover or focus. */
  .remove {
    opacity: 0;
    padding: 0.25rem 0.45rem;
    color: #888;
  }

  .account:hover .remove,
  .remove:focus-visible {
    opacity: 1;
  }

  .remove:hover {
    background: rgba(0, 0, 0, 0.08);
    color: #1a1a1a;
  }

  .add {
    padding: 0.5rem 0.625rem;
    color: #666;
    font-size: 0.75rem;
  }

  .add:hover {
    background: rgba(0, 0, 0, 0.05);
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
