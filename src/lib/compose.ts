// Drafting logic for the compose form — pure functions, no invoke() here.

import { formatFullDate, senderName } from "./format";
import type { MessageHeader } from "./types";

/** What the compose form opens with; accountId picks the From account. */
export interface ComposeDraft {
  accountId: number;
  to: string;
  subject: string;
  body: string;
}

/** Bare address out of `Name <addr>`; a plain address passes through. */
function senderAddress(from: string): string {
  const match = from.match(/<([^<>]+)>\s*$/);
  return (match?.[1] ?? from).trim();
}

function replySubject(subject: string): string {
  const trimmed = subject.trim();
  return /^re:/i.test(trimmed) ? trimmed : `Re: ${trimmed}`;
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
): ComposeDraft {
  return {
    accountId: message.accountId,
    to: senderAddress(message.from),
    subject: replySubject(message.subject),
    body: bodyText ? quote(message, bodyText) : "",
  };
}
