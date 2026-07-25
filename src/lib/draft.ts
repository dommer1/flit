// Drafting logic for compose windows — pure functions, no invoke() here.
// A draft is an OutgoingMessage that has not been sent yet, so the two
// share one shape.

import { formatFullDate, senderName } from "./format";
import { splitRecipients } from "./recipients";
import { escapeHtml } from "./richtext";
import type {
  Alias,
  DraftQuote,
  MessageHeader,
  MessageQuote,
  OutgoingMessage,
} from "./types";

/** Bare address out of `Name <addr>`; a plain address passes through. */
function senderAddress(from: string): string {
  const match = from.match(/<([^<>]+)>\s*$/);
  return (match?.[1] ?? from).trim();
}

/** The three ways a message in the viewer can spawn a compose draft. */
export type DraftKind = "reply" | "reply-all" | "forward";

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

function replySubject(subject: string): string {
  const trimmed = subject.trim();
  return /^re:/i.test(trimmed) ? trimmed : `Re: ${trimmed}`;
}

function forwardSubject(subject: string): string {
  const trimmed = subject.trim();
  return /^fwd:/i.test(trimmed) ? trimmed : `Fwd: ${trimmed}`;
}

/** The quote field of a reply draft: material + the attribution line built
 * for it. Absent when the original had nothing quotable. */
function draftQuote(
  message: MessageHeader,
  quote: MessageQuote | null,
): Pick<OutgoingMessage, "quote"> {
  if (!quote || (quote.html === "" && quote.text === "")) return {};
  return {
    quote: {
      attribution: `On ${formatFullDate(message.date)}, ${senderName(message.from)} wrote:`,
      html: quote.html,
      text: quote.text,
    },
  };
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

/** Threading identity a reply carries so recipients (and our own Sent copy)
 * group it into the conversation: answer the original's Message-ID and
 * extend its References chain. Empty when the original has no Message-ID —
 * there is nothing to thread on. */
function threading(
  message: MessageHeader,
): Pick<OutgoingMessage, "inReplyTo" | "references"> {
  if (!message.messageId) return {};
  const chain = message.references.split(/\s+/).filter(Boolean);
  return {
    inReplyTo: message.messageId,
    references: [...chain, message.messageId].join(" "),
  };
}

/** The account's alias the message was addressed to, if any — a reply
 * should leave from the same address the mail arrived on. */
function matchAlias(
  message: MessageHeader,
  aliases: Alias[],
): Alias | undefined {
  const recipients = new Set(
    [...splitRecipients(message.to), ...splitRecipients(message.cc)].map(
      (entry) => senderAddress(entry).toLowerCase(),
    ),
  );
  return aliases.find(
    (alias) =>
      alias.accountId === message.accountId &&
      recipients.has(alias.email.toLowerCase()),
  );
}

/** Reply goes from the account the message arrived on — and from the alias
 * it was addressed to, when one matches — to its sender. Replying to my own
 * sent message is a follow-up: it goes to the original recipients (mailing
 * myself would be useless) and keeps the identity it originally left from. */
export function replyDraft(
  message: MessageHeader,
  quote: MessageQuote | null,
  aliases: Alias[] = [],
  ownEmail = "",
): OutgoingMessage {
  const fromAddress = senderAddress(message.from).toLowerCase();
  const sentFromAlias = aliases.find(
    (alias) =>
      alias.accountId === message.accountId &&
      alias.email.toLowerCase() === fromAddress,
  );
  const isOwn =
    fromAddress === ownEmail.trim().toLowerCase() ||
    sentFromAlias !== undefined;
  return {
    accountId: message.accountId,
    aliasId: isOwn ? sentFromAlias?.id : matchAlias(message, aliases)?.id,
    // why the fallback: an own message with an empty To line would leave the
    // draft unsendable — degrade to replying to myself, like replyAllDraft.
    to:
      isOwn && message.to.trim() !== ""
        ? message.to
        : senderAddress(message.from),
    subject: replySubject(message.subject),
    // The editor opens empty; the quote rides separately and is merged
    // into body/bodyHtml at save/send time (composePlainBody/composeHtmlBody).
    body: "",
    ...draftQuote(message, quote),
    ...threading(message),
  };
}

/** Reply-all: sender + the original To line on To, the original Cc line on
 * Cc — minus the receiving account's own address and any duplicates. */
export function replyAllDraft(
  message: MessageHeader,
  quote: MessageQuote | null,
  ownEmail: string,
  aliases: Alias[] = [],
): OutgoingMessage {
  // why all account aliases, not just the matched one: every alias is "me" —
  // none of them belongs on the recipient lines of my own reply.
  const seen = new Set([
    ownEmail.trim().toLowerCase(),
    ...aliases
      .filter((a) => a.accountId === message.accountId)
      .map((a) => a.email.toLowerCase()),
  ]);
  const keep = (entry: string) => {
    const address = senderAddress(entry).toLowerCase();
    if (address === "" || seen.has(address)) return false;
    seen.add(address);
    return true;
  };
  const to = [message.from, ...splitRecipients(message.to)].filter(keep);
  const cc = splitRecipients(message.cc).filter(keep);
  return {
    accountId: message.accountId,
    aliasId: matchAlias(message, aliases)?.id,
    // why: everyone else can be me (replying to my own mail) — degrade to a
    // plain reply-to-sender rather than an unsendable empty To.
    to: to.length > 0 ? to.join(", ") : senderAddress(message.from),
    cc: cc.join(", "),
    subject: replySubject(message.subject),
    body: "",
    ...draftQuote(message, quote),
    ...threading(message),
  };
}

/** Forward: no recipient yet, the original riding below a header block. */
export function forwardDraft(
  message: MessageHeader,
  bodyText: string | null,
  aliases: Alias[] = [],
): OutgoingMessage {
  const header = [
    "---------- Forwarded message ----------",
    `From: ${message.from}`,
    `Date: ${formatFullDate(message.date)}`,
    `Subject: ${message.subject}`,
    `To: ${message.to}`,
    ...(message.cc !== "" ? [`Cc: ${message.cc}`] : []),
  ].join("\n");
  return {
    accountId: message.accountId,
    aliasId: matchAlias(message, aliases)?.id,
    to: "",
    subject: forwardSubject(message.subject),
    body: `\n\n${header}\n\n${bodyText ?? ""}`.trimEnd().concat("\n"),
  };
}
