import { expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import RichTextEditor from "./RichTextEditor.svelte";

it("renders the initial text as editable content", async () => {
  render(RichTextEditor, { props: { initialText: "hello\nworld" } });

  const box = await screen.findByRole("textbox", { name: "Message body" });
  expect(box).toHaveTextContent("hello");
  expect(box).toHaveTextContent("world");
  // One paragraph per input line, so the line structure survives.
  expect(box.querySelectorAll("p")).toHaveLength(2);
});

it("marks the bold control active once toggled", async () => {
  render(RichTextEditor, { props: { initialText: "hi" } });

  const bold = await screen.findByRole("button", { name: "Bold" });
  expect(bold).not.toHaveClass("active");

  await fireEvent.click(bold);

  expect(screen.getByRole("button", { name: "Bold" })).toHaveClass("active");
});

it("wraps the current paragraph when toggling a bullet list", async () => {
  render(RichTextEditor, { props: { initialText: "hi" } });

  await fireEvent.click(
    await screen.findByRole("button", { name: "Bullet list" }),
  );

  const box = screen.getByRole("textbox", { name: "Message body" });
  expect(box.querySelector("ul li")).not.toBeNull();
});

it("renders an initial blockquote as a real quote block", async () => {
  render(RichTextEditor, {
    props: {
      initialHtml: "<p>Vďaka!</p><blockquote><p>pôvodný</p></blockquote>",
    },
  });

  const box = await screen.findByRole("textbox", { name: "Message body" });
  expect(box.querySelector("blockquote p")).toHaveTextContent("pôvodný");
});

it("keeps inline data: images but drops remote ones", async () => {
  render(RichTextEditor, {
    props: {
      initialHtml:
        '<p>pics</p><img src="data:image/gif;base64,R0lGOD" alt="inline">' +
        '<img src="https://t.example/pixel.png" alt="tracker">',
    },
  });

  const box = await screen.findByRole("textbox", { name: "Message body" });
  expect(box.querySelector('img[src^="data:image/"]')).not.toBeNull();
  expect(box.querySelector('img[src^="https:"]')).toBeNull();
});

it("appendContent drops source whitespace instead of spawning empty lines", async () => {
  // A table-heavy newsletter fragment is full of whitespace runs between
  // tags — parsed as-is they exploded into hundreds of empty paragraphs.
  const { component } = render(RichTextEditor, {
    props: { initialText: "hi" },
  });
  const box = await screen.findByRole("textbox", { name: "Message body" });

  component.appendContent(
    "<blockquote>\n  \n  <p>quoted</p>\n  \n  <p>line</p>\n  \n</blockquote>",
  );

  await waitFor(() => expect(box.querySelector("blockquote")).not.toBeNull());
  expect(box.querySelectorAll("blockquote p")).toHaveLength(2);
});

it("disables the formatting controls while queueing", async () => {
  render(RichTextEditor, { props: { initialText: "", disabled: true } });

  const bold = await screen.findByRole("button", { name: "Bold" });
  expect(bold).toBeDisabled();
});
