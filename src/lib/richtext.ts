/** Escape text for safe embedding inside an HTML element. */
function escapeHtml(line: string): string {
  return line
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
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
