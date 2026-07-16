<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Editor } from "@tiptap/core";
  import StarterKit from "@tiptap/starter-kit";
  import { textToHtml } from "./richtext";

  let {
    text = $bindable(""),
    html = $bindable(""),
    initialText = "",
    disabled = false,
    ariaLabel = "Message body",
  }: {
    /** Plain-text rendering of the content — the multipart fallback. */
    text?: string;
    /** HTML rendering; "" while the editor is empty. */
    html?: string;
    initialText?: string;
    disabled?: boolean;
    ariaLabel?: string;
  } = $props();

  let element: HTMLDivElement;
  // why an object in $state: reassigning { editor } on every transaction is
  // the documented Tiptap+Svelte pattern — isActive() has no reactivity of
  // its own, so the toolbar re-renders off this reassignment.
  let view = $state<{ editor: Editor | null }>({ editor: null });

  function syncOut(editor: Editor) {
    // why blockSeparator "\n": textToHtml maps one line to one paragraph,
    // so this exact separator makes text → editor → text a round-trip.
    text = editor.getText({ blockSeparator: "\n" });
    html = editor.isEmpty ? "" : editor.getHTML();
  }

  onMount(() => {
    const editor = new Editor({
      element,
      extensions: [StarterKit],
      content: textToHtml(initialText),
      editorProps: {
        attributes: { "aria-label": ariaLabel, role: "textbox" },
      },
      onTransaction: ({ editor }) => {
        view = { editor };
        syncOut(editor);
      },
    });
    view = { editor };
    syncOut(editor);
  });

  onDestroy(() => view.editor?.destroy());

  $effect(() => {
    view.editor?.setEditable(!disabled);
  });

  /** Toolbar model: label, isActive() key, and the command to run. */
  const controls: {
    label: string;
    mark: string;
    run: (editor: Editor) => void;
  }[] = [
    { label: "Bold", mark: "bold", run: (e) => e.chain().focus().toggleBold().run() },
    { label: "Italic", mark: "italic", run: (e) => e.chain().focus().toggleItalic().run() },
    { label: "Underline", mark: "underline", run: (e) => e.chain().focus().toggleUnderline().run() },
    { label: "Bullet list", mark: "bulletList", run: (e) => e.chain().focus().toggleBulletList().run() },
    { label: "Numbered list", mark: "orderedList", run: (e) => e.chain().focus().toggleOrderedList().run() },
  ];
</script>

<div class="editor">
  <div class="format-bar" role="toolbar" aria-label="Formatting">
    {#if view.editor}
      {#each controls as control (control.label)}
        <!-- type="button" matters: the editor lives inside the compose
             <form>, and a bare <button> would submit (send) on click. -->
        <button
          type="button"
          class="fmt"
          class:active={view.editor.isActive(control.mark)}
          aria-label={control.label}
          title={control.label}
          disabled={disabled}
          onclick={() => view.editor && control.run(view.editor)}
        >
          {#if control.label === "Bold"}<b>B</b>
          {:else if control.label === "Italic"}<i>I</i>
          {:else if control.label === "Underline"}<u>U</u>
          {:else if control.label === "Bullet list"}•≡
          {:else}1.
          {/if}
        </button>
      {/each}
    {/if}
  </div>
  <div class="content" bind:this={element}></div>
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }

  .format-bar {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
    margin: 0 20px;
    padding: 6px 0;
    border-bottom: 1px solid var(--hairline);
  }

  .fmt {
    display: grid;
    place-items: center;
    min-width: 26px;
    padding: 3px 6px;
    border: none;
    border-radius: 5px;
    background: transparent;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .fmt:hover {
    background: var(--bg-hover);
  }

  .fmt.active {
    background: var(--bg-hover);
    color: var(--accent);
  }

  .fmt:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .content {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  /* :global — Tiptap renders its own .tiptap contenteditable inside. Match
     the old textarea's look so the swap is invisible. */
  .content > :global(.tiptap) {
    flex: 1;
    padding: 14px 20px;
    font: inherit;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-primary);
  }

  .content > :global(.tiptap:focus) {
    outline: none;
  }

  /* One paragraph per line, no vertical gaps — plain-text feel. */
  .content :global(.tiptap p) {
    margin: 0;
  }

  .content :global(.tiptap ul),
  .content :global(.tiptap ol) {
    margin: 0;
    padding-left: 1.4em;
  }
</style>
