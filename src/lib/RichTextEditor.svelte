<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Editor, generateHTML, generateJSON } from "@tiptap/core";
  import StarterKit from "@tiptap/starter-kit";
  import { textToHtml } from "./richtext";

  const EXTENSIONS = [StarterKit];

  /** Any HTML → the exact string this editor's getHTML() would produce for
   * it, so stored fragments become string-comparable with live content. */
  function normalize(html: string): string {
    return html ? generateHTML(generateJSON(html, EXTENSIONS), EXTENSIONS) : "";
  }

  let {
    text = $bindable(""),
    html = $bindable(""),
    initialText = "",
    initialHtml = "",
    disabled = false,
    ariaLabel = "Message body",
  }: {
    /** Plain-text rendering of the content — the multipart fallback. */
    text?: string;
    /** HTML rendering; "" while the editor is empty. */
    html?: string;
    initialText?: string;
    /** Initial content as HTML; wins over initialText when both are set. */
    initialHtml?: string;
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
    // so this separator turns paragraphs back into lines.
    // why trim: nested blocks (list items) and the editor's own trailing
    // paragraph leak extra separators at the edges of the fallback text.
    text = editor.getText({ blockSeparator: "\n" }).trim();
    html = editor.isEmpty ? "" : editor.getHTML();
  }

  onMount(() => {
    const editor = new Editor({
      element,
      extensions: EXTENSIONS,
      content: initialHtml || textToHtml(initialText),
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

  /**
   * Replace one HTML block with another (signature switching): the last
   * occurrence of `previousHtml` is swapped for `nextHtml`. When the old
   * block is gone (edited away, or there was none), the new one is appended
   * after a blank paragraph instead — the least surprising fallback.
   */
  export function swapBlock(previousHtml: string, nextHtml: string) {
    const editor = view.editor;
    if (!editor) return;
    const prev = normalize(previousHtml);
    const next = normalize(nextHtml);
    const current = editor.getHTML();
    if (prev && current.includes(prev)) {
      const at = current.lastIndexOf(prev);
      editor.commands.setContent(
        current.slice(0, at) + next + current.slice(at + prev.length),
      );
    } else if (next) {
      editor.commands.setContent(`${current}<p></p>${next}`);
    }
  }

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
    overflow-x: hidden;
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
