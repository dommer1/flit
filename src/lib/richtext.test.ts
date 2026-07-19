import { describe, expect, it } from "vitest";
import { quotedPlainText, textToHtml } from "./richtext";

describe("quotedPlainText", () => {
  const p = (text: string) => ({
    type: "paragraph",
    content: text === "" ? [] : [{ type: "text", text }],
  });

  it("renders paragraphs as lines, like getText did", () => {
    const doc = { type: "doc", content: [p("hello"), p("world")] };

    expect(quotedPlainText(doc)).toBe("hello\nworld");
  });

  it("prefixes blockquote content with > per nesting level", () => {
    const doc = {
      type: "doc",
      content: [
        p("Thanks!"),
        {
          type: "blockquote",
          content: [
            p("Build Week is open."),
            { type: "blockquote", content: [p("older line")] },
          ],
        },
      ],
    };

    expect(quotedPlainText(doc)).toBe(
      "Thanks!\n> Build Week is open.\n> > older line",
    );
  });

  it("keeps the prefix on hard-break lines and trims empty quoted lines", () => {
    const doc = {
      type: "doc",
      content: [
        {
          type: "blockquote",
          content: [
            {
              type: "paragraph",
              content: [
                { type: "text", text: "one" },
                { type: "hardBreak" },
                { type: "text", text: "two" },
              ],
            },
            p(""),
          ],
        },
      ],
    };

    expect(quotedPlainText(doc)).toBe("> one\n> two\n>");
  });

  it("walks list containers without prefixing them", () => {
    const doc = {
      type: "doc",
      content: [
        {
          type: "bulletList",
          content: [
            { type: "listItem", content: [p("first")] },
            { type: "listItem", content: [p("second")] },
          ],
        },
      ],
    };

    expect(quotedPlainText(doc)).toBe("first\nsecond");
  });
});

describe("textToHtml", () => {
  it("returns an empty string for empty input", () => {
    expect(textToHtml("")).toBe("");
  });

  it("wraps each line in a paragraph", () => {
    expect(textToHtml("hello\nworld")).toBe("<p>hello</p><p>world</p>");
  });

  it("keeps blank lines as empty paragraphs", () => {
    expect(textToHtml("hi\n\nbye")).toBe("<p>hi</p><p></p><p>bye</p>");
  });

  it("escapes html so quoted text stays text", () => {
    expect(textToHtml('> <b>bold</b> & "quotes"')).toBe(
      "<p>&gt; &lt;b&gt;bold&lt;/b&gt; &amp; \"quotes\"</p>",
    );
  });
});
