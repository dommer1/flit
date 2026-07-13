import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import Outbox, { type OutboxEntry } from "./Outbox.svelte";

function entry(overrides: Partial<OutboxEntry> = {}): OutboxEntry {
  return {
    id: 1,
    subject: "Ahoj",
    status: "sending",
    error: null,
    undoMs: 8000,
    ...overrides,
  };
}

it("shows a sending badge whose undo reports the entry id", async () => {
  const onUndo = vi.fn();
  render(Outbox, { entries: [entry({ id: 7 })], onUndo });

  expect(screen.getByText("Sending: Ahoj")).toBeInTheDocument();

  await fireEvent.click(screen.getByRole("button", { name: "Undo" }));

  expect(onUndo).toHaveBeenCalledWith(7);
});

it("animates a countdown donut over the undo window", () => {
  const { container } = render(Outbox, {
    entries: [entry()],
    onUndo: vi.fn(),
  });

  const fill = container.querySelector(".donut .fill");
  expect(fill).not.toBeNull();
  expect(fill!.getAttribute("style")).toContain("animation-duration: 8000ms");
  expect(container.querySelector(".check")).toBeNull();
});

it("shows a sent badge with a check instead of the donut", () => {
  const { container } = render(Outbox, {
    entries: [entry({ status: "sent" })],
    onUndo: vi.fn(),
  });

  expect(screen.getByText("Sent: Ahoj")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Undo" })).not.toBeInTheDocument();
  expect(container.querySelector(".check")).not.toBeNull();
  expect(container.querySelector(".donut")).toBeNull();
});

it("shows the error on a failed badge", () => {
  render(Outbox, {
    entries: [
      entry({ status: "failed", error: "smtp error: relay refused" }),
    ],
    onUndo: vi.fn(),
  });

  expect(screen.getByText("Couldn't send: Ahoj")).toBeInTheDocument();
  expect(screen.getByText("smtp error: relay refused")).toBeInTheDocument();
});

it("falls back to a placeholder for empty subjects", () => {
  render(Outbox, { entries: [entry({ subject: "  " })], onUndo: vi.fn() });

  expect(screen.getByText("Sending: (No subject)")).toBeInTheDocument();
});

it("renders nothing without entries", () => {
  const { container } = render(Outbox, { entries: [], onUndo: vi.fn() });

  expect(container.querySelector(".outbox")).toBeNull();
});
