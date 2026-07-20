import { describe, expect, it } from "vitest";
import {
  composeHtmlBody,
  composePlainBody,
  forwardDraft,
  isDraftEmpty,
  quoteEditorHtml,
  replyAllDraft,
  replyDraft,
  splitComposedHtml,
} from "./draft";
import type { Alias, MessageHeader, OutgoingMessage } from "./types";

const message: MessageHeader = {
  id: 7,
  accountId: 2,
  mailbox: "INBOX",
  from: "Alice Doe <alice@example.com>",
  to: "Me <me@example.com>",
  cc: "",
  replyTo: "",
    bcc: "",
  messageId: "",
  references: "",
  threadCount: 1,
  threadUnread: false,
  isDraft: false,
  subject: "Weekend plans",
  snippet: "Are we still on?",
  date: "2026-07-07T09:15:00Z",
  read: true,
  hasAttachments: false,
};

const aliases: Alias[] = [
  { id: 10, accountId: 2, name: "Igor", email: "igor@vocalio.sk" },
  { id: 11, accountId: 2, name: "", email: "dominik@vocalio.sk" },
  { id: 12, accountId: 1, name: "", email: "other@example.com" },
];

describe("send-as alias matching", () => {
  it("a reply leaves from the alias the mail was addressed to", () => {
    const toAlias = { ...message, to: "Igor <igor@vocalio.sk>" };
    expect(replyDraft(toAlias, null, aliases).aliasId).toBe(10);
  });

  it("matches an alias case-insensitively and in the Cc line", () => {
    const ccAlias = { ...message, cc: "DOMINIK@vocalio.sk" };
    expect(replyDraft(ccAlias, null, aliases).aliasId).toBe(11);
  });

  it("ignores another account's alias with the same address", () => {
    const toForeign = { ...message, to: "other@example.com" };
    expect(replyDraft(toForeign, null, aliases).aliasId).toBeUndefined();
  });

  it("leaves the alias unset when the mail came to the account itself", () => {
    expect(replyDraft(message, null, aliases).aliasId).toBeUndefined();
  });

  it("reply-all carries the matched alias too", () => {
    const toAlias = { ...message, to: "igor@vocalio.sk" };
    expect(
      replyAllDraft(toAlias, null, "me@example.com", aliases).aliasId,
    ).toBe(10);
  });

  it("reply-all never mails the account's own aliases back", () => {
    const toBoth = {
      ...message,
      to: "igor@vocalio.sk, Bob <bob@example.com>",
      cc: "dominik@vocalio.sk",
    };
    const draft = replyAllDraft(toBoth, null, "me@example.com", aliases);

    expect(draft.to).toBe("Alice Doe <alice@example.com>, Bob <bob@example.com>");
    expect(draft.cc).toBe("");
  });

  it("a forward keeps the identity the mail was addressed to", () => {
    const toAlias = { ...message, to: "igor@vocalio.sk" };
    expect(forwardDraft(toAlias, null, aliases).aliasId).toBe(10);
  });
});

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

  it("opens with an empty body and carries the quote separately", () => {
    const draft = replyDraft(message, {
      html: "<p>First line</p>",
      text: "First line",
    });

    expect(draft.body).toBe("");
    expect(draft.quote?.attribution).toContain("Alice Doe wrote:");
    expect(draft.quote?.html).toBe("<p>First line</p>");
    expect(draft.quote?.text).toBe("First line");
  });

  it("carries no quote when the original has no content", () => {
    expect(replyDraft(message, null).quote).toBeUndefined();
    expect(replyDraft(message, { html: "", text: "" }).quote).toBeUndefined();
  });

  it("stamps the reply with the original's threading identity", () => {
    const threaded = { ...message, messageId: "mid@x", references: "root@x" };

    const draft = replyDraft(threaded, null);

    expect(draft.inReplyTo).toBe("mid@x");
    expect(draft.references).toBe("root@x mid@x");
  });

  it("starts the chain at the original when it had no References", () => {
    const first = { ...message, messageId: "mid@x" };

    expect(replyDraft(first, null).references).toBe("mid@x");
  });

  it("omits threading when the original has no Message-ID", () => {
    const draft = replyDraft(message, null);

    expect(draft.inReplyTo).toBeUndefined();
    expect(draft.references).toBeUndefined();
  });
});

describe("reply to own message", () => {
  // A follow-up on my own sent mail: account 2's address is hello@vocalio.sk.
  const own = {
    ...message,
    from: "Me <hello@vocalio.sk>",
    to: "Alice Doe <alice@example.com>, bob@example.com",
  };

  it("addresses the original recipients instead of myself", () => {
    const draft = replyDraft(own, null, [], "hello@vocalio.sk");
    expect(draft.to).toBe("Alice Doe <alice@example.com>, bob@example.com");
  });

  it("compares my address case-insensitively", () => {
    const draft = replyDraft(own, null, [], "HELLO@vocalio.sk");
    expect(draft.to).toBe("Alice Doe <alice@example.com>, bob@example.com");
  });

  it("treats mail sent from my alias as my own and keeps that identity", () => {
    const fromAlias = { ...own, from: "Igor <igor@vocalio.sk>" };
    const draft = replyDraft(fromAlias, null, aliases, "hello@vocalio.sk");
    expect(draft.to).toBe("Alice Doe <alice@example.com>, bob@example.com");
    expect(draft.aliasId).toBe(10);
  });

  it("ignores another account's alias when judging ownership", () => {
    const foreign = { ...own, from: "other@example.com" };
    const draft = replyDraft(foreign, null, aliases, "hello@vocalio.sk");
    expect(draft.to).toBe("other@example.com");
  });

  it("falls back to the sender when my message had no recipients", () => {
    const draft = replyDraft({ ...own, to: "" }, null, [], "hello@vocalio.sk");
    expect(draft.to).toBe("hello@vocalio.sk");
  });
});

describe("replyAllDraft", () => {
  const group: MessageHeader = {
    ...message,
    to: "Me <me@example.com>, Bob <bob@example.com>",
    cc: "carol@example.com, Me <me@example.com>",
  };

  it("addresses the sender plus the other To recipients, minus me", () => {
    const draft = replyAllDraft(group, null, "me@example.com");

    expect(draft.to).toBe(
      "Alice Doe <alice@example.com>, Bob <bob@example.com>",
    );
  });

  it("keeps the surviving Cc recipients on the Cc line", () => {
    expect(replyAllDraft(group, null, "me@example.com").cc).toBe(
      "carol@example.com",
    );
  });

  it("compares addresses case-insensitively", () => {
    const draft = replyAllDraft(group, null, "ME@Example.com");

    expect(draft.to).not.toContain("me@example.com");
    expect(draft.cc).toBe("carol@example.com");
  });

  it("drops recipients that repeat across To and Cc", () => {
    const dup = { ...group, cc: "Bob <bob@example.com>" };

    const draft = replyAllDraft(dup, null, "me@example.com");

    expect(draft.to).toBe(
      "Alice Doe <alice@example.com>, Bob <bob@example.com>",
    );
    expect(draft.cc).toBe("");
  });

  it("keeps display names whose comma would split naively", () => {
    const comma = { ...group, to: '"Novák, Ján" <jan@example.com>' };

    expect(replyAllDraft(comma, null, "me@example.com").to).toBe(
      'Alice Doe <alice@example.com>, "Novák, Ján" <jan@example.com>',
    );
  });

  it("falls back to the sender when everyone else was me", () => {
    const solo = {
      ...message,
      from: "Me <me@example.com>",
      to: "me@example.com",
      cc: "",
    };

    expect(replyAllDraft(solo, null, "me@example.com").to).toBe(
      "me@example.com",
    );
  });

  it("prefixes the subject with Re: and quotes like a plain reply", () => {
    const draft = replyAllDraft(
      group,
      { html: "<p>Hello</p>", text: "Hello" },
      "me@example.com",
    );

    expect(draft.subject).toBe("Re: Weekend plans");
    expect(draft.quote?.text).toBe("Hello");
    expect(draft.accountId).toBe(2);
  });

  it("stamps reply-all with the original's threading identity", () => {
    const threaded = { ...group, messageId: "mid@x", references: "root@x" };

    const draft = replyAllDraft(threaded, null, "me@example.com");

    expect(draft.inReplyTo).toBe("mid@x");
    expect(draft.references).toBe("root@x mid@x");
  });
});

describe("forwardDraft", () => {
  it("leaves the recipient empty and prefixes the subject with Fwd:", () => {
    const draft = forwardDraft(message, null);

    expect(draft.to).toBe("");
    expect(draft.subject).toBe("Fwd: Weekend plans");
    expect(draft.accountId).toBe(2);
  });

  it("does not stack Fwd: prefixes", () => {
    const fwd = { ...message, subject: "FWD: Weekend plans" };

    expect(forwardDraft(fwd, null).subject).toBe("FWD: Weekend plans");
  });

  it("embeds the original headers and text in a forwarded block", () => {
    const body = forwardDraft(message, "Original text").body;

    expect(body).toContain("---------- Forwarded message ----------");
    expect(body).toContain("From: Alice Doe <alice@example.com>");
    expect(body).toContain("Subject: Weekend plans");
    expect(body).toContain("To: Me <me@example.com>");
    expect(body).toContain("Original text");
    expect(body).not.toContain("Cc:");
  });

  it("names the Cc recipients when the original had any", () => {
    const cc = { ...message, cc: "carol@example.com" };

    expect(forwardDraft(cc, "x").body).toContain("Cc: carol@example.com");
  });

  it("still produces the block when the original has no text", () => {
    const body = forwardDraft(message, null).body;

    expect(body).toContain("---------- Forwarded message ----------");
    expect(body).toContain("From: Alice Doe <alice@example.com>");
  });

  it("starts a fresh thread — no threading headers", () => {
    const threaded = { ...message, messageId: "mid@x", references: "root@x" };

    const draft = forwardDraft(threaded, null);

    expect(draft.inReplyTo).toBeUndefined();
    expect(draft.references).toBeUndefined();
  });
});

describe("composing a draft with its quote", () => {
  const quote = {
    attribution: "On July 17, 2026, OpenAI <dev@openai.com> wrote:",
    html: "<p>Build Week is open.</p>",
    text: "Build Week is open.\n\nSubmissions close July 21.",
  };

  it("plain body rides the quote below the attribution as > lines", () => {
    expect(composePlainBody("Thanks!", quote)).toBe(
      "Thanks!\n\n" +
        "On July 17, 2026, OpenAI <dev@openai.com> wrote:\n" +
        "> Build Week is open.\n>\n> Submissions close July 21.\n",
    );
  });

  it("plain body passes through without a quote", () => {
    expect(composePlainBody("Thanks!", null)).toBe("Thanks!");
    expect(composePlainBody("Thanks!", undefined)).toBe("Thanks!");
  });

  it("html body appends a marked, inline-styled blockquote", () => {
    const html = composeHtmlBody("<p>Thanks!</p>", quote);

    expect(html.startsWith("<p>Thanks!</p>")).toBe(true);
    expect(html).toContain('<div class="flit-draft-quote">');
    // The attribution's <angle brackets> must be escaped, not markup.
    expect(html).toContain("OpenAI &lt;dev@openai.com&gt; wrote:");
    expect(html).toContain('<blockquote type="cite" style="');
    expect(html).toContain("<p>Build Week is open.</p></blockquote></div>");
  });

  it("quoteEditorHtml escapes the attribution and drops the marker", () => {
    const html = quoteEditorHtml(quote);

    expect(html).toBe(
      "<p></p><p>On July 17, 2026, OpenAI &lt;dev@openai.com&gt; wrote:</p>" +
        "<blockquote><p>Build Week is open.</p></blockquote>",
    );
  });

  it("splitComposedHtml gives the editor back only its own content", () => {
    const html = composeHtmlBody("<p>Thanks!</p>", quote);

    expect(splitComposedHtml(html)).toEqual({
      own: "<p>Thanks!</p>",
      hasQuote: true,
    });
    expect(splitComposedHtml("<p>No quote</p>")).toEqual({
      own: "<p>No quote</p>",
      hasQuote: false,
    });
  });
});

describe("isDraftEmpty", () => {
  const blank: OutgoingMessage = {
    accountId: 1,
    to: "",
    subject: "",
    body: "",
  };

  it("treats a blank message as empty", () => {
    expect(isDraftEmpty(blank)).toBe(true);
  });

  it("treats whitespace-only fields as empty", () => {
    expect(isDraftEmpty({ ...blank, to: "  ", body: "\n" })).toBe(true);
  });

  it("keeps a message with any typed text", () => {
    expect(isDraftEmpty({ ...blank, to: "jan" })).toBe(false);
    expect(isDraftEmpty({ ...blank, bcc: "x@y" })).toBe(false);
    expect(isDraftEmpty({ ...blank, body: "ahoj" })).toBe(false);
  });

  it("keeps a message that only has an attachment", () => {
    const withFile = {
      ...blank,
      attachments: [{ path: "/tmp/a.txt", name: "a.txt" }],
    };
    expect(isDraftEmpty(withFile)).toBe(false);
  });
});
