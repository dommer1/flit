<script lang="ts">
  import type { ComposeDraft } from "./draft";
  import type { Account, OutgoingMessage } from "./types";

  let {
    accounts,
    draft,
    onSend,
    onCancel,
  }: {
    accounts: Account[];
    draft: ComposeDraft;
    onSend: (message: OutgoingMessage) => Promise<void> | void;
    onCancel: () => void;
  } = $props();

  // why: local copies, not $derived — the draft is only the starting point,
  // everything after that belongs to the user. The ignores acknowledge that
  // capturing just the initial prop value is exactly the intent here.
  // svelte-ignore state_referenced_locally
  let accountId = $state(draft.accountId);
  // svelte-ignore state_referenced_locally
  let to = $state(draft.to);
  // svelte-ignore state_referenced_locally
  let subject = $state(draft.subject);
  // svelte-ignore state_referenced_locally
  let body = $state(draft.body);

  let sending = $state(false);
  let error = $state<string | null>(null);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    sending = true;
    error = null;
    try {
      // why: the parent owns success (send + close); this form owns failure,
      // so the draft stays open and editable when the server says no.
      await onSend({ accountId, to, subject, body });
    } catch (err) {
      error = String(err);
    } finally {
      sending = false;
    }
  }
</script>

<div class="scrim">
  <form class="panel" aria-label="Compose message" onsubmit={submit}>
    <h2>{subject.trim() === "" ? "New Message" : subject}</h2>

    <label>
      From
      <select bind:value={accountId}>
        {#each accounts as account (account.id)}
          <option value={account.id}>{account.name} — {account.email}</option>
        {/each}
      </select>
    </label>
    <label>
      To
      <input bind:value={to} required placeholder="recipient@example.com" />
    </label>
    <label>
      Subject
      <input bind:value={subject} />
    </label>
    <textarea aria-label="Message body" bind:value={body}></textarea>

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}

    <footer>
      <button type="button" onclick={onCancel}>Cancel</button>
      <button type="submit" disabled={sending}>
        {sending ? "Sending…" : "Send"}
      </button>
    </footer>
  </form>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.25);
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: min(560px, calc(100vw - 80px));
    max-height: calc(100vh - 80px);
    padding: 16px 18px;
    border-radius: 10px;
    background: var(--bg-window);
    box-shadow:
      0 0 0 1px var(--hairline),
      0 18px 50px rgba(0, 0, 0, 0.3);
  }

  h2 {
    overflow: hidden;
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  input,
  select {
    flex: 1;
    padding: 5px 8px;
    border: 1px solid var(--hairline);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    color: var(--text-primary, inherit);
  }

  textarea {
    min-height: 180px;
    padding: 8px;
    border: 1px solid var(--hairline);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    line-height: 1.5;
    color: inherit;
    resize: vertical;
  }

  .error {
    margin: 0;
    font-size: 12px;
    color: #d9302c;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  button {
    padding: 5px 14px;
    border: 1px solid var(--hairline);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    cursor: pointer;
  }

  button[type="submit"] {
    background: var(--accent);
    border-color: var(--accent);
    color: #ffffff;
  }

  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
