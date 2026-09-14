<script lang="ts">
  import { onMount } from "svelte";
  import { getShortcuts, resetShortcuts, setShortcut } from "./api";
  import { comboFromEvent, formatCombo } from "./shortcuts";
  import type { ShortcutAction, ShortcutBinding } from "./types";

  const LABELS: Record<ShortcutAction, string> = {
    "new-message": "New Message",
    reply: "Reply",
    "reply-all": "Reply All",
    forward: "Forward",
    archive: "Archive",
    trash: "Move to Trash",
    "toggle-read": "Mark Read/Unread",
    "check-mail": "Check for New Mail",
    "toggle-sidebar": "Toggle Sidebar",
    "focus-search": "Search",
    send: "Send (compose window)",
  };

  /** Built in and not rebindable — shown so the list is complete. */
  const FIXED: { keys: string; label: string }[] = [
    { keys: "↑ / ↓", label: "Previous / next message" },
    { keys: "⇧↑ / ⇧↓", label: "Extend the selection" },
    { keys: "⌘,", label: "Settings" },
    { keys: "Esc", label: "Close a dialog or Settings" },
  ];

  let bindings = $state<ShortcutBinding[]>([]);
  /** The action waiting for its new key combination, if any. */
  let recording = $state<ShortcutAction | null>(null);
  let note = $state<string | null>(null);
  let error = $state<string | null>(null);

  /** Save one change and say which other action, if any, lost its combo —
   * the backend moves a taken combo rather than refusing it. */
  async function save(action: ShortcutAction, combo: string | null) {
    note = null;
    error = null;
    const before = bindings;
    try {
      bindings = await setShortcut(action, combo);
      const loser =
        combo === null
          ? undefined
          : before.find((b) => b.action !== action && b.combo === combo);
      if (loser && combo !== null) {
        note = `${formatCombo(combo)} removed from ${LABELS[loser.action]}`;
      }
    } catch (err) {
      error = combo === null ? String(err) : `${formatCombo(combo)}: ${err}`;
    }
  }

  async function restoreAll() {
    note = null;
    error = null;
    recording = null;
    try {
      bindings = await resetShortcuts();
    } catch (err) {
      error = String(err);
    }
  }

  // why the capture phase on window: WebKit does not focus a clicked button,
  // so the chip cannot listen for keys itself — and the settings window
  // closes on Escape, which must not happen while a shortcut is recorded.
  function onRecordKey(event: KeyboardEvent) {
    if (recording === null) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      recording = null;
      return;
    }
    const combo = comboFromEvent(event);
    if (combo === null) return; // a lone modifier: keep listening
    const action = recording;
    recording = null;
    void save(action, combo);
  }

  onMount(() => {
    void getShortcuts()
      .then((stored) => (bindings = stored))
      .catch((err: unknown) => (error = String(err)));
  });
</script>

<svelte:window onkeydowncapture={onRecordKey} />

<section class="content">
  <div class="section-label">Keyboard shortcuts</div>
  <div class="group">
    {#each bindings as binding (binding.action)}
      {@const label = LABELS[binding.action]}
      <div class="row">
        <span class="row-label">{label}</span>
        <span class="controls">
          {#if binding.combo !== binding.defaultCombo}
            <button
              class="icon"
              aria-label="Restore default for {label}"
              title="Restore {formatCombo(binding.defaultCombo)}"
              onclick={() => void save(binding.action, binding.defaultCombo)}
            >
              ↺
            </button>
          {/if}
          {#if binding.combo !== null}
            <button
              class="icon"
              aria-label="Clear shortcut for {label}"
              title="Clear"
              onclick={() => void save(binding.action, null)}
            >
              ×
            </button>
          {/if}
          <button
            class="chip"
            class:recording={recording === binding.action}
            class:unbound={binding.combo === null}
            aria-label="Change shortcut for {label}"
            onclick={() => {
              note = null;
              error = null;
              recording =
                recording === binding.action ? null : binding.action;
            }}
          >
            {#if recording === binding.action}
              Press keys…
            {:else if binding.combo === null}
              None
            {:else}
              {formatCombo(binding.combo)}
            {/if}
          </button>
        </span>
      </div>
    {/each}
  </div>
  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if note}
    <p class="explain note">{note}</p>
  {/if}
  <div class="footer">
    <p class="explain">
      Click a shortcut, then press the new keys. Esc cancels. Shortcuts
      without ⌘ or ⌃ do nothing while you type in a text field.
    </p>
    <button class="restore" onclick={() => void restoreAll()}>
      Restore Defaults
    </button>
  </div>

  <div class="section-label">Fixed</div>
  <div class="group">
    {#each FIXED as fixed (fixed.keys)}
      <div class="row">
        <span class="row-label">{fixed.label}</span>
        <span class="keys">{fixed.keys}</span>
      </div>
    {/each}
  </div>
</section>

<style>
  .content {
    padding: 20px 26px;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .section-label {
    margin-bottom: 7px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  /* White grouped boxes on the settings canvas, like the other tabs. */
  .group {
    display: flex;
    flex-direction: column;
    max-width: 520px;
    padding: 3px 0;
    border: 1px solid var(--hairline);
    border-radius: 9px;
    background: var(--bg-window);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 5px 12px;
  }

  .row-label {
    font-size: 13px;
    font-weight: 500;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .chip,
  .restore {
    padding: 3px 8px;
    border: 1px solid var(--border-chrome);
    border-radius: 6px;
    background: var(--bg-window);
    font: inherit;
    font-size: 12.5px;
    color: var(--text-primary);
    cursor: default;
  }

  /* why a fixed width, not min-width: "Press keys…" is wider than most
     combos, and a growing chip shoved the × and ↺ buttons sideways. */
  .chip {
    width: 96px;
    font-variant-numeric: tabular-nums;
  }

  .chip.unbound {
    color: var(--text-tertiary);
  }

  .chip.recording {
    border-color: var(--accent);
    color: var(--accent);
  }

  .icon {
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: 5px;
    background: none;
    font: inherit;
    font-size: 14px;
    color: var(--text-secondary);
    cursor: default;
  }

  .icon:hover {
    background: var(--bg-hover);
  }

  .keys {
    font-size: 12.5px;
    color: var(--text-secondary);
  }

  .explain {
    margin: 8px 2px 0;
    max-width: 520px;
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .note {
    color: var(--text-primary);
  }

  .error {
    margin: 8px 2px 0;
    font-size: 12px;
    color: var(--danger);
  }

  .footer {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    max-width: 520px;
    margin-bottom: 20px;
  }

  .restore {
    flex-shrink: 0;
    margin-top: 8px;
  }
</style>
