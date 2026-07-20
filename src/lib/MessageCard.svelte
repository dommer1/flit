<script lang="ts">
  import { getMessageBody, saveAllAttachments, saveAttachment } from "./api";
  import { extColor, fileExt } from "./attachments";
  import { formatFileSize, formatFullDate, senderName } from "./format";
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
    expanded,
    last,
    ownEmail,
    accountColor,
    onToggle,
    onDraft,
  }: {
    message: MessageHeader;
    /** This message's body from the conversation's bulk load; null while
     * the batch is still in flight (or when it failed — see `error`). */
    body: MessageBody | null;
    loading: boolean;
    error: string | null;
    expanded: boolean;
    /** Newest message of the conversation — its card is emphasized. */
    last: boolean;
    /** The owning account's address; a match marks the message as "me". */
    ownEmail: string | null;
    /** The owning account's accent color for the "me" avatar. */
    accountColor: string | null;
    onToggle: () => void;
    /** Per-message reply actions in the card footer. */
    onDraft: (kind: "reply" | "reply-all") => void;
  } = $props();

  /** Bare address out of `Name <addr>`; a plain address passes through. */
  function bareAddress(from: string): string {
    const match = from.match(/<([^<>]+)>\s*$/);
    return (match?.[1] ?? from).trim();
  }

  let own = $derived(
    ownEmail !== null &&
      bareAddress(message.from).toLowerCase() === ownEmail.toLowerCase(),
  );

  // Clicking the sender's name reveals the bare address in the meta line.
  let showAddr = $state(false);

  let metaLine = $derived(
    (showAddr ? `From: ${bareAddress(message.from)} · ` : "") +
      `To: ${message.to}`,
  );
  // Like the prototype: Cc and Bcc when present, nothing else — Reply-To
  // stays a sending concern, not a display line.
  let ccLine = $derived(
    [
      message.cc !== "" ? `Cc: ${message.cc}` : "",
      message.bcc !== "" ? `Bcc: ${message.bcc}` : "",
    ]
      .filter(Boolean)
      .join(" · "),
  );

  // The per-message "Load Images" click re-renders richer than the bulk
  // body — it wins over the prop until this card instance dies.
  let richBody = $state<MessageBody | null>(null);
  let shown = $derived(richBody ?? body);

  // Folded quoted history of a text body; opens per card, like Gmail's •••.
  let showQuoted = $state(false);

  // Trust warnings, both computed locally by the backend (contact history
  // and the receiving server's own Authentication-Results header) — only
  // an explicit "fail" warns; missing verdicts prove nothing and stay quiet.
  let failedChecks = $derived(
    (["spf", "dkim", "dmarc"] as const)
      .filter((method) => shown?.auth?.[method] === "fail")
      .map((method) => method.toUpperCase()),
  );

  // Hides the trust banner for this card instance only — reopening the
  // message shows it again; a warning is not something to accept forever.
  let trustDismissed = $state(false);

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

  /** Ceiling for a measured body: viewport-tracking content (100vh blocks)
   * makes the content-driven measurement follow the frame itself — such a
   * pathological mail stops here and scrolls inside instead of growing the
   * frame forever. */
  const MAX_BODY_HEIGHT = 20000;

  /**
   * Auto-size the body iframe to its document, so every open message of a
   * conversation shows whole in the stacked cards.
   *
   * SECURITY: reading contentDocument is what `sandbox="allow-same-origin"`
   * exists for — see the iframe below. Scripts stay blocked, so nothing
   * inside the frame can exploit the shared origin.
   */
  function autoSize(frame: HTMLIFrameElement) {
    let bodyObserver: ResizeObserver | null = null;

    const size = () => {
      // why optional: jsdom (tests) has no srcdoc layout — leave the CSS
      // fallback height alone when there is nothing to measure.
      const frameBody = frame.contentDocument?.body;
      if (!frameBody || frameBody.scrollHeight === 0) return;
      // why measure the body, not documentElement: the document element's
      // scrollHeight is clamped to the frame's own viewport, so measuring
      // it feeds the height we just set back into the next measurement —
      // the frame grows forever. The body's box follows only its content.
      // Its margins sit outside scrollHeight, so they are added explicitly.
      const styles = frame.contentWindow?.getComputedStyle(frameBody);
      const margins = styles
        ? parseFloat(styles.marginTop) + parseFloat(styles.marginBottom)
        : 0;
      // A horizontal scrollbar (wide fixed-width mail) sits inside the
      // frame's viewport — reserve its thickness so it never clips the
      // bottom of the message (vertical scrolling is disabled in the doc).
      const hScrollbar =
        frameBody.scrollWidth > frame.clientWidth ? 18 : 0;
      const height = Math.min(
        Math.ceil(frameBody.scrollHeight + (margins || 0) + hScrollbar) + 2,
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

<section class="card" class:last>
  {#if expanded && !trustDismissed && (shown?.senderAnomaly || failedChecks.length > 0)}
    <!-- Full-width strip above the header; .card's overflow:hidden clips it
         to the rounded corners. Dismissal is per card instance only — the
         warning returns on the next open, it is not "accept forever". -->
    <div class="trust-banner" role="alert">
      <svg
        width="14"
        height="14"
        viewBox="0 0 20 20"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        aria-hidden="true"
      >
        <path d="M10 3 1.8 16.5h16.4L10 3Z" stroke-linejoin="round" />
        <path d="M10 8.4v3.4" stroke-linecap="round" />
        <circle cx="10" cy="14.1" r="0.4" fill="currentColor" stroke="none" />
      </svg>
      <span class="trust-lines">
        {#if shown?.senderAnomaly}
          <span title="Usually writes from {shown.senderAnomaly.usualEmail}">
            {shown.senderAnomaly.name} does not usually use this email address.
          </span>
        {/if}
        {#if failedChecks.length > 0}
          <span>
            This message failed {failedChecks.join(", ")} authentication.
          </span>
        {/if}
      </span>
      <button
        class="trust-dismiss"
        aria-label="Dismiss warning"
        onclick={() => (trustDismissed = true)}
      >
        ×
      </button>
    </div>
  {/if}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events
       — the row is a large toggle target like the design's; keyboard users
       reach the same state through the sender-name button inside it. -->
  <div class="head" onclick={onToggle}>
    <span
      class="avatar"
      class:own
      aria-hidden="true"
      style:background={own ? (accountColor ?? "var(--accent)") : undefined}
    >
      {senderName(message.from).charAt(0).toUpperCase()}
    </span>
    <span class="who">
      <span class="name-row">
        <button
          class="name"
          title={expanded ? "Show sender address" : undefined}
          onclick={(e) => {
            // Like the prototype's name click: reveal the address and make
            // sure the card is open to show it.
            e.stopPropagation();
            showAddr = !showAddr;
            if (!expanded) onToggle();
          }}
        >
          {senderName(message.from)}
        </button>
        {#if message.isDraft}
          <span class="draft-badge">Draft</span>
        {:else if own}
          <span class="me">me</span>
        {/if}
      </span>
      {#if !expanded}
        <span class="preview">{message.snippet}</span>
      {:else}
        <span class="meta">{metaLine}</span>
        {#if ccLine}
          <span class="meta">{ccLine}</span>
        {/if}
      {/if}
    </span>
    {#if !message.read && !expanded}
      <span class="dot" aria-hidden="true"></span>
    {/if}
    <span class="date">{formatFullDate(message.date)}</span>
  </div>

  {#if expanded}
    <div class="content">
      {#if shown && shown.attachments.length > 0}
        <div class="atts-label">
          <svg
            width="13"
            height="13"
            viewBox="0 0 20 20"
            fill="none"
            stroke="currentColor"
            stroke-width="1.6"
            aria-hidden="true"
          >
            <path
              d="M15.5 9.5 9.9 15a3.5 3.5 0 0 1-5-5l6.4-6.3a2.3 2.3 0 0 1 3.3 3.3L8.3 13.2a1.2 1.2 0 0 1-1.7-1.7l5.2-5.1"
            />
          </svg>
          {shown.attachments.length}
          {shown.attachments.length === 1 ? "attachment" : "attachments"}
        </div>
        <div class="atts">
          {#each shown.attachments as attachment (attachment.id)}
            {@const ext = fileExt(attachment.filename)}
            <button
              class="att"
              title="Save “{attachment.filename}”"
              disabled={savingAttachments}
              onclick={() => saveOne(attachment)}
            >
              <span class="ext" style:background={extColor(ext)}>
                {ext === "" ? "?" : ext}
              </span>
              <span class="att-info">
                <span class="att-name">{attachment.filename}</span>
                <span class="att-size">{formatFileSize(attachment.size)}</span>
              </span>
              <svg
                class="down"
                width="15"
                height="15"
                viewBox="0 0 20 20"
                fill="none"
                stroke="currentColor"
                stroke-width="1.6"
                aria-hidden="true"
              >
                <path d="M10 3.5v9m0 0 3.5-3.5M10 12.5 6.5 9" />
                <path d="M4 15.5h12" />
              </svg>
            </button>
          {/each}
          {#if shown.attachments.length > 1}
            <button
              class="att save-all"
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
        <div class="att-divider"></div>
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
             this iframe. sandbox contains EXACTLY allow-same-origin and
             nothing else: it lets the parent read the document's height
             (autoSize) so whole conversations can be open at once.
             allow-scripts must NEVER be added — scripts stay blocked by the
             sandbox flag, by the sanitizer, and by the srcdoc's own CSP. -->
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
        {#if shown.quotedText}
          {#if showQuoted}
            <pre class="body quoted">{shown.quotedText}</pre>
          {:else}
            <button
              class="quote-toggle"
              title="Show quoted text"
              aria-label="Show quoted text"
              onclick={() => (showQuoted = true)}
            >
              •••
            </button>
          {/if}
        {/if}
      {:else if loading}
        <p class="loading">Loading…</p>
      {:else if error}
        <p class="error" role="alert">{error}</p>
      {/if}
      <div class="actions">
        <button
          class="action"
          title="Reply"
          aria-label="Reply to this message"
          onclick={() => onDraft("reply")}
        >
          <svg
            width="15"
            height="15"
            viewBox="0 0 20 20"
            fill="none"
            stroke="currentColor"
            stroke-width="1.6"
            aria-hidden="true"
          >
            <path d="M8 4 3 8.5 8 13" />
            <path d="M3 8.5h8.5a5 5 0 0 1 5 5V16" />
          </svg>
        </button>
        <button
          class="action"
          title="Reply All"
          aria-label="Reply all to this message"
          onclick={() => onDraft("reply-all")}
        >
          <svg
            width="15"
            height="15"
            viewBox="0 0 20 20"
            fill="none"
            stroke="currentColor"
            stroke-width="1.6"
            aria-hidden="true"
          >
            <path d="M7 4 2 8.5 7 13" />
            <path d="M11 4 6 8.5l5 4.5" />
            <path d="M6 8.5h7.5a4.5 4.5 0 0 1 4.5 4.5V16" />
          </svg>
        </button>
      </div>
    </div>
  {/if}
</section>

<style>
  .card {
    background: var(--bg-card);
    border: 1px solid var(--card-border);
    border-radius: 10px;
    overflow: hidden;
  }

  /* The newest message carries the visual weight. */
  .card.last {
    border-color: var(--card-border-strong);
    box-shadow: 0 1px 4px rgb(0 0 0 / 6%);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 14px;
    cursor: default;
  }

  .head:hover {
    background: var(--bg-hover);
  }

  .avatar {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--avatar-muted);
    font-size: 12.5px;
    font-weight: 600;
    color: #ffffff;
  }

  .who {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }

  .name-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .name {
    overflow: hidden;
    margin-left: -3px;
    padding: 0 3px;
    border: none;
    border-radius: 4px;
    background: none;
    font: inherit;
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary);
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }

  .name:hover {
    background: var(--bg-selected-muted);
  }

  .me {
    flex-shrink: 0;
    font-size: 10.5px;
    color: var(--text-secondary);
  }

  /* Same amber family as the trust banner: "unfinished", not an error. */
  .draft-badge {
    flex-shrink: 0;
    padding: 1px 7px;
    border-radius: 9px;
    background: rgba(178, 134, 14, 0.12);
    font-size: 10.5px;
    font-weight: 600;
    color: #9c7c10;
  }

  .preview,
  .meta {
    overflow: hidden;
    margin-top: 1px;
    font-size: 12px;
    color: var(--text-secondary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    font-size: 11.5px;
    user-select: text;
  }

  .dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }

  .date {
    flex-shrink: 0;
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .content {
    padding: 2px 16px 12px 24px;
    border-top: 1px solid var(--hairline);
  }

  .atts-label {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 12px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .atts {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }

  .att {
    display: flex;
    align-items: center;
    gap: 9px;
    max-width: 240px;
    padding: 8px 12px 8px 9px;
    border: 1px solid var(--card-border);
    border-radius: 8px;
    background: var(--bg-thread);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .att:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--card-border-strong);
  }

  .att:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .ext {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 32px;
    height: 32px;
    border-radius: 7px;
    font-size: 8.5px;
    font-weight: 700;
    letter-spacing: 0.03em;
    color: #ffffff;
  }

  .att-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .att-name {
    overflow: hidden;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .att-size {
    font-size: 10.5px;
    color: var(--text-secondary);
  }

  .att .down {
    flex-shrink: 0;
    margin-left: 2px;
    color: var(--text-secondary);
  }

  .att.save-all {
    padding: 8px 14px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }

  .att-divider {
    margin-top: 12px;
    border-bottom: 1px solid var(--hairline);
  }

  .attachment-error {
    margin: 8px 0 0;
    font-size: 12px;
    color: #d9302c;
  }

  /* Amber, not red: "unusual, look twice", not "confirmed malicious". */
  .trust-banner {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 8px 14px;
    background: rgba(178, 134, 14, 0.09);
    border-bottom: 1px solid rgba(178, 134, 14, 0.28);
    font-size: 12px;
    font-weight: 600;
    color: #9c7c10;
  }

  .trust-banner svg {
    flex-shrink: 0;
  }

  .trust-lines {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .trust-dismiss {
    margin-left: auto;
    padding: 0 2px;
    border: none;
    background: none;
    font-size: 15px;
    line-height: 1;
    color: inherit;
    opacity: 0.7;
    cursor: pointer;
  }

  .trust-dismiss:hover {
    opacity: 1;
  }

  .remote-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 12px;
    padding: 6px 12px;
    border: 1px solid var(--card-border);
    border-radius: 8px;
    background: var(--bg-thread);
    font-size: 12px;
    color: var(--text-secondary);
  }

  .remote-banner button {
    padding: 2px 10px;
    border: 1px solid var(--card-border-strong);
    border-radius: 6px;
    background: var(--bg-card);
    font: inherit;
    font-size: 12px;
    color: var(--text-primary);
    white-space: nowrap;
    cursor: pointer;
  }

  .loading {
    margin: 12px 0 0;
    color: var(--text-tertiary);
  }

  .error {
    margin: 12px 0 0;
    color: #d9302c;
  }

  .body {
    margin: 0;
    padding-top: 2px;
    /* why: overflow-y auto alone computes overflow-x to auto — clip
       sideways, the conversation column is the only scroller. */
    overflow-x: hidden;
    font: inherit;
    font-size: 13.5px;
    line-height: 1.6;
    color: var(--text-primary);
    white-space: pre-wrap;
    word-wrap: break-word;
  }

  .body.quoted {
    color: var(--text-secondary);
  }

  /* Same pill as the summary inside HTML bodies (BODY_STYLE). */
  .quote-toggle {
    align-self: flex-start;
    margin-top: 10px;
    padding: 1px 9px;
    border: none;
    border-radius: 9px;
    background: var(--bg-hover);
    font-size: 10px;
    letter-spacing: 0.1em;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .quote-toggle:hover {
    background: var(--bg-selected-muted);
  }

  .body-frame {
    display: block;
    width: 100%;
    /* Fallback until autoSize measures the document (and in tests). */
    height: 320px;
    border: none;
    background: #ffffff;
    border-radius: 6px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 2px;
    margin-top: 10px;
  }

  .action {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 26px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .action:hover {
    background: var(--bg-hover);
  }
</style>
