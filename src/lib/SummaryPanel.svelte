<script lang="ts">
  // A boxed on-device summary: plain text from the backend shown as bullet
  // lines — never interpreted as HTML. Shared by message cards and the
  // conversation header.
  import type { SummaryState } from "./summaries";

  let {
    summary,
    label = "AI summary",
    onRetry,
    onClose,
    onCancel,
  }: {
    summary: SummaryState;
    /** Region name and heading. */
    label?: string;
    onRetry: () => void;
    onClose: () => void;
    /** Stop a summary still being written (the × while loading). */
    onCancel: () => void;
  } = $props();

  let lines = $derived(
    (summary.text ?? "")
      .split("\n")
      .map((line) => line.replace(/^\s*[-•*]\s*/, "").trim())
      .filter((line) => line !== ""),
  );
</script>

<div class="summary" role="region" aria-label={label}>
  <div class="summary-head">
    <span class="summary-title">{label}</span>
    <span class="summary-note">local model · may be inaccurate</span>
    <button
      class="summary-btn"
      title="Summarize again"
      aria-label="Summarize again"
      disabled={summary.loading}
      onclick={onRetry}
    >
      ↻
    </button>
    {#if summary.loading}
      <button
        class="summary-btn"
        title="Cancel"
        aria-label="Cancel summary"
        onclick={onCancel}
      >
        ×
      </button>
    {:else}
      <button
        class="summary-btn"
        title="Close summary"
        aria-label="Close summary"
        onclick={onClose}
      >
        ×
      </button>
    {/if}
  </div>
  {#if summary.loading && lines.length === 0}
    <p class="summary-status">Summarizing…</p>
  {:else if summary.error}
    <p class="summary-status error" role="alert">{summary.error}</p>
  {:else}
    <ul class="summary-lines" class:writing={summary.loading}>
      {#each lines as line, i (i)}
        <li>{line}</li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  /* Boxed so it reads as a note about the mail, not as part of it. */
  .summary {
    padding: 8px 12px 10px;
    border: 1px solid var(--hairline);
    border-radius: 8px;
    background: var(--bg-hover);
    font-size: 12.5px;
  }

  .summary-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin-bottom: 4px;
  }

  .summary-title {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  .summary-note {
    flex: 1;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .summary-btn {
    padding: 0 4px;
    border: none;
    background: none;
    font: inherit;
    font-size: 14px;
    line-height: 1;
    color: var(--text-secondary);
    cursor: default;
  }

  .summary-btn:hover:not(:disabled) {
    color: var(--text-primary);
  }

  .summary-status {
    margin: 2px 0 0;
    color: var(--text-secondary);
  }

  .summary-status.error {
    color: var(--danger);
  }

  .summary-lines {
    margin: 2px 0 0;
    padding-left: 18px;
  }

  .summary-lines li + li {
    margin-top: 2px;
  }

  /* A blinking caret after the last line while the model is still writing. */
  .summary-lines.writing li:last-child::after {
    content: "▍";
    margin-left: 2px;
    color: var(--text-secondary);
    animation: blink 1s steps(2) infinite;
  }

  @keyframes blink {
    to {
      visibility: hidden;
    }
  }
</style>
