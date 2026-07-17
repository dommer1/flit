<script lang="ts">
  import { getMessageBody, saveAllAttachments, saveAttachment } from "./api";
  import {
    formatFileSize,
    formatFullDate,
    formatListDate,
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
    body,
    loading,
    error,
    collapsed,
    single,
    onToggle,
  }: {
    message: MessageHeader;
    /** This message's body from the conversation's bulk load; null while
     * the batch is still in flight (or when it failed — see `error`). */
    body: MessageBody | null;
    loading: boolean;
    error: string | null;
    collapsed: boolean;
    /** Threads of one render a single always-open, non-collapsible card. */
    single: boolean;
    onToggle: () => void;
  } = $props();

  // The per-message "Load Images" click re-renders richer than the bulk
  // body — it wins over the prop until this card instance dies.
  let richBody = $state<MessageBody | null>(null);
  let shown = $derived(richBody ?? body);

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
    const id = message.id;
    try {
      const loaded = await getMessageBody(id, true);
      if (message.id === id) richBody = loaded;
    } catch (err) {
      if (message.id === id) attachmentError = String(err);
    }
  }

  /**
   * Auto-size the body iframe to its document, so every message of a
   * conversation can be fully open at once (Canary-style stack).
   *
   * SECURITY: reading contentDocument is what `sandbox="allow-same-origin"`
   * exists for — see the iframe below. Scripts stay blocked, so nothing
   * inside the frame can exploit the shared origin.
   */
  /** Ceiling for a measured body: viewport-tracking content (100vh blocks)
   * makes the content-driven measurement follow the frame itself — such a
   * pathological mail stops here and scrolls inside instead of growing the
   * frame forever. */
  const MAX_BODY_HEIGHT = 20000;

  function autoSize(frame: HTMLIFrameElement) {
    let bodyObserver: ResizeObserver | null = null;

    const size = () => {
      // why optional: jsdom (tests) has no srcdoc layout — leave the CSS
      // fallback height alone when there is nothing to measure.
      const body = frame.contentDocument?.body;
      if (!body || body.scrollHeight === 0) return;
      // why measure the body, not documentElement: the document element's
      // scrollHeight is clamped to the frame's own viewport, so measuring
      // it feeds the height we just set back into the next measurement —
      // the frame grows forever. The body's box follows only its content.
      // Its margins sit outside scrollHeight, so they are added explicitly.
      const styles = frame.contentWindow?.getComputedStyle(body);
      const margins = styles
        ? parseFloat(styles.marginTop) + parseFloat(styles.marginBottom)
        : 0;
      const height = Math.min(
        Math.ceil(body.scrollHeight + (margins || 0)) + 2,
        MAX_BODY_HEIGHT,
      );
      const current = frame.offsetHeight;
      // why asymmetric: any undershoot leaves an inner scrollbar (the
      // "double scroll"), so growing applies immediately; shrinking
      // tolerates 2px so sub-pixel rounding never oscillates.
      if (height > current || height < current - 2) {
        frame.style.height = `${height}px`;
      }
    };

    // (Re)attach to the current document: the srcdoc replaces the initial
    // about:blank document ("Load Images" swaps it again later), orphaning
    // anything bound to the previous document's body.
    const hook = () => {
      size();
      bodyObserver?.disconnect();
      bodyObserver = new ResizeObserver(size);
      const doc = frame.contentDocument;
      if (doc?.body) bodyObserver.observe(doc.body);
      // why: inline data: images can report their intrinsic size after the
      // document's load event — each late decode reflows the body, so every
      // finished image re-measures. Parent-attached listeners work without
      // any script running inside the frame.
      for (const image of Array.from(doc?.images ?? [])) {
        image.addEventListener("load", size);
      }
    };

    frame.addEventListener("load", hook);
    hook();
    // why also observe the frame: it fires on insertion (covering a load
    // event the listener attached too late for) and on pane resizes. The
    // measurement is content-driven, so this can re-trigger size() but
    // never feed back into it.
    const frameObserver = new ResizeObserver(size);
    frameObserver.observe(frame);
    return {
      destroy() {
        bodyObserver?.disconnect();
        frameObserver.disconnect();
      },
    };
  }
</script>

{#if collapsed}
  <button class="collapsed" onclick={onToggle}>
    <span class="avatar small" aria-hidden="true">
      {senderInitials(message.from)}
    </span>
    <span class="who">
      <span class="from">{senderName(message.from)}</span>
      <span class="snippet">{message.snippet}</span>
    </span>
    {#if !message.read}
      <span class="dot" aria-hidden="true"></span>
    {/if}
    <span class="date">{formatListDate(message.date)}</span>
  </button>
{:else}
  <section class="card">
    <header>
      <span class="avatar" aria-hidden="true">
        {senderInitials(message.from)}
      </span>
      <div class="who">
        <p class="from" title={message.from}>{senderName(message.from)}</p>
        <h2 class="subject">{message.subject}</h2>
        <p class="meta">
          <span class="pair">
            <span class="key">From:</span>
            <span class="val" title={message.from}>{message.from}</span>
          </span>
          {#if message.to}
            <span class="pair">
              <span class="key">To:</span>
              <span class="val" title={message.to}>{message.to}</span>
            </span>
          {/if}
          {#if message.cc}
            <span class="pair">
              <span class="key">Cc:</span>
              <span class="val" title={message.cc}>{message.cc}</span>
            </span>
          {/if}
          {#if message.replyTo}
            <span class="pair">
              <span class="key">Reply-To:</span>
              <span class="val" title={message.replyTo}>{message.replyTo}</span>
            </span>
          {/if}
        </p>
      </div>
      <span class="date">{formatFullDate(message.date)}</span>
      {#if !single}
        <button
          class="fold"
          title="Collapse message"
          aria-label="Collapse message"
          onclick={onToggle}
        >
          <svg
            viewBox="0 0 24 24"
            width="14"
            height="14"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path d="m18 15-6-6-6 6" />
          </svg>
        </button>
      {/if}
    </header>
    {#if shown && shown.attachments.length > 0}
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
        {#each shown.attachments as attachment (attachment.id)}
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
        {#if shown.attachments.length > 1}
          <button
            class="chip save-all"
            disabled={savingAttachments}
            onclick={() => saveAll(message.id)}
          >
            Save All
          </button>
        {/if}
      </div>
      {#if attachmentError}
        <p class="attachment-error" role="alert">{attachmentError}</p>
      {/if}
    {/if}
    {#if shown?.canLoadRemote}
      <div class="remote-banner">
        <span>
          {shown.blockedImages === 1
            ? "1 remote image was"
            : `${shown.blockedImages} remote images were`} blocked to protect
          your privacy.
        </span>
        <button onclick={() => void loadRemoteImages()}>Load Images</button>
      </div>
    {/if}
    {#if shown?.html}
      <!-- SECURITY (hard rule): the srcdoc is a sanitized document built in
           Rust (mail::sanitize) — never render raw mail HTML, never outside
           this iframe. sandbox contains EXACTLY allow-same-origin and nothing
           else: it lets the parent read the document's height (autoSize)
           so whole conversations can be open at once. allow-scripts must
           NEVER be added — scripts stay blocked by the sandbox flag, by the
           sanitizer, and by the srcdoc's own CSP (default-src 'none'). -->
      <iframe
        class="body-frame"
        title="Message body"
        sandbox="allow-same-origin"
        srcdoc={shown.html}
        referrerpolicy="no-referrer"
        use:autoSize
      ></iframe>
    {:else if shown?.text}
      <pre class="body">{shown.text}</pre>
    {:else if loading}
      <p class="loading">Loading…</p>
    {:else if error}
      <p class="error" role="alert">{error}</p>
    {/if}
  </section>
{/if}

<style>
  /* ── Collapsed form: one quiet row, like Canary's stacked messages. ── */
  .collapsed {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 10px 20px;
    border: none;
    border-bottom: 1px solid var(--hairline);
    background: var(--bg-window);
    font: inherit;
    color: inherit;
    text-align: left;
    cursor: default;
  }

  .collapsed:hover {
    background: var(--bg-hover);
  }

  .collapsed .who {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    gap: 0;
  }

  .collapsed .from {
    overflow: hidden;
    font-size: 13px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .collapsed .snippet {
    overflow: hidden;
    font-size: 12px;
    color: var(--text-secondary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .collapsed .dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
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

  .avatar.small {
    width: 28px;
    height: 28px;
    font-size: 11px;
  }

  .date {
    flex-shrink: 0;
    align-self: flex-start;
    padding-top: 2px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .collapsed .date {
    align-self: center;
    padding-top: 0;
    font-size: 11px;
  }

  /* ── Expanded form: the full message, sized to its content. ── */
  .card {
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--hairline);
    background: var(--bg-window);
  }

  header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    flex-shrink: 0;
    padding: 16px 28px 14px;
    border-bottom: 1px solid var(--hairline);
  }

  .fold {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--text-tertiary);
    cursor: pointer;
  }

  .fold:hover {
    background: var(--bg-hover);
    color: var(--text-secondary);
  }

  .card .who {
    flex: 1;
    min-width: 0;
  }

  .card .who > * {
    overflow: hidden;
    margin: 0;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card .from {
    font-size: 15px;
    font-weight: 700;
  }

  .subject {
    margin-top: 1px;
    font-size: 13px;
    font-weight: 400;
    color: var(--text-secondary);
  }

  /* One quiet line under the subject: From: … · To: … · Cc: … */
  .meta {
    display: flex;
    flex-wrap: wrap;
    column-gap: 6px;
    margin-top: 4px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .pair {
    display: inline-flex;
    gap: 4px;
    min-width: 0;
    max-width: 100%;
  }

  .pair + .pair::before {
    content: "·";
    margin-right: 6px;
    color: var(--text-tertiary);
  }

  .key {
    flex-shrink: 0;
    color: var(--text-tertiary);
  }

  .val {
    overflow: hidden;
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

  .loading {
    margin: 0;
    padding: 16px 28px;
    color: var(--text-tertiary);
  }

  .error {
    margin: 0;
    padding: 16px 28px;
    color: #d9302c;
  }

  /* Plain-text bodies read in a centered column, like the design's HTML
     mails; the iframe keeps full bleed (its document styles itself). */
  .body {
    box-sizing: border-box;
    width: 100%;
    max-width: 736px;
    margin: 0 auto;
    padding: 28px 48px;
    /* why no scroll: the conversation column scrolls as one — a body is
       always shown whole. Clip sideways only. */
    overflow-x: hidden;
    font: inherit;
    font-size: 14.5px;
    line-height: 1.6;
    white-space: pre-wrap;
    word-wrap: break-word;
  }

  .body-frame {
    width: 100%;
    /* Fallback until autoSize measures the document (and in tests). */
    height: 320px;
    border: none;
  }
</style>
