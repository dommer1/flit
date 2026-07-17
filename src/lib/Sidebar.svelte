<script lang="ts">
  import type { Account, Mailbox, ScheduledMessage } from "./types";

  let {
    accounts,
    mailboxes,
    selectedAccountId,
    selectedMailbox,
    onSelect,
    scheduled,
    onCancelScheduled,
  }: {
    accounts: Account[];
    mailboxes: Record<number, Mailbox[]>;
    selectedAccountId: number | null;
    selectedMailbox: string;
    onSelect: (accountId: number | null, mailbox?: string) => void;
    /** Pending send-later messages; the section hides when empty. */
    scheduled: ScheduledMessage[];
    onCancelScheduled: (id: number) => void;
  } = $props();

  const EXPANDED_KEY = "flit.sidebar.expanded";

  function loadExpanded(): Record<number, boolean> {
    try {
      return JSON.parse(localStorage.getItem(EXPANDED_KEY) ?? "{}");
    } catch {
      return {};
    }
  }

  let expanded = $state<Record<number, boolean>>(loadExpanded());

  function toggle(accountId: number) {
    expanded[accountId] = !expanded[accountId];
    localStorage.setItem(EXPANDED_KEY, JSON.stringify(expanded));
  }

  // The account row itself is the inbox, so the sublist holds the rest.
  function folders(accountId: number): Mailbox[] {
    return (mailboxes[accountId] ?? []).filter((m) => m.role !== "inbox");
  }

  function scheduledTitle(entry: ScheduledMessage): string {
    const subject = entry.subject.trim();
    return subject === "" ? "(No subject)" : subject;
  }

  function scheduledWhen(entry: ScheduledMessage): string {
    return new Date(entry.scheduledAt * 1000).toLocaleString(undefined, {
      weekday: "short",
      day: "numeric",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
</script>

<nav>
  <p class="section">Favorites</p>
  <button
    class="row"
    class:active={selectedAccountId === null}
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
    <!-- One visual row, two click targets: the name selects the inbox, the
         chevron folds the folders — the wrapper carries the shared
         hover/active highlight so they never read as two pieces. -->
    <div
      class="account-row"
      class:active={selectedAccountId === account.id &&
        selectedMailbox === "INBOX"}
    >
      <button class="row grow" onclick={() => onSelect(account.id)}>
        <span
          class="color-dot"
          aria-hidden="true"
          style:background={account.color ?? undefined}
        ></span>
        <span class="label" title={account.email}>
          <span class="name">{account.name}</span>
          <span class="email">{account.email}</span>
        </span>
      </button>
      {#if folders(account.id).length > 0}
        <button
          class="chevron"
          class:open={expanded[account.id]}
          aria-label={`Toggle folders for ${account.name}`}
          aria-expanded={!!expanded[account.id]}
          onclick={() => toggle(account.id)}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path
              d="m6 4 4 4-4 4"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        </button>
      {/if}
    </div>
    {#if expanded[account.id]}
      {#each folders(account.id) as mailbox (mailbox.id)}
        <button
          class="row folder"
          class:active={selectedAccountId === account.id &&
            selectedMailbox === mailbox.name}
          onclick={() => onSelect(account.id, mailbox.name)}
        >
          <span class="label">{mailbox.displayName}</span>
        </button>
      {/each}
    {/if}
  {/each}

  {#if scheduled.length > 0}
    <!-- Send-later messages waiting in the local schedule. Not a folder —
         the rows live only in SQLite until the scheduler delivers them. -->
    <p class="section">Scheduled</p>
    <ul class="scheduled" aria-label="Scheduled messages">
      {#each scheduled as entry (entry.id)}
        <li class="row">
          <span class="label">
            <span class="name">{scheduledTitle(entry)}</span>
            <span class="email">{scheduledWhen(entry)}</span>
          </span>
          <button
            class="cancel"
            aria-label={`Cancel scheduled ${scheduledTitle(entry)}`}
            title="Cancel — reopens as a draft"
            onclick={() => onCancelScheduled(entry.id)}
          >
            ×
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</nav>

<style>
  nav {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 14px 10px;
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

  .account-row {
    display: flex;
    align-items: center;
    gap: 2px;
    padding-right: 6px;
    border-radius: 8px;
  }

  .account-row:hover {
    background: var(--bg-hover);
  }

  .account-row.active {
    background: var(--bg-selected-muted);
  }

  /* Inside the shared highlight the inner buttons stay quiet. */
  .account-row .row:hover,
  .account-row .chevron:hover {
    background: none;
  }

  .grow {
    flex: 1;
    min-width: 0;
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

  /* Indented under the account row, aligned with its label. */
  .folder {
    margin-left: 24px;
    min-height: 26px;
    padding: 2px 9px;
  }

  .chevron {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--text-tertiary);
    cursor: default;
  }

  .chevron:hover {
    background: var(--bg-hover);
  }

  .chevron svg {
    width: 12px;
    height: 12px;
    transition: transform 0.15s ease;
  }

  .chevron.open svg {
    transform: rotate(90deg);
  }

  .icon {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    color: var(--accent);
  }

  /* The account's identity color (settings swatch); gray when unset. */
  .color-dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    margin: 0 3px;
    border-radius: 50%;
    background: var(--text-tertiary);
  }

  .label {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    font-weight: 500;
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

  .scheduled {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .scheduled .label {
    flex: 1;
    min-width: 0;
  }

  .cancel {
    flex-shrink: 0;
    display: none;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: 5px;
    background: none;
    font-size: 13px;
    line-height: 1;
    color: var(--text-secondary);
    cursor: default;
  }

  .scheduled .row:hover .cancel {
    display: grid;
  }

  .cancel:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
</style>
