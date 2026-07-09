<script lang="ts">
  import type { Account } from "./types";

  let {
    accounts,
    selectedId,
    onSelect,
  }: {
    accounts: Account[];
    selectedId: number | null;
    onSelect: (id: number | null) => void;
  } = $props();
</script>

<nav>
  <p class="section">Favorites</p>
  <button
    class="row"
    class:active={selectedId === null}
    onclick={() => onSelect(null)}
  >
    <svg class="icon" viewBox="0 0 16 16" aria-hidden="true">
      <path
        d="M1.5 8.5 3 3.8A1.2 1.2 0 0 1 4.14 3h7.72A1.2 1.2 0 0 1 13 3.8l1.5 4.7v2.8a1.5 1.5 0 0 1-1.5 1.5H3a1.5 1.5 0 0 1-1.5-1.5Zm0 0h3.4a.6.6 0 0 1 .54.34l.32.66a.6.6 0 0 0 .54.34h3.4a.6.6 0 0 0 .54-.34l.32-.66a.6.6 0 0 1 .54-.34h3.4"
        fill="none"
        stroke="currentColor"
        stroke-width="1.2"
        stroke-linejoin="round"
      />
    </svg>
    <span class="label">All Inboxes</span>
  </button>

  {#if accounts.length > 0}
    <p class="section">Accounts</p>
  {/if}

  {#each accounts as account (account.id)}
    <button
      class="row"
      class:active={selectedId === account.id}
      onclick={() => onSelect(account.id)}
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true">
        <rect
          x="1.75"
          y="3.25"
          width="12.5"
          height="9.5"
          rx="1.5"
          fill="none"
          stroke="currentColor"
          stroke-width="1.2"
        />
        <path
          d="m2.5 4.5 5.5 4 5.5-4"
          fill="none"
          stroke="currentColor"
          stroke-width="1.2"
          stroke-linejoin="round"
        />
      </svg>
      <span class="label" title={account.email}>
        <span class="name">{account.name}</span>
        <span class="email">{account.email}</span>
      </span>
    </button>
  {/each}
</nav>

<style>
  nav {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 8px 10px;
  }

  .section {
    margin: 10px 0 3px;
    padding: 0 8px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
  }

  .section:first-child {
    margin-top: 2px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 4px 9px;
    min-height: 30px;
    border: none;
    border-radius: 8px;
    background: none;
    font: inherit;
    color: inherit;
    text-align: left;
    cursor: default;
  }

  .row:hover {
    background: var(--bg-hover);
  }

  .row.active {
    background: var(--bg-selected-muted);
  }

  .icon {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    color: var(--accent);
  }

  .label {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .label > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .email {
    font-size: 11px;
    color: var(--text-secondary);
  }
</style>
