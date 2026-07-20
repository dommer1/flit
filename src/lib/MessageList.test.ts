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
    bcc: "",
    messageId: "",
    references: "",
    threadCount: 1,
    threadUnread: false,
    isDraft: false,
    subject: `Subject ${id}`,
    snippet: "…",
    date: "2026-07-07T09:15:00Z",
    read,
    hasAttachments: false,
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

it("shows a paperclip on rows whose conversation has attachments", () => {
  renderList({
    messages: [
      { ...header(1, "Alice <alice@example.com>", true), hasAttachments: true },
      header(2, "Bob <bob@example.com>", true),
    ],
  });

  const withFile = screen.getByRole("option", { name: /Alice/ });
  expect(withFile.querySelector(".clip")).not.toBeNull();
  const without = screen.getByRole("option", { name: /Bob/ });
  expect(without.querySelector(".clip")).toBeNull();
});

it("dots a read representative whose conversation is unread elsewhere", () => {
  renderList({
    messages: [
      {
        ...header(1, "Alice <alice@example.com>", true),
        threadCount: 2,
        threadUnread: true,
      },
    ],
  });

  const row = screen.getByRole("option", { name: /Alice/ });
  expect(row.querySelector(".dot")).not.toBeNull();
});

// Pull-to-refresh: upward wheel pulses on the list itself while at the top.

it("starts a refresh once a deep pull at the top settles", async () => {
  const onRefresh = vi.fn();
  renderList({ onRefresh });
  const list = screen.getByRole("listbox");

  await swipe(list, 0, -80);
  await swipe(list, 0, -80);
  expect(onRefresh).not.toHaveBeenCalled(); // fingers still down

  vi.advanceTimersByTime(200);
  expect(onRefresh).toHaveBeenCalledOnce();
});

it("arms the hint only past the trigger line", async () => {
  renderList({ onRefresh: vi.fn() });
  const list = screen.getByRole("listbox");

  expect(screen.queryByText(/refresh/)).toBeNull();
  await swipe(list, 0, -80); // 40px of pull — short of the trigger
  expect(screen.getByText("Pull to refresh")).toBeInTheDocument();
  await swipe(list, 0, -80);
  expect(screen.getByText("Release to refresh")).toBeInTheDocument();
  vi.advanceTimersByTime(200); // release, don't leak the timer
});

it("fires nothing when the pull stops short of the trigger", async () => {
  const onRefresh = vi.fn();
  renderList({ onRefresh });

  await swipe(screen.getByRole("listbox"), 0, -80);
  vi.advanceTimersByTime(200);

  expect(onRefresh).not.toHaveBeenCalled();
});

it("ignores upward wheels while the list is scrolled down", async () => {
  const onRefresh = vi.fn();
  renderList({ onRefresh });
  const list = screen.getByRole("listbox");
  list.scrollTop = 100;

  await swipe(list, 0, -300);
  vi.advanceTimersByTime(200);

  expect(onRefresh).not.toHaveBeenCalled();
});

it("shows a checking indicator and swallows pulls while refreshing", async () => {
  const onRefresh = vi.fn();
  renderList({ onRefresh, refreshing: true });

  expect(screen.getByText(/Checking for new mail/)).toBeInTheDocument();

  await swipe(screen.getByRole("listbox"), 0, -300);
  vi.advanceTimersByTime(200);
  expect(onRefresh).not.toHaveBeenCalled();
});

it("leaves horizontal row swipes out of the pull", async () => {
  const onRefresh = vi.fn();
  renderList({ onRefresh, onArchive: vi.fn() });

  // A row swipe bubbles to the list — dominant deltaX must not pull.
  await swipe(screen.getByRole("option", { name: /Alice/ }), 120, -20);
  vi.advanceTimersByTime(200);

  expect(onRefresh).not.toHaveBeenCalled();
});

/** A view status: 500 of 10000 messages cached, 800 rows listable. */
function status(overrides: Record<string, unknown> = {}) {
  return {
    listRows: 800,
    unread: 56,
    cached: 500,
    serverTotal: 10000,
    ...overrides,
  };
}

/** Pin the scroll geometry jsdom doesn't compute, then fire a scroll. */
function scrollTo(list: HTMLElement, top: number, height = 2000, view = 400) {
  Object.defineProperty(list, "scrollHeight", {
    value: height,
    configurable: true,
  });
  Object.defineProperty(list, "clientHeight", {
    value: view,
    configurable: true,
  });
  list.scrollTop = top;
  return fireEvent.scroll(list);
}

it("asks for more rows when scrolled near the bottom", async () => {
  const onLoadMore = vi.fn();
  renderList({ status: status(), onLoadMore });

  await scrollTo(screen.getByRole("listbox"), 1500);

  expect(onLoadMore).toHaveBeenCalledOnce();
});

it("asks for nothing far from the bottom", async () => {
  const onLoadMore = vi.fn();
  renderList({ status: status(), onLoadMore });

  await scrollTo(screen.getByRole("listbox"), 0);

  expect(onLoadMore).not.toHaveBeenCalled();
});

it("asks for nothing once every row is listed", async () => {
  const onLoadMore = vi.fn();
  renderList({ status: status({ listRows: messages.length }), onLoadMore });

  await scrollTo(screen.getByRole("listbox"), 1500);

  expect(onLoadMore).not.toHaveBeenCalled();
});

it("headers the view's totals, not the revealed slice", () => {
  renderList({ status: status() });

  const count = document.querySelector(".count");
  expect(count?.textContent).toMatch(/800\s+messages/);
  expect(count?.textContent).toContain("56 unread");
});

it("shows backfill progress while the cache trails the server", () => {
  renderList({ status: status() });

  expect(screen.getByText(/Syncing older messages/)).toBeTruthy();
});

it("hides the progress line once the mailbox is fully mirrored", () => {
  renderList({ status: status({ cached: 10000 }) });

  expect(screen.queryByText(/Syncing older messages/)).toBeNull();
});

it("hides the progress line while the server total is unknown", () => {
  renderList({ status: status({ serverTotal: null }) });

  expect(screen.queryByText(/Syncing older messages/)).toBeNull();
});

it("renders one date section header where the section changes", () => {
  const today = new Date().toISOString();
  renderList({
    messages: [
      { ...header(1, "Alice <alice@example.com>", false), date: today },
      { ...header(2, "Bob <bob@example.com>", true), date: today },
      { ...header(3, "Carol <carol@example.com>", true), date: "2025-03-10T10:00:00Z" },
    ],
  });

  const sections = [...document.querySelectorAll(".section")].map((el) =>
    el.textContent?.trim(),
  );
  expect(sections).toEqual(["Today", "2025"]);
});

it("renders no section headers for an empty list", () => {
  renderList({ messages: [] });

  expect(document.querySelector(".section")).toBeNull();
});
