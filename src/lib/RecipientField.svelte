<script lang="ts">
  import { listContacts } from "./api";
  import { activeTerm, applySuggestion } from "./recipients";
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

  let inputEl = $state<HTMLInputElement | null>(null);
  let suggestions = $state<Contact[]>([]);
  let highlighted = $state(0);

  function caret(): number {
    return inputEl?.selectionStart ?? value.length;
  }

  async function refresh() {
    const term = activeTerm(value, caret());
    if (term === "") {
      suggestions = [];
      return;
    }
    try {
      const found = await listContacts(term);
      // why re-check: a slower lookup must not overwrite the dropdown for
      // whatever the user is typing now.
      if (activeTerm(value, caret()) === term) {
        suggestions = found;
        highlighted = 0;
      }
    } catch {
      // Suggestions are a convenience — typing goes on without them.
      suggestions = [];
    }
  }

  function pick(contact: Contact) {
    value = applySuggestion(value, caret(), contact.email);
    suggestions = [];
    inputEl?.focus();
  }

  function onKeydown(event: KeyboardEvent) {
    if (suggestions.length === 0) return;
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        highlighted = (highlighted + 1) % suggestions.length;
        break;
      case "ArrowUp":
        event.preventDefault();
        highlighted = (highlighted - 1 + suggestions.length) % suggestions.length;
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
  }
</script>

<div class="field">
  <input
    bind:this={inputEl}
    aria-label={label}
    bind:value
    {required}
    {disabled}
    {placeholder}
    autocomplete="off"
    spellcheck="false"
    oninput={() => void refresh()}
    onkeydown={onKeydown}
    onblur={() => (suggestions = [])}
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
    flex: 1;
    min-width: 0;
  }

  /* Matches the borderless envelope inputs of the compose window. */
  input {
    flex: 1;
    min-width: 0;
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
