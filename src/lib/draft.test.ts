import { describe, expect, it } from "vitest";
import { replyDraft } from "./draft";
import type { MessageHeader } from "./types";

const message: MessageHeader = {
  id: 7,
  accountId: 2,
  from: "Alice Doe <alice@example.com>",
  to: "Me <me@example.com>",
  cc: "",
  subject: "Weekend plans",
  snippet: "Are we still on?",
  date: "2026-07-07T09:15:00Z",
  read: true,
};

describe("replyDraft", () => {
  it("replies from the account the message arrived on", () => {
    expect(replyDraft(message, null).accountId).toBe(2);
  });

  it("addresses the sender's bare address", () => {
    expect(replyDraft(message, null).to).toBe("alice@example.com");
  });

  it("keeps a plain from field as the address", () => {
    const plain = { ...message, from: "alice@example.com" };
    expect(replyDraft(plain, null).to).toBe("alice@example.com");
  });

  it("prefixes the subject with Re:", () => {
    expect(replyDraft(message, null).subject).toBe("Re: Weekend plans");
  });

  it("does not stack Re: prefixes", () => {
    const re = { ...message, subject: "RE: Weekend plans" };
    expect(replyDraft(re, null).subject).toBe("RE: Weekend plans");
  });

  it("quotes the original text under an attribution line", () => {
    const body = replyDraft(message, "First line\nSecond line").body;

    expect(body).toContain("Alice Doe wrote:");
    expect(body).toContain("> First line");
    expect(body).toContain("> Second line");
  });

  it("leaves the body empty when the original has no text", () => {
    expect(replyDraft(message, null).body).toBe("");
  });
});
