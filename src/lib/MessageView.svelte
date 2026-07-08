<script lang="ts">
  import { getMessageBody } from "./api";
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
      <h2>{message.subject}</h2>
      <p class="meta">
        <span class="from">{message.from}</span>
        <span class="date">{message.date}</span>
      </p>
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
    gap: 1rem;
    flex: 1;
    padding: 1.25rem 1.5rem;
    overflow-y: auto;
  }

  .empty {
    margin: auto;
    color: #888;
  }

  .error {
    margin: 0;
    color: #8a1f1f;
  }

  h2 {
    margin: 0 0 0.375rem;
    font-size: 1.125rem;
  }

  .meta {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    margin: 0;
    font-size: 0.8125rem;
    color: #666;
  }

  .body {
    margin: 0;
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
