// Drafting logic for compose windows — pure functions, no invoke() here.
// A draft is an OutgoingMessage that has not been sent yet, so the two
// share one shape.

import { formatFullDate, senderName } from "./format";
import type { MessageHeader, OutgoingMessage } from "./types";

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

/** Split a display list on commas, except commas inside a display name —
 * a piece without an @ belongs to the name before the address
 * (`"Novák, Ján" <jan@x>`), so it is glued back onto the previous piece. */
function splitRecipients(list: string): string[] {
  const entries: string[] = [];
  for (const piece of list.split(",")) {
    const trimmed = piece.trim();
    if (trimmed === "") continue;
    const last = entries.length - 1;
    if (last >= 0 && !entries[last].includes("@")) {
      entries[last] += `, ${trimmed}`;
    } else {
      entries.push(trimmed);
    }
  }
  return entries;
}

function quote(message: MessageHeader, bodyText: string): string {
  const quoted = bodyText
    .trimEnd()
    .split("\n")
    .map((line) => `> ${line}`)
    .join("\n");
  const attribution = `On ${formatFullDate(message.date)}, ${senderName(message.from)} wrote:`;
  return `\n\n${attribution}\n${quoted}\n`;
}

/** Reply goes from the account the message arrived on, to its sender. */
export function replyDraft(
  message: MessageHeader,
  bodyText: string | null,
): OutgoingMessage {
  return {
    accountId: message.accountId,
    to: senderAddress(message.from),
    subject: replySubject(message.subject),
    body: bodyText ? quote(message, bodyText) : "",
  };
}

/** Reply-all: sender + the original To line on To, the original Cc line on
 * Cc — minus the receiving account's own address and any duplicates. */
export function replyAllDraft(
  message: MessageHeader,
  bodyText: string | null,
  ownEmail: string,
): OutgoingMessage {
  const seen = new Set([ownEmail.trim().toLowerCase()]);
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
    // why: everyone else can be me (replying to my own mail) — degrade to a
    // plain reply-to-sender rather than an unsendable empty To.
    to: to.length > 0 ? to.join(", ") : senderAddress(message.from),
    cc: cc.join(", "),
    subject: replySubject(message.subject),
    body: bodyText ? quote(message, bodyText) : "",
  };
}

/** Forward: no recipient yet, the original riding below a header block. */
export function forwardDraft(
  message: MessageHeader,
  bodyText: string | null,
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
    to: "",
    subject: forwardSubject(message.subject),
    body: `\n\n${header}\n\n${bodyText ?? ""}`.trimEnd().concat("\n"),
  };
}
