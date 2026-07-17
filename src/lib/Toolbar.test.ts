import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import Toolbar from "./Toolbar.svelte";
import type { Mailbox, MessageHeader } from "./types";

const message: MessageHeader = {
  id: 1,
  accountId: 1,
  mailbox: "INBOX",
  from: "Alice <alice@example.com>",
  to: "me@example.com",
  cc: "",
  replyTo: "",
  messageId: "",
  references: "",
  subject: "Weekend plans",
  snippet: "Hey",
  date: "2026-07-07T09:15:00Z",
  read: false,
};

const folders: Mailbox[] = [
  {
    id: 2,
    accountId: 1,
    name: "Work",
    role: null,
    displayName: "Work",
    unreadCount: 0,
  },
  {
    id: 3,
    accountId: 1,
    name: "K&APQBYQ-",
    role: "trash",
    displayName: "Kôš",
    unreadCount: 0,
  },
];

function renderToolbar(props: Record<string, unknown> = {}) {
  return render(Toolbar, {
    props: {
      sidebarCollapsed: false,
      sidebarWidth: 230,
      onToggleSidebar: vi.fn(),
      onRefresh: vi.fn(),
      onCompose: vi.fn(),
      onSearch: vi.fn(),
      onOpenSettings: vi.fn(),
      selected: null,
      onDraft: vi.fn(),
      onSetRead: vi.fn(),
      onArchive: vi.fn(),
      onTrash: vi.fn(),
      onMove: vi.fn(),
      ...props,
    },
  });
}

it("fires the chrome callbacks from their buttons", async () => {
  const onToggleSidebar = vi.fn();
  const onRefresh = vi.fn();
  const onCompose = vi.fn();
  const onOpenSettings = vi.fn();
  renderToolbar({ onToggleSidebar, onRefresh, onCompose, onOpenSettings });

  await fireEvent.click(screen.getByRole("button", { name: "Toggle sidebar" }));
  expect(onToggleSidebar).toHaveBeenCalledOnce();

  await fireEvent.click(
    screen.getByRole("button", { name: "Check for new mail" }),
  );
  expect(onRefresh).toHaveBeenCalledOnce();

  await fireEvent.click(screen.getByRole("button", { name: "New Message" }));
  expect(onCompose).toHaveBeenCalledOnce();

  await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(onOpenSettings).toHaveBeenCalledOnce();
});

it("reports what is typed into the search field", async () => {
  const onSearch = vi.fn();
  renderToolbar({ onSearch });

  const input = screen.getByRole("searchbox", { name: "Search messages" });
  await fireEvent.input(input, { target: { value: "from:alice" } });

  expect(onSearch).toHaveBeenCalledWith("from:alice");
});

it("sizes the traffic-light zone to the sidebar and shrinks it collapsed", async () => {
  const { container, rerender } = renderToolbar({ sidebarWidth: 260 });

  const zone = container.querySelector<HTMLElement>(".sidebar-zone");
  expect(zone?.style.width).toBe("260px");

  await rerender({ sidebarCollapsed: true, sidebarWidth: 260 });
  expect(zone?.style.width).toBe("130px");
});

it("disables the message actions while nothing is selected", () => {
  renderToolbar({ selected: null });

  for (const name of ["Reply", "Reply All", "Forward", "Mark Unread", "Archive", "Move to", "Trash"]) {
    expect(screen.getByRole("button", { name })).toBeDisabled();
  }
});

it("reports the draft kind for reply, reply all and forward", async () => {
  const onDraft = vi.fn();
  renderToolbar({ selected: message, onDraft });

  await fireEvent.click(screen.getByRole("button", { name: "Reply" }));
  expect(onDraft).toHaveBeenCalledWith("reply");

  await fireEvent.click(screen.getByRole("button", { name: "Reply All" }));
  expect(onDraft).toHaveBeenCalledWith("reply-all");

  await fireEvent.click(screen.getByRole("button", { name: "Forward" }));
  expect(onDraft).toHaveBeenCalledWith("forward");
});

it("toggles read state via the Mark Read / Mark Unread button", async () => {
  const onSetRead = vi.fn();
  const { rerender } = renderToolbar({ selected: message, onSetRead });

  // The fixture is unread, so the button offers to mark it read.
  await fireEvent.click(screen.getByRole("button", { name: "Mark Read" }));
  expect(onSetRead).toHaveBeenCalledWith(1, true);

  await rerender({ selected: { ...message, read: true } });
  await fireEvent.click(screen.getByRole("button", { name: "Mark Unread" }));
  expect(onSetRead).toHaveBeenCalledWith(1, false);
});

it("archives and trashes the selected message", async () => {
  const onArchive = vi.fn();
  const onTrash = vi.fn();
  renderToolbar({ selected: message, onArchive, onTrash });

  await fireEvent.click(screen.getByRole("button", { name: "Archive" }));
  expect(onArchive).toHaveBeenCalledWith(1);

  await fireEvent.click(screen.getByRole("button", { name: "Trash" }));
  expect(onTrash).toHaveBeenCalledWith(1);
});

it("relabels the archive action on an already-archived message", async () => {
  const onArchive = vi.fn();
  renderToolbar({ selected: message, archived: true, onArchive });

  expect(
    screen.queryByRole("button", { name: "Archive" }),
  ).not.toBeInTheDocument();
  await fireEvent.click(screen.getByRole("button", { name: "Move to Inbox" }));
  expect(onArchive).toHaveBeenCalledWith(1);
});

it("moves the message via the Move to menu using the wire name", async () => {
  const onMove = vi.fn();
  renderToolbar({ selected: message, moveTargets: folders, onMove });

  await fireEvent.click(screen.getByRole("button", { name: "Move to" }));

  expect(screen.getByRole("menuitem", { name: "Work" })).toBeInTheDocument();

  // The move reports the folder's IMAP wire name, not its display name.
  await fireEvent.click(screen.getByRole("menuitem", { name: "Kôš" }));
  expect(onMove).toHaveBeenCalledWith(1, "K&APQBYQ-");
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

it("disables Move to when there is nowhere to move", () => {
  renderToolbar({ selected: message, moveTargets: [] });

  expect(screen.getByRole("button", { name: "Move to" })).toBeDisabled();
});
