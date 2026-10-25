// Drafting logic for compose windows — pure functions, no invoke() here.
// A draft is an OutgoingMessage that has not been sent yet, so the two
// share one shape. Reply and forward drafts are built by the backend
// (mail::reply, through reply_draft); this is what the editor does with one.

import { escapeHtml } from "./richtext";
import type { DraftQuote, OutgoingMessage } from "./types";

/** Nothing worth keeping: every text field blank, no attachments. An empty
 * compose window is never saved to the server's Drafts folder. */
export function isDraftEmpty(message: OutgoingMessage): boolean {
  const fields = [
    message.to,
    message.cc ?? "",
    message.bcc ?? "",
    message.subject,
    message.body,
  ];
  return (
    fields.every((field) => field.trim() === "") &&
    (message.attachments?.length ?? 0) === 0
  );
}

/** Marker the composed bodyHtml wraps its quote block in — the split point
 * when a reopened draft separates editor content from the quote again.
 *
 * why the leading <br>: clients that normalize <p> margins away (Spark,
 * Canary) otherwise glue the attribution to the last typed line. Gmail
 * writes the same break. It sits INSIDE the marker so a save/reopen round
 * trip splits it off with the quote instead of stacking another one.
 *
 * why gmail_quote: the class other clients key their "fold the quoted
 * history" heuristic on — without it the whole original stays expanded. */
const QUOTE_MARKER = '<br><div class="gmail_quote flit-draft-quote">';

/** Inline-styled (Gmail-style bar) so every recipient client renders it —
 * a receiving Flit recognizes the <blockquote> and folds it as history. */
const QUOTE_BLOCK_STYLE =
  "margin:0 0 0 0.8ex;border-left:2px solid #c8ccd4;padding-left:1ex";

/** The outgoing plain-text body: the editor's text with the quote riding
 * below the attribution as "> " lines. */
export function composePlainBody(
  editorText: string,
  quote?: DraftQuote | null,
): string {
  if (!quote) return editorText;
  const quoted = quote.text
    .split("\n")
    .map((line) => (line === "" ? ">" : `> ${line}`))
    .join("\n");
  return `${editorText.trimEnd()}\n\n${quote.attribution}\n${quoted}\n`;
}

/** The outgoing HTML body: the editor's HTML with the quote block appended
 * under the attribution line. */
export function composeHtmlBody(
  editorHtml: string,
  quote?: DraftQuote | null,
): string {
  if (!quote) return editorHtml;
  return (
    `${editorHtml}${QUOTE_MARKER}` +
    `<div class="gmail_attr">${escapeHtml(quote.attribution)}</div>` +
    `<blockquote class="gmail_quote" type="cite" style="${QUOTE_BLOCK_STYLE}">` +
    `${quote.html}</blockquote></div>`
  );
}

/** The quote as editor content — what expanding the ••• block inserts:
 * attribution paragraph + blockquote, no marker div. From then on the
 * editor owns the quote; it ships as ordinary edited content. */
export function quoteEditorHtml(quote: DraftQuote): string {
  return `<p></p><p>${escapeHtml(quote.attribution)}</p><blockquote>${quote.html}</blockquote>`;
}

/** Separate a composed bodyHtml back into the editor's own content and the
 * quote block ("did it carry one") — a reopened draft (undo, failed send)
 * must not feed the quote into the editor, where it would be mangled. */
export function splitComposedHtml(bodyHtml: string): {
  own: string;
  hasQuote: boolean;
} {
  const at = bodyHtml.indexOf(QUOTE_MARKER);
  if (at < 0) return { own: bodyHtml, hasQuote: false };
  return { own: bodyHtml.slice(0, at), hasQuote: true };
}
