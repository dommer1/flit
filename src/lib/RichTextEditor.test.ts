import { expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
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

it("disables the formatting controls while queueing", async () => {
  render(RichTextEditor, { props: { initialText: "", disabled: true } });

  const bold = await screen.findByRole("button", { name: "Bold" });
  expect(bold).toBeDisabled();
});
