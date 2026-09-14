<script lang="ts">
  import { onMount } from "svelte";
  import { getLlmSummaryEnabled, setLlmSummaryEnabled } from "./api";

  let enabled = $state(false);
  let error = $state<string | null>(null);

  async function toggle(next: boolean) {
    error = null;
    const previous = enabled;
    enabled = next;
    try {
      await setLlmSummaryEnabled(next);
    } catch (err) {
      // why: the box must not lie — a failed save rolls the value back.
      enabled = previous;
      error = String(err);
    }
  }

  onMount(() => {
    void getLlmSummaryEnabled().then((stored) => (enabled = stored));
  });
</script>

<section class="pane">
  <fieldset>
    <legend>On-device summaries</legend>
    <div class="choice">
      <input
        type="checkbox"
        id="llm-summary"
        checked={enabled}
        onchange={(e) => void toggle(e.currentTarget.checked)}
      />
      <span>
        <label for="llm-summary">Summarize messages with a local AI model</label>
        <small>Adds a summarize button to messages and threads.</small>
      </span>
    </div>
    <p class="explain">
      Experimental. Summaries are written by a small language model that runs
      entirely on this Mac — no message ever leaves it. The model is a
      separate download you pick and start yourself; switching this on
      downloads nothing by itself.
    </p>
  </fieldset>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 20px 26px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* White grouped boxes on the settings canvas, like the other tabs. */
  fieldset {
    margin: 0;
    max-width: 520px;
    padding: 8px 12px;
    border: 1px solid var(--hairline);
    border-radius: 9px;
    background: var(--bg-window);
  }

  legend {
    padding: 0 4px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  .choice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 5px 0;
    font-size: 13px;
  }

  .choice input {
    margin-top: 2px;
  }

  .choice span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .choice label {
    font-weight: 500;
  }

  .choice small {
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .explain {
    margin: 6px 0 2px;
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .error {
    margin: 0;
    font-size: 12px;
    color: var(--danger);
  }
</style>
