import { describe, expect, it } from "vitest";
import { textToHtml } from "./richtext";

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
