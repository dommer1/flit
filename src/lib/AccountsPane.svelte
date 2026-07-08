<script lang="ts">
  import type { Account, NewAccount } from "./types";
  import AddAccountForm from "./AddAccountForm.svelte";

  let {
    accounts,
    onAdd,
    onDelete,
  }: {
    accounts: Account[];
    // why: resolves to the created account on success, null on failure — the
    // parent owns the api call and error display; the pane only needs to know
    // whether to leave the add form.
    onAdd: (account: NewAccount, password: string) => Promise<Account | null>;
    // why: hands over the whole account — the parent's confirmation dialog
    // needs the name and email, not just the id.
    onDelete: (account: Account) => void;
  } = $props();

  let selectedId = $state<number | null>(null);
  let adding = $state(false);

  // why: falls back to the first account when nothing was selected yet or the
  // selected account was just deleted from the list.
  let selected = $derived(
    accounts.find((a) => a.id === selectedId) ?? accounts[0] ?? null,
  );

  async function handleSubmit(account: NewAccount, password: string) {
    const created = await onAdd(account, password);
    if (created) {
      selectedId = created.id;
      adding = false;
    }
  }
</script>

<div class="pane">
  <aside>
    <ul>
      {#each accounts as account (account.id)}
        <li>
          <button
            class="account"
            class:active={!adding && selected?.id === account.id}
            onclick={() => {
              selectedId = account.id;
              adding = false;
            }}
          >
            <span class="name">{account.name}</span>
            <span class="email">{account.email}</span>
          </button>
        </li>
      {/each}
    </ul>
    <div class="actions">
      <button aria-label="Add account" onclick={() => (adding = true)}>
        +
      </button>
      <button
        aria-label="Delete account"
        disabled={adding || selected === null}
        onclick={() => selected && onDelete(selected)}
      >
        −
      </button>
    </div>
  </aside>

  <div class="detail">
    {#if adding}
      <AddAccountForm
        onSubmit={handleSubmit}
        onCancel={() => (adding = false)}
      />
    {:else if selected}
      <dl>
        <dt>Name</dt>
        <dd>{selected.name}</dd>
        <dt>Email</dt>
        <dd>{selected.email}</dd>
        <dt>IMAP server</dt>
        <dd>{selected.imapHost}:{selected.imapPort}</dd>
        <dt>SMTP server</dt>
        <dd>{selected.smtpHost}:{selected.smtpPort}</dd>
        <dt>Username</dt>
        <dd>{selected.username}</dd>
      </dl>
    {:else}
      <p class="empty">No accounts yet. Add one with +.</p>
    {/if}
  </div>
</div>

<style>
  .pane {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  aside {
    display: flex;
    flex-direction: column;
    width: 14rem;
    border-right: 1px solid #e5e5e5;
    background: #fafafa;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    flex: 1;
    margin: 0;
    padding: 0.5rem;
    list-style: none;
    overflow-y: auto;
  }

  .account {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.125rem;
    width: 100%;
    padding: 0.5rem 0.625rem;
    border: none;
    border-radius: 0.375rem;
    background: none;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .account:hover {
    background: rgba(0, 0, 0, 0.05);
  }

  .account.active {
    background: rgba(0, 0, 0, 0.08);
    font-weight: 600;
  }

  .email {
    font-size: 0.75rem;
    font-weight: 400;
    color: #666;
  }

  .actions {
    display: flex;
    gap: 0.25rem;
    padding: 0.375rem 0.5rem;
    border-top: 1px solid #e5e5e5;
  }

  .actions button {
    width: 1.5rem;
    height: 1.5rem;
    border: 1px solid #d4d4d4;
    border-radius: 0.25rem;
    background: #fff;
    font: inherit;
    line-height: 1;
    cursor: pointer;
  }

  .actions button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .detail {
    flex: 1;
    padding: 1.25rem;
    overflow-y: auto;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.5rem 1rem;
    margin: 0;
  }

  dt {
    color: #666;
  }

  dd {
    margin: 0;
  }

  .empty {
    color: #666;
  }
</style>
