<script lang="ts">
  import { onMount } from "svelte";
  import {
    getLlmSummaryEnabled,
    llmStatus,
    setLlmModel,
    setLlmSummaryEnabled,
  } from "./api";
  import { formatFileSize } from "./format";
  import type { LlmStatus } from "./types";

  let enabled = $state(false);
  let status = $state<LlmStatus | null>(null);
  let error = $state<string | null>(null);

  async function refresh() {
    status = await llmStatus();
  }

  async function toggle(next: boolean) {
    error = null;
    const previous = enabled;
    enabled = next;
    try {
      await setLlmSummaryEnabled(next);
      await refresh();
    } catch (err) {
      // why: the box must not lie — a failed save rolls the value back.
      enabled = previous;
      error = String(err);
    }
  }

  async function pick(id: string) {
    error = null;
    try {
      await setLlmModel(id);
      await refresh();
    } catch (err) {
      error = String(err);
    }
  }

  onMount(() => {
    void getLlmSummaryEnabled().then((stored) => (enabled = stored));
    void refresh();
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

  {#if enabled && status}
    <fieldset>
      <legend>Model</legend>
      <ul class="models">
        {#each status.models as model (model.id)}
          {@const ready = model.state.kind === "ready"}
          <li class="model" class:active={status.activeModel === model.id}>
            <input
              type="radio"
              name="llm-model"
              id="llm-model-{model.id}"
              aria-label="Use {model.name}"
              checked={status.activeModel === model.id}
              disabled={!ready}
              onchange={() => void pick(model.id)}
            />
            <div class="body">
              <div class="title">
                <label for="llm-model-{model.id}">{model.name}</label>
                {#if model.recommended}
                  <span class="badge">Recommended</span>
                {/if}
                <span class="size">{formatFileSize(model.size)}</span>
              </div>
              <small>{model.description}</small>
              <div class="state">
                {#if ready}
                  <span class="ready">Downloaded</span>
                {:else}
                  <span>Not downloaded</span>
                {/if}
              </div>
            </div>
          </li>
        {/each}
      </ul>
      <p class="explain">
        Models are fetched once from huggingface.co over TLS, verified, and
        kept in Flit's data folder. Several can be downloaded to compare; the
        picked one writes the summaries.
      </p>
    </fieldset>
  {/if}

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

  .choice small,
  .model small {
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .explain {
    margin: 6px 0 2px;
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .models {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .model {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 0;
    border-top: 1px solid var(--hairline);
    font-size: 13px;
  }

  .model:first-child {
    border-top: none;
  }

  .model input {
    margin-top: 3px;
  }

  .body {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .title {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .title label {
    font-weight: 500;
  }

  .badge {
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--bg-selected-muted);
    font-size: 10.5px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .size {
    margin-left: auto;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    color: var(--text-secondary);
  }

  .state {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 4px;
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .ready {
    color: var(--text-primary);
  }

  .error {
    margin: 0;
    font-size: 12px;
    color: var(--danger);
  }
</style>
