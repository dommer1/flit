import { describe, expect, it, vi } from "vitest";
import { hookBodyLinks } from "./bodyLinks";

function bodyDoc(html: string) {
  const doc = document.implementation.createHTMLDocument();
  doc.body.innerHTML = html;
  return doc;
}

function click(target: Element) {
  const event = new MouseEvent("click", { bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event;
}

describe("hookBodyLinks", () => {
  it("opens an http(s) link externally and swallows the navigation", () => {
    const open = vi.fn();
    const doc = bodyDoc('<a href="https://example.com/offer">deal</a>');
    hookBodyLinks(doc, open);

    const event = click(doc.querySelector("a")!);

    expect(open).toHaveBeenCalledWith("https://example.com/offer");
    expect(event.defaultPrevented).toBe(true);
  });

  it("opens when the click lands on an element inside the anchor", () => {
    const open = vi.fn();
    const doc = bodyDoc(
      '<a href="http://example.com"><img src="data:image/gif;base64,R0lGOD" alt="banner"></a>',
    );
    hookBodyLinks(doc, open);

    click(doc.querySelector("img")!);

    expect(open).toHaveBeenCalledWith("http://example.com");
  });

  it("tolerates whitespace around the href", () => {
    const open = vi.fn();
    const doc = bodyDoc('<a href="\n  https://example.com/x ">x</a>');
    hookBodyLinks(doc, open);

    click(doc.querySelector("a")!);

    expect(open).toHaveBeenCalledWith("https://example.com/x");
  });

  it("ignores clicks that are not on a link", () => {
    const open = vi.fn();
    const doc = bodyDoc("<p>just text</p>");
    hookBodyLinks(doc, open);

    const event = click(doc.querySelector("p")!);

    expect(open).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });

  it("reveals each link's real target as its hover tooltip", () => {
    const doc = bodyDoc(
      '<a href="https://evil.example/login" title="Your bank">bank</a>',
    );
    hookBodyLinks(doc, vi.fn());

    expect(doc.querySelector("a")!.title).toBe("https://evil.example/login");
  });

  it("swallows non-http links without opening anything", () => {
    const open = vi.fn();
    const doc = bodyDoc(
      '<a href="mailto:x@example.com">mail</a><a href="/relative">rel</a>',
    );
    hookBodyLinks(doc, open);

    for (const anchor of Array.from(doc.querySelectorAll("a"))) {
      const event = click(anchor);
      expect(event.defaultPrevented).toBe(true);
    }
    expect(open).not.toHaveBeenCalled();
  });
});
