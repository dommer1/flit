/** Escape text for safe embedding inside an HTML element. */
export function escapeHtml(line: string): string {
  return line
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

/** The Tiptap JSON shape we walk — only what the serializer touches. */
interface ContentNode {
  type?: string;
  text?: string;
  content?: ContentNode[];
}

/** Block types that hold inline content — one line of fallback text each. */
const TEXT_BLOCKS = new Set(["paragraph", "heading", "codeBlock"]);

/**
 * Plain-text form of editor content with blockquote nesting rendered as
 * "> " prefixes. Tiptap's own getText flattens blockquotes silently — an
 * expanded reply-quote would lose its quote markers in the multipart
 * fallback (the classic aerion/getText bug).
 */
export function quotedPlainText(doc: ContentNode): string {
  return blockLines(doc, 0).join("\n").trim();
}

function blockLines(node: ContentNode, depth: number): string[] {
  const lines: string[] = [];
  for (const child of node.content ?? []) {
    if (child.type === "blockquote") {
      lines.push(...blockLines(child, depth + 1));
    } else if (TEXT_BLOCKS.has(child.type ?? "")) {
      const prefix = "> ".repeat(depth);
      // A hard break inside a paragraph is still a line — each piece gets
      // the quote prefix of its block.
      for (const piece of inlineText(child).split("\n")) {
        lines.push((prefix + piece).trimEnd());
      }
    } else {
      lines.push(...blockLines(child, depth));
    }
  }
  return lines;
}

function inlineText(node: ContentNode): string {
  return (node.content ?? [])
    .map((child) =>
      child.type === "text"
        ? (child.text ?? "")
        : child.type === "hardBreak"
          ? "\n"
          : inlineText(child),
    )
    .join("");
}

/**
 * Plain text → editor HTML: one paragraph per line, blank lines become empty
 * paragraphs.
 *
 * why: drafts (reply quotes, reopened sends) arrive as plain text. Feeding
 * the editor one <p> per line keeps the visual line structure and round-trips
 * back through getText({ blockSeparator: "\n" }) unchanged.
 */
export function textToHtml(text: string): string {
  if (!text) return "";
  return text
    .split("\n")
    .map((line) => (line === "" ? "<p></p>" : `<p>${escapeHtml(line)}</p>`))
    .join("");
}
