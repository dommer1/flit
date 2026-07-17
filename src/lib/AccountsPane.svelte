<script lang="ts">
  import { ACCOUNT_COLORS } from "./accountColors";
  import type { Account, NewAccount } from "./types";
  import AddAccountForm from "./AddAccountForm.svelte";

  let {
    accounts,
    onAdd,
    onDelete,
    onSetColor,
  }: {
    accounts: Account[];
    // why: resolves to the created account on success, null on failure — the
    // parent owns the api call and error display; the pane only needs to know
    // whether to leave the add form.
    onAdd: (account: NewAccount, password: string) => Promise<Account | null>;
    // why: hands over the whole account — the parent's confirmation dialog
    // needs the name and email, not just the id.
    onDelete: (account: Account) => void;
    // why: `null` clears the color; the parent owns the api call + refresh.
    onSetColor: (id: number, color: string | null) => void;
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
            <span
              class="dot"
              aria-hidden="true"
              style:background={account.color ?? undefined}
            ></span>
            <span class="text">
              <span class="name">{account.name}</span>
              <span class="email">{account.email}</span>
            </span>
            {#if account.lastError}
              <span class="warning" title="Connection problem"></span>
            {/if}
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
        <dt>Status</dt>
        <dd>
          {#if selected.lastError}
            <span class="status broken">{selected.lastError}</span>
          {:else if selected.checkedAt !== null}
            <span class="status ok">Connected</span>
            <span class="checked-at">
              checked {new Date(selected.checkedAt * 1000).toLocaleString()}
            </span>
          {:else}
            <span class="status unknown">Not checked yet</span>
          {/if}
        </dd>
        <dt>Color</dt>
        <dd class="colors">
          <button
            class="swatch none"
            class:selected={!selected.color}
            aria-label="No color"
            title="No color"
            onclick={() => onSetColor(selected.id, null)}
          ></button>
          {#each ACCOUNT_COLORS as option (option.value)}
            <button
              class="swatch"
              class:selected={selected.color === option.value}
              style:background={option.value}
              aria-label={option.name}
              title={option.name}
              onclick={() => onSetColor(selected.id, option.value)}
            ></button>
          {/each}
        </dd>
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
    width: 200px;
    flex-shrink: 0;
    border-right: 1px solid var(--hairline);
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    margin: 0;
    padding: 12px 8px;
    list-style: none;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .account {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 9px;
    border: none;
    border-radius: 7px;
    background: none;
    font: inherit;
    text-align: left;
    cursor: default;
  }

  .account:hover {
    background: var(--bg-hover);
  }

  /* The selected account fills with the accent, like the design. */
  .account.active {
    background: var(--accent);
    color: var(--accent-text);
  }

  /* The account's identity color (the swatch below); gray when unset. */
  .dot {
    flex-shrink: 0;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--text-tertiary);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .text > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-size: 12.5px;
    font-weight: 600;
  }

  .email {
    font-size: 10.5px;
    color: var(--text-secondary);
  }

  .active .email {
    color: inherit;
    opacity: 0.7;
  }

  /* Connection trouble marker on the row — details live in the status. */
  .warning {
    flex-shrink: 0;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #f5a623;
  }

  .status::before {
    content: "● ";
  }

  .status.ok::before {
    color: #34c759;
  }

  .status.broken {
    color: #d9302c;
  }

  .status.unknown::before {
    color: var(--text-tertiary);
  }

  .checked-at {
    margin-left: 6px;
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .actions {
    display: flex;
    gap: 4px;
    padding: 4px 10px 10px;
  }

  .actions button {
    width: 26px;
    height: 24px;
    border: 1px solid var(--border-chrome);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    font-size: 14px;
    line-height: 1;
    color: var(--text-primary);
    cursor: default;
  }

  .actions button:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .actions button:disabled {
    opacity: 0.4;
  }

  .detail {
    flex: 1;
    padding: 20px 24px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* Stacked field-label-over-value rows, like the design's form. */
  dl {
    margin: 0;
  }

  dt {
    margin-bottom: 3px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  dd {
    margin: 0 0 13px;
    font-size: 13px;
  }

  .colors {
    display: flex;
    flex-wrap: wrap;
    gap: 0.375rem;
  }

  .swatch {
    width: 20px;
    height: 20px;
    padding: 0;
    border: 1px solid rgba(0, 0, 0, 0.15);
    border-radius: 50%;
    cursor: default;
  }

  /* The selected swatch gets a ring: a canvas-colored gap, then the accent. */
  .swatch.selected {
    box-shadow:
      0 0 0 2px #f5f5f7,
      0 0 0 3.5px var(--accent);
  }

  @media (prefers-color-scheme: dark) {
    .swatch.selected {
      box-shadow:
        0 0 0 2px #232326,
        0 0 0 3.5px var(--accent);
    }
  }

  .swatch.none {
    position: relative;
    background: #fff;
  }

  /* A red diagonal marks the "no color" swatch. */
  .swatch.none::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: linear-gradient(
      to top right,
      transparent 45%,
      #d9302c 45%,
      #d9302c 55%,
      transparent 55%
    );
  }

  .empty {
    color: #666;
  }
</style>
