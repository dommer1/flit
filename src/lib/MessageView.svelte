<script lang="ts">
  import { getMessageBody } from "./api";
  import { formatFullDate, senderInitials, senderName } from "./format";
  import type { MessageBody, MessageHeader } from "./types";

  let { message }: { message: MessageHeader | null } = $props();

  let body = $state<MessageBody | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // why: $effect re-runs whenever the selected message changes; the id guard
  // drops stale responses when the user clicks faster than bodies load.
  $effect(() => {
    body = null;
    error = null;
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
    font-size: 11px;
    color: var(--text-secondary);
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
