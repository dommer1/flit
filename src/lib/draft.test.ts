import { describe, expect, it } from "vitest";
import {
  composeHtmlBody,
  composePlainBody,
  isDraftEmpty,
  quoteEditorHtml,
  splitComposedHtml,
} from "./draft";
import type { OutgoingMessage } from "./types";

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
    expect(html).toContain('<div class="gmail_quote flit-draft-quote">');
    // The attribution's <angle brackets> must be escaped, not markup.
    expect(html).toContain("OpenAI &lt;dev@openai.com&gt; wrote:");
    expect(html).toContain(
      '<blockquote class="gmail_quote" type="cite" style="',
    );
    expect(html).toContain("<p>Build Week is open.</p></blockquote></div>");
  });

  it("html body separates the typed text from the quote with a break", () => {
    // why <br> and not paragraph margins: clients that normalize away <p>
    // margins (Spark, Canary) otherwise glue the attribution to the last
    // typed line. Gmail writes the same break for the same reason.
    expect(composeHtmlBody("<p>Thanks!</p>", quote)).toContain(
      '<p>Thanks!</p><br><div class="gmail_quote',
    );
  });

  it("html body labels the quote the way clients fold it", () => {
    // gmail_quote/gmail_attr is what other clients look for to collapse the
    // quoted history behind a "show more" control.
    const html = composeHtmlBody("<p>Thanks!</p>", quote);

    expect(html).toContain(
      '<div class="gmail_attr">On July 17, 2026, ' +
        "OpenAI &lt;dev@openai.com&gt; wrote:</div>",
    );
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
