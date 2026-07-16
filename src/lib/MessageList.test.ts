import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import MessageList from "./MessageList.svelte";
import type { MessageHeader } from "./types";

function header(id: number, from: string, read: boolean): MessageHeader {
  return {
    id,
    accountId: 1,
    mailbox: "INBOX",
    from,
    to: "me@example.com",
    cc: "",
    replyTo: "",
    subject: `Subject ${id}`,
    snippet: "…",
    date: "2026-07-07T09:15:00Z",
    read,
  };
}

const messages = [
  header(1, "Alice <alice@example.com>", false),
  header(2, "Bob <bob@example.com>", true),
];

function renderList(props: Record<string, unknown> = {}) {
  return render(MessageList, {
    props: {
      title: "Inbox",
      messages,
      selectedId: null,
      onSelect: vi.fn(),
      ...props,
    },
  });
}

/** One trackpad pulse; natural scrolling makes fingers-left = +deltaX. */
function swipe(row: HTMLElement, deltaX: number, deltaY = 0) {
  return fireEvent.wheel(row, { deltaX, deltaY });
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

it("archives a row once a full swipe left settles", async () => {
  const onArchive = vi.fn();
  renderList({ onArchive });
  const row = screen.getByRole("option", { name: /Alice/ });

  await swipe(row, 60);
  await swipe(row, 60);
  expect(onArchive).not.toHaveBeenCalled(); // fingers still down

  vi.advanceTimersByTime(200);
  expect(onArchive).toHaveBeenCalledWith(1);
});

it("toggles read state once a full swipe right settles", async () => {
  const onSetRead = vi.fn();
  renderList({ onSetRead });

  // The unread row offers Mark Read…
  await swipe(screen.getByRole("option", { name: /Alice/ }), -120);
  vi.advanceTimersByTime(200);
  expect(onSetRead).toHaveBeenCalledWith(1, true);

  // …and the read row offers Mark Unread.
  await swipe(screen.getByRole("option", { name: /Bob/ }), -120);
  vi.advanceTimersByTime(200);
  expect(onSetRead).toHaveBeenCalledWith(2, false);
});

it("fires nothing when the swipe stops short of the trigger", async () => {
  const onArchive = vi.fn();
  const onSetRead = vi.fn();
  renderList({ onArchive, onSetRead });

  await swipe(screen.getByRole("option", { name: /Alice/ }), 30);
  vi.advanceTimersByTime(200);

  expect(onArchive).not.toHaveBeenCalled();
  expect(onSetRead).not.toHaveBeenCalled();
});

it("routes swipes through the configured actions", async () => {
  const onTrash = vi.fn();
  const onReply = vi.fn();
  renderList({
    swipeActions: { left: "trash", right: "reply" },
    onTrash,
    onReply,
  });
  const row = screen.getByRole("option", { name: /Alice/ });

  // Mid-gesture the strip announces the configured action, not Archive.
  await swipe(row, 60);
  expect(screen.getByText("Trash")).toBeInTheDocument();
  await swipe(row, 60);
  vi.advanceTimersByTime(200);
  expect(onTrash).toHaveBeenCalledWith(1);

  await swipe(row, -120);
  expect(screen.getByText("Reply")).toBeInTheDocument();
  vi.advanceTimersByTime(200);
  expect(onReply).toHaveBeenCalledWith(1);
});

it("neither moves nor fires on a direction configured to none", async () => {
  const onArchive = vi.fn();
  renderList({
    swipeActions: { left: "none", right: "toggleRead" },
    onArchive,
  });
  const row = screen.getByRole("option", { name: /Alice/ });

  await swipe(row, 120);
  // The row stays pinned — no strip is revealed for the dead side.
  expect(row.style.transform).toBe("");
  vi.advanceTimersByTime(200);

  expect(onArchive).not.toHaveBeenCalled();
});

it("leaves vertical scrolling alone", async () => {
  const onArchive = vi.fn();
  renderList({ onArchive });

  // deltaY dominates — this is a scroll, not a swipe, however far it goes.
  await swipe(screen.getByRole("option", { name: /Alice/ }), 100, 300);
  vi.advanceTimersByTime(200);

  expect(onArchive).not.toHaveBeenCalled();
});

it("labels the swipe backdrop Move to Inbox for archived rows", async () => {
  renderList({ onArchive: vi.fn(), isArchived: () => true });

  // Mid-swipe (short of the trigger) the backdrop is visible.
  await swipe(screen.getByRole("option", { name: /Alice/ }), 30);
  expect(screen.getByText("Move to Inbox")).toBeInTheDocument();

  vi.advanceTimersByTime(200); // snap back, nothing fired
});
