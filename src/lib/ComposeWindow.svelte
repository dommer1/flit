<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    closeCompose,
    listAccounts,
    sendMessage,
    takeComposeDraft,
  } from "./api";
  import type { Account } from "./types";

  /** How long a send can still be undone before it really leaves. */
  const UNDO_SECONDS = 8;
  /** How long the "Message sent" confirmation stays before auto-close. */
  const SENT_CLOSE_MS = 1200;

  let accounts = $state<Account[]>([]);
  let accountId = $state<number | null>(null);
  let to = $state("");
  let subject = $state("");
  let body = $state("");

  // editing → countdown (undoable) → sending → sent; a failure or an undo
  // drops back to editing with the draft intact.
  let phase = $state<"editing" | "countdown" | "sending" | "sent">("editing");
  let secondsLeft = $state(UNDO_SECONDS);
  let error = $state<string | null>(null);

  let countdownTimer: ReturnType<typeof setInterval> | undefined;
  onDestroy(() => clearInterval(countdownTimer));

  onMount(() => {
    void (async () => {
      const [loadedAccounts, draft] = await Promise.all([
        listAccounts(),
        takeComposeDraft(),
      ]);
      accounts = loadedAccounts;
      // why: a null draft (webview reload after pickup) degrades to a blank
      // message from the first account instead of a broken window.
      accountId = draft?.accountId ?? accounts[0]?.id ?? null;
      to = draft?.to ?? "";
      subject = draft?.subject ?? "";
      body = draft?.body ?? "";
    })();
  });

  // why: submit only arms the countdown — nothing touches the network until
  // it runs out, so Undo is a plain timer cancel, never a message recall.
  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (phase !== "editing" || accountId === null) return;
    error = null;
    phase = "countdown";
    secondsLeft = UNDO_SECONDS;
    countdownTimer = setInterval(() => {
      secondsLeft -= 1;
      if (secondsLeft <= 0) {
        clearInterval(countdownTimer);
        void send();
      }
    }, 1000);
  }

  function undoSend() {
    clearInterval(countdownTimer);
    phase = "editing";
  }

  async function send() {
    if (accountId === null) return;
    phase = "sending";
    try {
      await sendMessage({ accountId, to, subject, body });
      // why: success is what closes the window — after a short confirmation
      // beat; a rejection drops back to the editable draft.
      phase = "sent";
      setTimeout(() => void closeCompose(), SENT_CLOSE_MS);
    } catch (err) {
      error = String(err);
      phase = "editing";
    }
  }
</script>

<form class="window" aria-label="Compose message" onsubmit={submit}>
  <!-- Canary-style toolbar: a tinted strip that hosts the native traffic
       lights (title bar overlay) and doubles as the window drag handle. -->
  <header class="toolbar" data-tauri-drag-region>
    <button
      type="button"
      class="icon"
      aria-label="Cancel"
      title="Cancel"
      onclick={() => void closeCompose()}
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
    {#if phase === "countdown"}
      <button type="button" class="undo" onclick={undoSend}>
        Undo ({secondsLeft}s)
      </button>
    {:else}
      <button
        type="submit"
        class="icon send"
        aria-label={phase === "sending" ? "Sending…" : "Send"}
        title="Send"
        disabled={phase !== "editing" || accountId === null}
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
    {/if}
  </header>

  <!-- Canary-style envelope fields: quiet label + borderless input rows
       divided by hairlines, subject as a bold standalone line. -->
  <div class="row">
    <span class="key" aria-hidden="true">To:</span>
    <input
      aria-label="To"
      bind:value={to}
      required
      disabled={phase !== "editing"}
      placeholder="recipient@example.com"
    />
  </div>
  <div class="row">
    <span class="key" aria-hidden="true">From:</span>
    <select
      aria-label="From"
      bind:value={accountId}
      disabled={phase !== "editing"}
    >
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
      disabled={phase !== "editing"}
      placeholder="Subject"
    />
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
  {#if phase === "sent"}
    <p class="status" role="status">Message sent</p>
  {/if}

  <textarea
    aria-label="Message body"
    bind:value={body}
    disabled={phase !== "editing"}
  ></textarea>
</form>

<style>
  .window {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg-window);
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
    /* why the left padding: the title bar is an overlay, so the macOS
       traffic lights sit on this strip — the ✕ must clear them. */
    padding: 10px 14px 10px 84px;
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

  /* The undo pill takes the send icon's spot while the countdown runs. */
  .undo {
    padding: 4px 12px;
    border: none;
    border-radius: 100px;
    background: var(--accent);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--accent-text);
    cursor: pointer;
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

  .status {
    flex-shrink: 0;
    margin: 0;
    padding: 8px 20px;
    border-bottom: 1px solid var(--hairline);
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
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
