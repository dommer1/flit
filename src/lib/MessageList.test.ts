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
    messageId: "",
    references: "",
    threadCount: 1,
    threadUnread: false,
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

// why MouseEvent: jsdom has no PointerEvent, and fireEvent.pointerDown then
// synthesizes a bare Event without clientX/button. A MouseEvent with the
// pointer event type still reaches the handlers with real coordinates;
// bubbles is required — Svelte 5 delegates the handlers to the app root.
function pointer(type: string, init: MouseEventInit = {}) {
  return new MouseEvent(type, { bubbles: true, ...init });
}

/** Press, drag horizontally by `dx`, release — a mouse/touch swipe. */
async function drag(row: HTMLElement, dx: number) {
  await fireEvent(row, pointer("pointerdown", { clientX: 200, clientY: 50 }));
  // Two moves: past the slop first, then the rest — like a real pointer.
  await fireEvent(
    row,
    pointer("pointermove", { clientX: 200 + dx / 2, clientY: 50 }),
  );
  await fireEvent(
    row,
    pointer("pointermove", { clientX: 200 + dx, clientY: 50 }),
  );
  await fireEvent(row, pointer("pointerup", { clientX: 200 + dx, clientY: 50 }));
}

it("fires the configured action when a pointer drag releases past the trigger", async () => {
  const onArchive = vi.fn();
  const onSetRead = vi.fn();
  const onSelect = vi.fn();
  renderList({ onArchive, onSetRead, onSelect });
  const row = screen.getByRole("option", { name: /Alice/ });

  await drag(row, -80);
  expect(onArchive).toHaveBeenCalledWith(1);

  await drag(row, 80);
  expect(onSetRead).toHaveBeenCalledWith(1, true);

  // The click that follows a drag's release must not select the row.
  await fireEvent.click(row);
  expect(onSelect).not.toHaveBeenCalled();
});

it("snaps back without selecting when a drag stops short of the trigger", async () => {
  const onArchive = vi.fn();
  const onSelect = vi.fn();
  renderList({ onArchive, onSelect });
  const row = screen.getByRole("option", { name: /Alice/ });

  await drag(row, -30);
  await fireEvent.click(row);

  expect(onArchive).not.toHaveBeenCalled();
  expect(onSelect).not.toHaveBeenCalled();
  expect(row.style.transform).toBe("");
});

it("still selects on a plain click", async () => {
  const onSelect = vi.fn();
  renderList({ onSelect });
  const row = screen.getByRole("option", { name: /Alice/ });

  await fireEvent(row, pointer("pointerdown", { clientX: 200, clientY: 50 }));
  await fireEvent(row, pointer("pointerup", { clientX: 200, clientY: 50 }));
  await fireEvent.click(row);

  expect(onSelect).toHaveBeenCalledWith(1);
});

it("does not start a drag from a mostly vertical pointer move", async () => {
  const onArchive = vi.fn();
  const onSelect = vi.fn();
  renderList({ onArchive, onSelect });
  const row = screen.getByRole("option", { name: /Alice/ });

  await fireEvent(row, pointer("pointerdown", { clientX: 200, clientY: 50 }));
  await fireEvent(row, pointer("pointermove", { clientX: 120, clientY: 250 }));
  await fireEvent(row, pointer("pointerup", { clientX: 120, clientY: 250 }));
  await fireEvent.click(row);

  expect(onArchive).not.toHaveBeenCalled();
  // No drag happened, so the click still counts as a selection.
  expect(onSelect).toHaveBeenCalledWith(1);
});

it("labels the swipe backdrop Move to Inbox for archived rows", async () => {
  renderList({ onArchive: vi.fn(), isArchived: () => true });

  // Mid-swipe (short of the trigger) the backdrop is visible.
  await swipe(screen.getByRole("option", { name: /Alice/ }), 30);
  expect(screen.getByText("Move to Inbox")).toBeInTheDocument();

  vi.advanceTimersByTime(200); // snap back, nothing fired
});
