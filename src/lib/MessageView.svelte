<script lang="ts">
  import { getMessageBody, saveAllAttachments, saveAttachment } from "./api";
  import {
    formatFileSize,
    formatFullDate,
    senderInitials,
    senderName,
  } from "./format";
  import type {
    MessageAttachment,
    MessageBody,
    MessageHeader,
  } from "./types";

  let {
    message,
  }: {
    message: MessageHeader | null;
  } = $props();

  let body = $state<MessageBody | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // why: $effect re-runs whenever the selected message changes; the id guard
  // drops stale responses when the user clicks faster than bodies load.
  $effect(() => {
    body = null;
    error = null;
    attachmentError = null;
    if (message === null) return;
    const id = message.id;
    loading = true;
    getMessageBody(id)
      .then((loaded) => {
        if (message?.id === id) body = loaded;
      })
      .catch((err) => {
        if (message?.id === id) error = String(err);
      })
      .finally(() => {
        if (message?.id === id) loading = false;
      });
  });

  // Saving pulls the bytes from the server (they are never cached), so the
  // chips lock while a fetch is in flight and errors surface inline.
  let savingAttachments = $state(false);
  let attachmentError = $state<string | null>(null);

  async function runSave(action: () => Promise<void>) {
    if (savingAttachments) return;
    savingAttachments = true;
    attachmentError = null;
    try {
      await action();
    } catch (err) {
      attachmentError = String(err);
    } finally {
      savingAttachments = false;
    }
  }

  const saveOne = (attachment: MessageAttachment) =>
    void runSave(() => saveAttachment(attachment));
  const saveAll = (messageId: number) =>
    void runSave(() => saveAllAttachments(messageId));

  // The per-message "Load Images" click (policy "ask"): same body, re-rendered
  // by the backend with remote images fetched and inlined. No spinner — the
  // old body stays visible until the richer one arrives.
  async function loadRemoteImages() {
    if (message === null) return;
    const id = message.id;
    try {
      const loaded = await getMessageBody(id, true);
      if (message?.id === id) body = loaded;
    } catch (err) {
      if (message?.id === id) error = String(err);
    }
  }
</script>

<article>
  {#if message === null}
    <p class="empty">Select a message</p>
  {:else}
    <header>
      <span class="avatar" aria-hidden="true">
        {senderInitials(message.from)}
      </span>
      <div class="who">
        <p class="from" title={message.from}>{senderName(message.from)}</p>
        <h2 class="subject">{message.subject}</h2>
      </div>
      <span class="date">{formatFullDate(message.date)}</span>
    </header>
    <dl class="recipients">
      <dt>From</dt>
      <dd title={message.from}>{message.from}</dd>
      {#if message.to}
        <dt>To</dt>
        <dd title={message.to}>{message.to}</dd>
      {/if}
      {#if message.cc}
        <dt>Cc</dt>
        <dd title={message.cc}>{message.cc}</dd>
      {/if}
      {#if message.replyTo}
        <dt>Reply-To</dt>
        <dd title={message.replyTo}>{message.replyTo}</dd>
      {/if}
    </dl>
    {#if body && body.attachments.length > 0}
      {@const current = message}
      <div class="attachments">
        <svg
          class="clip"
          viewBox="0 0 24 24"
          width="14"
          height="14"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path
            d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"
          />
        </svg>
        {#each body.attachments as attachment (attachment.id)}
          <button
            class="chip"
            title="Save “{attachment.filename}”"
            disabled={savingAttachments}
            onclick={() => saveOne(attachment)}
          >
            <span class="name">{attachment.filename}</span>
            <span class="size">{formatFileSize(attachment.size)}</span>
          </button>
        {/each}
        {#if body.attachments.length > 1}
          <button
            class="chip save-all"
            disabled={savingAttachments}
            onclick={() => saveAll(current.id)}
          >
            Save All
          </button>
        {/if}
      </div>
      {#if attachmentError}
        <p class="attachment-error" role="alert">{attachmentError}</p>
      {/if}
    {/if}
    {#if body?.canLoadRemote}
      <div class="remote-banner">
        <span>
          {body.blockedImages === 1
            ? "1 remote image was"
            : `${body.blockedImages} remote images were`} blocked to protect
          your privacy.
        </span>
        <button onclick={() => void loadRemoteImages()}>Load Images</button>
      </div>
    {/if}
    {#if loading}
      <p class="empty">Loading…</p>
    {:else if error}
      <p class="error" role="alert">{error}</p>
    {:else if body?.html}
      <!-- SECURITY (hard rule): sandbox MUST stay empty — JS disabled, opaque
           origin, no forms/popups/navigation. The srcdoc is a sanitized
           document built in Rust (mail::sanitize); never render raw mail HTML
           and never render it outside this iframe. -->
      <iframe
        class="body-frame"
        title="Message body"
        sandbox=""
        srcdoc={body.html}
        referrerpolicy="no-referrer"
      ></iframe>
    {:else if body?.text}
      <pre class="body">{body.text}</pre>
    {/if}
  {/if}
</article>

<style>
  article {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--bg-window);
  }

  .empty {
    margin: auto;
    color: var(--text-tertiary);
  }

  .error {
    margin: 0;
    padding: 16px 20px;
    color: #d9302c;
  }

  header {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-shrink: 0;
    padding: 14px 20px;
    border-bottom: 1px solid var(--hairline);
  }

  .avatar {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: linear-gradient(180deg, #a8b0bd, #8b95a6);
    font-size: 15px;
    font-weight: 600;
    color: #ffffff;
  }

  .who {
    flex: 1;
    min-width: 0;
  }

  .who > * {
    overflow: hidden;
    margin: 0;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .from {
    font-weight: 600;
  }

  .subject {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-secondary);
  }

  .date {
    flex-shrink: 0;
    align-self: flex-start;
    padding-top: 2px;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .recipients {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 2px 10px;
    flex-shrink: 0;
    margin: 0;
    padding: 8px 20px;
    border-bottom: 1px solid var(--hairline);
    font-size: 12px;
  }

  .recipients dt {
    color: var(--text-tertiary);
    text-align: right;
    user-select: none;
  }

  .recipients dd {
    margin: 0;
    overflow: hidden;
    color: var(--text-secondary);
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .attachments {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    flex-shrink: 0;
    padding: 8px 20px;
    border-bottom: 1px solid var(--hairline);
  }

  .attachments .clip {
    flex-shrink: 0;
    color: var(--text-tertiary);
  }

  .chip {
    display: flex;
    align-items: baseline;
    gap: 6px;
    max-width: 260px;
    padding: 3px 10px;
    border: 1px solid var(--hairline);
    border-radius: 999px;
    background: var(--bg-window);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .chip:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .chip:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .chip .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chip .size {
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .chip.save-all {
    color: var(--accent);
  }

  .attachment-error {
    flex-shrink: 0;
    margin: 0;
    padding: 4px 20px 8px;
    border-bottom: 1px solid var(--hairline);
    font-size: 12px;
    color: #d9302c;
  }

  .remote-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-shrink: 0;
    padding: 6px 20px;
    border-bottom: 1px solid var(--hairline);
    background: var(--bg-hover);
    font-size: 12px;
    color: var(--text-secondary);
  }

  .remote-banner button {
    padding: 2px 10px;
    border: 1px solid var(--hairline);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
  }

  .body {
    margin: 0;
    padding: 16px 20px;
    overflow-y: auto;
    font: inherit;
    line-height: 1.5;
    white-space: pre-wrap;
    word-wrap: break-word;
  }

  .body-frame {
    flex: 1;
    width: 100%;
    min-height: 0;
    border: none;
  }
</style>
