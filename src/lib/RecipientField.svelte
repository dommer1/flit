<script lang="ts">
  import { listContacts } from "./api";
  import { joinRecipients, splitRecipients } from "./recipients";
  import type { Contact } from "./types";

  let {
    label,
    value = $bindable(""),
    disabled = false,
    required = false,
    placeholder = "",
  }: {
    label: string;
    value?: string;
    disabled?: boolean;
    required?: boolean;
    placeholder?: string;
  } = $props();

  /** Committed recipients, rendered as removable chips. */
  let chips = $state<string[]>([]);
  /** The address still being typed after the last chip. */
  let text = $state("");

  let inputEl = $state<HTMLInputElement | null>(null);
  let suggestions = $state<Contact[]>([]);
  let highlighted = $state(0);

  // why: the parent binds `value` (the comma-separated wire format) while
  // this component thinks in chips + typed fragment. Internal edits keep the
  // two equal via sync(); a write from outside (draft prefill) shows up as a
  // mismatch and is parsed into chips. Re-normalizing `value` afterwards
  // makes the effect converge after one pass instead of looping.
  $effect(() => {
    if (value !== joinRecipients(chips, text)) {
      chips = splitRecipients(value);
      text = "";
      value = joinRecipients(chips, "");
    }
  });

  function sync() {
    value = joinRecipients(chips, text);
  }

  function commitText() {
    const address = text.trim();
    if (address !== "") chips = [...chips, address];
    text = "";
    sync();
  }

  function removeChip(index: number) {
    chips = chips.filter((_, i) => i !== index);
    sync();
    inputEl?.focus();
  }

  async function refresh() {
    const term = text.trim();
    if (term === "") {
      suggestions = [];
      return;
    }
    try {
      const found = await listContacts(term);
      // why re-check: a slower lookup must not overwrite the dropdown for
      // whatever the user is typing now.
      if (text.trim() === term) {
        suggestions = found;
        highlighted = 0;
      }
    } catch {
      // Suggestions are a convenience — typing goes on without them.
      suggestions = [];
    }
  }

  function pick(contact: Contact) {
    chips = [...chips, contact.email];
    text = "";
    sync();
    suggestions = [];
    inputEl?.focus();
  }

  function onInput(event: Event) {
    const raw = (event.currentTarget as HTMLInputElement).value;
    if (raw.includes(",")) {
      // A comma commits everything before it; pasting a whole list turns
      // all complete entries into chips at once, keeping an open tail
      // (no trailing comma) editable.
      const parts = splitRecipients(raw);
      const pending = /,\s*$/.test(raw) ? "" : (parts.pop() ?? "");
      if (parts.length > 0) chips = [...chips, ...parts];
      text = pending;
    } else {
      text = raw;
    }
    // why the manual write-back: committing "a@x," leaves `text` at "" both
    // before and after, so Svelte sees nothing to re-render — the leftover
    // comma has to be cleared out of the DOM by hand.
    if (inputEl && inputEl.value !== text) inputEl.value = text;
    sync();
    void refresh();
  }

  function onKeydown(event: KeyboardEvent) {
    if (suggestions.length > 0) {
      switch (event.key) {
        case "ArrowDown":
          event.preventDefault();
          highlighted = (highlighted + 1) % suggestions.length;
          break;
        case "ArrowUp":
          event.preventDefault();
          highlighted =
            (highlighted - 1 + suggestions.length) % suggestions.length;
          break;
        // why Tab too: both keys "accept" in every mail client's To field.
        // preventDefault stops Enter from submitting the compose form.
        case "Enter":
        case "Tab":
          event.preventDefault();
          pick(suggestions[highlighted]);
          break;
        case "Escape":
          suggestions = [];
          break;
      }
      return;
    }
    if (event.key === "Enter" && text.trim() !== "") {
      // why: Enter finishes the address instead of firing the form's send —
      // with something half-typed, submitting is never what was meant.
      event.preventDefault();
      commitText();
    } else if (event.key === "Tab" && text.trim() !== "") {
      // No preventDefault — the fresh chip stays and focus moves on.
      commitText();
    } else if (event.key === "Backspace" && text === "" && chips.length > 0) {
      removeChip(chips.length - 1);
    }
  }
</script>

<div class="field">
  {#each chips as chip, index (`${index}-${chip}`)}
    <span class="chip">
      <span class="chip-label">{chip}</span>
      <button
        type="button"
        class="chip-remove"
        aria-label={`Remove ${chip}`}
        onclick={() => removeChip(index)}
        {disabled}
      >
        ×
      </button>
    </span>
  {/each}
  <input
    bind:this={inputEl}
    aria-label={label}
    value={text}
    required={required && chips.length === 0}
    {disabled}
    placeholder={chips.length === 0 ? placeholder : ""}
    autocomplete="off"
    spellcheck="false"
    oninput={onInput}
    onkeydown={onKeydown}
    onblur={() => {
      suggestions = [];
      commitText();
    }}
  />
  {#if suggestions.length > 0}
    <ul class="suggestions" role="listbox" aria-label="{label} suggestions">
      {#each suggestions as contact, index (contact.email)}
        <li>
          <!-- why mousedown: it fires before the input's blur, so picking
               with the mouse wins the race against the dropdown closing. -->
          <button
            type="button"
            role="option"
            aria-selected={index === highlighted}
            class:highlighted={index === highlighted}
            onmousedown={(e) => {
              e.preventDefault();
              pick(contact);
            }}
          >
            {#if contact.name}
              <span class="name">{contact.name}</span>
            {/if}
            <span class="email">{contact.email}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .field {
    position: relative;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 6px;
    flex: 1;
    min-width: 0;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    max-width: 100%;
    padding: 1px 4px 1px 9px;
    border: 1px solid var(--hairline);
    border-radius: 999px;
    background: var(--bg-hover);
    font-size: 12px;
    color: var(--text-primary);
    white-space: nowrap;
  }

  .chip-label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .chip-remove {
    display: grid;
    place-items: center;
    width: 15px;
    height: 15px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    font-size: 12px;
    line-height: 1;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .chip-remove:hover {
    background: color-mix(in srgb, var(--text-secondary) 18%, transparent);
    color: var(--text-primary);
  }

  .chip-remove:disabled {
    opacity: 0.45;
    cursor: default;
  }

  /* Matches the borderless envelope inputs of the compose window. */
  input {
    flex: 1;
    min-width: 140px;
    padding: 0;
    border: none;
    background: transparent;
    font: inherit;
    font-size: 13px;
    color: var(--text-primary);
  }

  input:focus {
    outline: none;
  }

  input::placeholder {
    color: var(--text-tertiary);
  }

  .suggestions {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    min-width: 240px;
    max-width: 360px;
    margin: 0;
    padding: 4px;
    border: 1px solid var(--hairline);
    border-radius: 8px;
    background: var(--bg-window);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.16);
    list-style: none;
  }

  .suggestions button {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    padding: 5px 10px;
    border: none;
    border-radius: 5px;
    background: none;
    font: inherit;
    font-size: 13px;
    text-align: left;
    color: var(--text-primary);
    cursor: pointer;
  }

  .suggestions button.highlighted,
  .suggestions button:hover {
    background: var(--bg-hover);
  }

  .name {
    flex-shrink: 0;
    font-weight: 500;
  }

  .email {
    overflow: hidden;
    color: var(--text-secondary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
