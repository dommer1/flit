<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    closeCompose,
    inspectAttachments,
    listAccounts,
    onFileDrop,
    queueSend,
    saveDraft,
    takeComposeDraft,
  } from "./api";
  import { debounce } from "./debounce";
  import { isDraftEmpty } from "./draft";
  import type { Account, AttachmentInfo, OutgoingMessage } from "./types";
  import RichTextEditor from "./RichTextEditor.svelte";

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
  let bodyHtml = $state("");
  /** The draft's plain text, handed to the editor exactly once at mount. */
  let initialBody = $state("");
  // why: the editor renders only after the draft resolved — Tiptap takes its
  // content at construction, so mounting early would show an empty body.
  let loaded = $state(false);
  let attachments = $state<AttachmentInfo[]>([]);
  /** A file drag is currently above the window — shows the drop overlay. */
  let dropHover = $state(false);

  let queueing = $state(false);
  let error = $state<string | null>(null);

  // Gmail-style silent drafts: while the user types, the message is saved to
  // the account's server Drafts folder (debounced), and once more when the
  // window closes — no dialogs. draftMessageId is the handle under which the
  // previous version is replaced on each save.
  const AUTOSAVE_MS = 30_000;
  let draftMessageId: string | null = null;
  let saveError = $state<string | null>(null);
  // why watching, not `loaded`: the editor mounts on `loaded`, so its
  // initial bind-backs still look like edits — only changes after the whole
  // mount sequence settled count as the user's.
  let watching = false;
  let dirty = false;
  let saving = false;
  let sent = false;

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let unlistenClose: (() => void) | undefined;
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
      initialBody = draft?.body ?? "";
      body = initialBody;
      loaded = true;
      // why re-stat instead of trusting the draft: an undone/failed send may
      // reopen after the file changed or vanished — surface that now.
      if (draft?.attachments?.length) {
        await addAttachments(draft.attachments.map((a) => a.path));
      }
      unlisten = await onFileDrop({
        onHover: (hovering) => (dropHover = hovering),
        onDrop: (paths) => void addAttachments(paths),
      });
      unlistenClose = await getCurrentWindow().onCloseRequested(
        async (event) => {
          if (sent || !dirty) return;
          const message = currentMessage();
          if (!message || (isDraftEmpty(message) && draftMessageId === null)) {
            return;
          }
          // why: hold the window open until the save lands — destroying the
          // webview mid-save could lose the newest keystrokes.
          event.preventDefault();
          await saveNow();
          if (dirty) return; // save failed; keep the window and its error
          await getCurrentWindow().destroy();
        },
      );
      watching = true;
    })();
    return () => {
      unlisten?.();
      unlistenClose?.();
    };
  });

  function currentMessage(): OutgoingMessage | null {
    return accountId === null ? null : buildMessage(accountId);
  }

  async function saveNow() {
    if (saving || sent || !dirty) return;
    const message = currentMessage();
    if (!message) return;
    // An untouched-then-cleared window has nothing worth a server round
    // trip; once a version exists it keeps being replaced, even by "".
    if (isDraftEmpty(message) && draftMessageId === null) return;
    saving = true;
    // why clear before the await: keystrokes landing during the save must
    // re-mark the draft dirty, not be swallowed by a stale flag.
    dirty = false;
    try {
      draftMessageId = await saveDraft(message, draftMessageId);
      saveError = null;
    } catch (err) {
      dirty = true;
      saveError = String(err);
    } finally {
      saving = false;
    }
  }

  const scheduleAutosave = debounce(() => void saveNow(), AUTOSAVE_MS);

  $effect(() => {
    void [to, cc, bcc, subject, body, attachments];
    if (!watching) return;
    dirty = true;
    scheduleAutosave();
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

  function buildMessage(accountId: number) {
    return {
      accountId,
      to,
      cc,
      bcc,
      subject,
      body,
      // why || undefined: an empty editor reports "" — the wire format
      // treats a missing field as "plain text only".
      bodyHtml: bodyHtml || undefined,
      attachments: attachments.map(({ path, name }) => ({ path, name })),
    };
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
      await queueSend(buildMessage(accountId));
      // why: the message now belongs to the send queue — the close below
      // must not snapshot it back into the Drafts folder.
      sent = true;
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

  {#if error || saveError}
    <p class="error" role="alert">
      {error ?? `Draft not saved: ${saveError}`}
    </p>
  {/if}

  {#if loaded}
    <div class="body-area" class:with-attachments={attachments.length > 0}>
      <RichTextEditor
        initialText={initialBody}
        bind:text={body}
        bind:html={bodyHtml}
        disabled={queueing}
      />
    </div>
  {/if}

  {#if attachments.length > 0}
    <!-- Floating card pinned over the bottom of the body, so attachments
         never push the envelope fields or the text around. -->
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
</form>

<style>
  .window {
    /* why relative: the attachments card is positioned against the window. */
    position: relative;
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
  select:focus {
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

  /* Floating card: hovers over the bottom edge of the body instead of
     occupying a row of the envelope — the compose text flows beneath it. */
  .attachments {
    position: absolute;
    bottom: 14px;
    left: 50%;
    transform: translateX(-50%);
    width: max-content;
    max-width: calc(100% - 40px);
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
    margin: 0;
    padding: 8px 10px;
    border: 1px solid var(--hairline);
    border-radius: 12px;
    /* why color-mix + blur: the card floats over the user's own text, so it
       stays readable without fully hiding what's underneath. */
    background: color-mix(in srgb, var(--bg-window) 82%, transparent);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.16);
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

  .body-area {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  /* Keep the last lines of text visible above the floating card. */
  .body-area.with-attachments :global(.tiptap) {
    padding-bottom: 72px;
  }
</style>
