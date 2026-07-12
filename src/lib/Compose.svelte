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
    <!-- Canary-style toolbar: a tinted strip with icon actions, send at the
         far right as a paper plane. -->
    <header class="toolbar">
      <button
        type="button"
        class="icon"
        aria-label="Cancel"
        title="Cancel"
        onclick={onCancel}
      >
        <svg
          viewBox="0 0 24 24"
          width="17"
          height="17"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
        >
          <path d="M18 6 6 18M6 6l12 12" />
        </svg>
      </button>
      <button
        type="submit"
        class="icon send"
        aria-label={sending ? "Sending…" : "Send"}
        title="Send"
        disabled={sending}
      >
        <svg
          viewBox="0 0 24 24"
          width="18"
          height="18"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M22 2 11 13" />
          <path d="M22 2 15 22l-4-9-9-4z" />
        </svg>
      </button>
    </header>

    <!-- Canary-style envelope fields: quiet label + borderless input rows
         divided by hairlines, subject as a bold standalone line. -->
    <div class="row">
      <span class="key" aria-hidden="true">To:</span>
      <input
        aria-label="To"
        bind:value={to}
        required
        placeholder="recipient@example.com"
      />
    </div>
    <div class="row">
      <span class="key" aria-hidden="true">From:</span>
      <select aria-label="From" bind:value={accountId}>
        {#each accounts as account (account.id)}
          <option value={account.id}>{account.email}</option>
        {/each}
      </select>
      <span class="chevron" aria-hidden="true">⌄</span>
    </div>
    <div class="row subject-row">
      <input
        class="subject"
        aria-label="Subject"
        bind:value={subject}
        placeholder="Subject"
      />
    </div>

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}

    <textarea aria-label="Message body" bind:value={body}></textarea>
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
    width: min(640px, calc(100vw - 80px));
    height: min(680px, calc(100vh - 80px));
    border-radius: 12px;
    background: var(--bg-window);
    box-shadow:
      0 0 0 1px var(--hairline),
      0 18px 50px rgba(0, 0, 0, 0.3);
    overflow: hidden;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
    padding: 10px 14px;
    /* Canary's pale periwinkle strip, derived from the accent so it holds
       up in dark mode too. */
    background: color-mix(in srgb, var(--accent) 16%, var(--bg-window));
  }

  .icon {
    display: grid;
    place-items: center;
    padding: 5px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .icon:hover {
    background: var(--bg-hover);
  }

  .icon.send {
    color: var(--accent);
  }

  .icon:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    margin: 0 20px;
    padding: 10px 0;
    border-bottom: 1px solid var(--hairline);
  }

  .key {
    flex-shrink: 0;
    font-size: 13px;
    color: var(--text-secondary);
  }

  input,
  select {
    border: none;
    padding: 0;
    background: transparent;
    font: inherit;
    font-size: 13px;
    color: var(--text-primary);
  }

  input {
    flex: 1;
  }

  input:focus,
  select:focus,
  textarea:focus {
    outline: none;
  }

  input::placeholder {
    color: var(--text-tertiary);
  }

  /* Borderless account switcher: plain text with a small chevron, like
     Canary's From line. */
  select {
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
  }

  .chevron {
    font-size: 11px;
    line-height: 1;
    color: var(--text-secondary);
    transform: translateY(-2px);
  }

  .subject {
    font-weight: 600;
    font-size: 14px;
  }

  .error {
    flex-shrink: 0;
    margin: 0;
    padding: 8px 20px;
    border-bottom: 1px solid var(--hairline);
    font-size: 12px;
    color: #d9302c;
  }

  textarea {
    flex: 1;
    padding: 14px 20px;
    border: none;
    background: transparent;
    font: inherit;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-primary);
    resize: none;
  }
</style>
