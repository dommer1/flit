<script lang="ts">
  import { onMount } from "svelte";
  import {
    closeCompose,
    inspectAttachments,
    listAccounts,
    onFileDrop,
    queueSend,
    takeComposeDraft,
  } from "./api";
  import type { Account, AttachmentInfo } from "./types";

  let accounts = $state<Account[]>([]);
  let accountId = $state<number | null>(null);
  let to = $state("");
  let cc = $state("");
  let bcc = $state("");
  // why: Cc/Bcc rows stay hidden until asked for (or the draft carries
  // them) so the default envelope stays as quiet as Apple Mail's.
  let showCcBcc = $state(false);
  let subject = $state("");
  let body = $state("");
  let attachments = $state<AttachmentInfo[]>([]);
  /** A file drag is currently above the window — shows the drop overlay. */
  let dropHover = $state(false);

  let queueing = $state(false);
  let error = $state<string | null>(null);

  onMount(() => {
    let unlisten: (() => void) | undefined;
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
      cc = draft?.cc ?? "";
      bcc = draft?.bcc ?? "";
      showCcBcc = cc !== "" || bcc !== "";
      subject = draft?.subject ?? "";
      body = draft?.body ?? "";
      // why re-stat instead of trusting the draft: an undone/failed send may
      // reopen after the file changed or vanished — surface that now.
      if (draft?.attachments?.length) {
        await addAttachments(draft.attachments.map((a) => a.path));
      }
      unlisten = await onFileDrop({
        onHover: (hovering) => (dropHover = hovering),
        onDrop: (paths) => void addAttachments(paths),
      });
    })();
    return () => unlisten?.();
  });

  async function addAttachments(paths: string[]) {
    if (queueing) return;
    try {
      const infos = await inspectAttachments(paths);
      const fresh = infos.filter(
        (info) => !attachments.some((a) => a.path === info.path),
      );
      attachments = [...attachments, ...fresh];
      error = null;
    } catch (err) {
      error = String(err);
    }
  }

  function removeAttachment(path: string) {
    attachments = attachments.filter((a) => a.path !== path);
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  // why: the backend queue owns the undo window — this window only hands
  // the message over (which validates addresses) and closes. The undo badge
  // lives in the main window from here on.
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (queueing || accountId === null) return;
    queueing = true;
    error = null;
    try {
      await queueSend({
        accountId,
        to,
        cc,
        bcc,
        subject,
        body,
        attachments: attachments.map(({ path, name }) => ({ path, name })),
      });
      await closeCompose();
    } catch (err) {
      error = String(err);
    } finally {
      queueing = false;
    }
  }
</script>

<form
  class="window"
  class:drop-target={dropHover}
  aria-label="Compose message"
  onsubmit={submit}
>
  <!-- Canary-style toolbar: a tinted strip that hosts the native traffic
       lights (title bar overlay) and doubles as the window drag handle.
       Closing goes through the red traffic light — no extra ✕ here. -->
  <header class="toolbar" data-tauri-drag-region>
    <button
      type="submit"
      class="icon send"
      aria-label="Send"
      title="Send"
      disabled={queueing || accountId === null}
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
      disabled={queueing}
      placeholder="recipient@example.com"
    />
    {#if !showCcBcc}
      <button
        type="button"
        class="reveal"
        onclick={() => (showCcBcc = true)}
        disabled={queueing}
      >
        Cc/Bcc
      </button>
    {/if}
  </div>
  {#if showCcBcc}
    <div class="row">
      <span class="key" aria-hidden="true">Cc:</span>
      <input aria-label="Cc" bind:value={cc} disabled={queueing} />
    </div>
    <div class="row">
      <span class="key" aria-hidden="true">Bcc:</span>
      <input aria-label="Bcc" bind:value={bcc} disabled={queueing} />
    </div>
  {/if}
  <div class="row">
    <span class="key" aria-hidden="true">From:</span>
    <select
      aria-label="From"
      bind:value={accountId}
      disabled={queueing}
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
      disabled={queueing}
      placeholder="Subject"
    />
  </div>

  {#if attachments.length > 0}
    <ul class="attachments" aria-label="Attachments">
      {#each attachments as attachment (attachment.path)}
        <li class="chip">
          <span class="chip-name">{attachment.name}</span>
          <span class="chip-size">{formatSize(attachment.size)}</span>
          <button
            type="button"
            class="chip-remove"
            aria-label={`Remove ${attachment.name}`}
            onclick={() => removeAttachment(attachment.path)}
            disabled={queueing}
          >
            ×
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  <textarea
    aria-label="Message body"
    bind:value={body}
    disabled={queueing}
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
    justify-content: flex-end;
    flex-shrink: 0;
    /* why 38px: matches --titlebar-inset, so the send icon centers on the
       same axis as the overlay traffic lights on the left. */
    min-height: 38px;
    padding: 0 14px;
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

  .reveal {
    flex-shrink: 0;
    padding: 0;
    border: none;
    background: transparent;
    font: inherit;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .reveal:hover {
    color: var(--text-primary);
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

  /* Dashed accent frame over the whole window while a file drag hovers.
     pointer-events: none so it never swallows the drop itself. */
  .window.drop-target::after {
    content: "Drop files to attach";
    position: fixed;
    inset: 8px;
    display: grid;
    place-items: center;
    border: 2px dashed var(--accent);
    border-radius: 10px;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    font-size: 13px;
    font-weight: 600;
    color: var(--accent);
    pointer-events: none;
  }

  .attachments {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    flex-shrink: 0;
    margin: 0 20px;
    padding: 10px 0;
    border-bottom: 1px solid var(--hairline);
    list-style: none;
  }

  .chip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    border: 1px solid var(--hairline);
    border-radius: 6px;
    background: var(--bg-hover);
    font-size: 12px;
    color: var(--text-primary);
  }

  .chip-size {
    color: var(--text-tertiary);
  }

  .chip-remove {
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    background: transparent;
    font-size: 13px;
    line-height: 1;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .chip-remove:hover {
    color: var(--text-primary);
  }

  .chip-remove:disabled {
    opacity: 0.45;
    cursor: default;
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
