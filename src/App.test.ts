import { beforeEach, expect, it, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/svelte";
import type {
  Account,
  MessageHeader,
  ScheduledMessage,
  SendEvent,
  SwipeActions,
} from "./lib/types";

const accounts: Account[] = [
  {
    id: 1,
    name: "Personal",
    email: "domco@example.com",
    imapHost: "imap.example.com",
    imapPort: 993,
    smtpHost: "smtp.example.com",
    smtpPort: 587,
    username: "domco@example.com",
    lastError: null,
    checkedAt: 1751900000,
    color: null,
    signatureId: null,
    notifyEnabled: null,
    notifySound: null,
    defaultAliasId: null,
  },
  {
    id: 2,
    name: "Work",
    email: "hello@vocalio.sk",
    imapHost: "imap.vocalio.sk",
    imapPort: 993,
    smtpHost: "smtp.vocalio.sk",
    smtpPort: 587,
    username: "hello@vocalio.sk",
    lastError: null,
    checkedAt: 1751900000,
    color: null,
    signatureId: null,
    notifyEnabled: null,
    notifySound: null,
    defaultAliasId: null,
  },
];

const allMessages: MessageHeader[] = [
  {
    id: 1,
    accountId: 1,
    mailbox: "INBOX",
    from: "Alice <alice@example.com>",
    to: "domco@example.com, Bob <bob@example.com>",
    cc: "carol@example.com",
    replyTo: "",
    messageId: "",
    references: "",
    threadCount: 1,
    threadUnread: false,
    subject: "Weekend plans",
    snippet: "Are we still on for Saturday?",
    date: "2026-07-07T09:15:00Z",
    read: false,
  },
  {
    id: 2,
    accountId: 2,
    mailbox: "INBOX",
    from: "Peter",
    to: "hello@vocalio.sk",
    cc: "",
    replyTo: "",
    messageId: "",
    references: "",
    threadCount: 1,
    threadUnread: false,
    subject: "Re: Invoice",
    snippet: "Payment went out this morning.",
    date: "2026-07-08T07:45:00Z",
    read: true,
  },
];

// Lives in Personal's Archive folder — only listed when that folder is open.
const archivedMessages: MessageHeader[] = [
  {
    id: 9,
    accountId: 1,
    mailbox: "Archive",
    from: "Old Friend <old@example.com>",
    to: "hello@vocalio.sk",
    cc: "",
    replyTo: "",
    messageId: "",
    references: "",
    threadCount: 1,
    threadUnread: false,
    subject: "Archived note",
    snippet: "Filed away long ago.",
    date: "2026-06-01T00:00:00Z",
    read: true,
  },
];

// why: mutable + captured by the mock factory, so tests can simulate the
// backend changing state and firing change events.
let currentAccounts: Account[] = [];
let currentMessages: MessageHeader[] = [];
let currentScheduled: ScheduledMessage[] = [];
let accountsChanged: (() => void) | undefined;
let messagesChanged: (() => void) | undefined;
let sendQueued: ((e: SendEvent) => void) | undefined;
let sendFinished: ((e: SendEvent) => void) | undefined;
let sendUndone: ((e: SendEvent) => void) | undefined;
let scheduledMissed: (() => void) | undefined;
let scheduledChanged: (() => void) | undefined;
let settingsChanged: (() => void) | undefined;
let currentSwipeActions: SwipeActions;

// why a named default: tests override listMailboxes with
// mockImplementation, which clearAllMocks does NOT undo — beforeEach
// reinstalls this so an override can never leak into later tests.
async function defaultListMailboxes(accountId: number) {
  return accountId === 1
    ? [
        {
          id: 1,
          accountId: 1,
          name: "INBOX",
          role: "inbox",
          displayName: "INBOX",
          unreadCount: 0,
        },
        {
          id: 2,
          accountId: 1,
          name: "Archive",
          role: "archive",
          displayName: "Archive",
          unreadCount: 0,
        },
        // A Gmail-style folder: the wire name stays modified UTF-7, the
        // backend ships the decoded label alongside it.
        {
          id: 3,
          accountId: 1,
          name: "[Gmail]/Odoslan&AOk-",
          role: "sent",
          displayName: "Odoslané",
          unreadCount: 0,
        },
      ]
    : [];
}

vi.mock("./lib/api", () => ({
  listAccounts: vi.fn(async () => currentAccounts),
  listMailboxes: vi.fn(defaultListMailboxes),
  listMessages: vi.fn(async (accountId: number | null, mailbox = "INBOX") => {
    const pool = mailbox === "Archive" ? archivedMessages : currentMessages;
    return accountId === null
      ? pool
      : pool.filter((m) => m.accountId === accountId);
  }),
  // why: a canned single-hit result — App tests only assert the wiring
  // (what was called with what); real matching is covered by Rust tests.
  searchMessages: vi.fn(async () => [currentMessages[1]]),
  // The view's conversation fetch: empty = fall back to the selected row.
  listThread: vi.fn(async () => []),
  // Bulk bodies for the conversation view, keyed by message id — serve the
  // canned body under the anchor so the view pane shows it.
  threadBodies: vi.fn(async (messageId: number) => ({
    [messageId]: {
      html: null,
      text: "body text",
      blockedImages: 0,
      canLoadRemote: false,
      attachments: [],
    },
  })),
  getMessageBody: vi.fn(async () => ({
    html: null,
    text: "body text",
    blockedImages: 0,
    canLoadRemote: false,
    attachments: [],
  })),
  syncAccount: vi.fn(async () => undefined),
  setMessageRead: vi.fn(async () => undefined),
  moveToTrash: vi.fn(async () => undefined),
  archiveMessage: vi.fn(async () => undefined),
  moveMessage: vi.fn(async () => undefined),
  trashThread: vi.fn(async () => undefined),
  archiveThread: vi.fn(async () => undefined),
  moveThread: vi.fn(async () => undefined),
  openCompose: vi.fn(async () => undefined),
  openDraft: vi.fn(async () => undefined),
  openSettings: vi.fn(async () => undefined),
  onAccountsChanged: vi.fn(async (callback: () => void) => {
    accountsChanged = callback;
    return () => {};
  }),
  onMessagesChanged: vi.fn(async (callback: () => void) => {
    messagesChanged = callback;
    return () => {};
  }),
  undoSend: vi.fn(async () => undefined),
  onSendQueued: vi.fn(async (callback: (e: SendEvent) => void) => {
    sendQueued = callback;
    return () => {};
  }),
  onSendFinished: vi.fn(async (callback: (e: SendEvent) => void) => {
    sendFinished = callback;
    return () => {};
  }),
  onSendUndone: vi.fn(async (callback: (e: SendEvent) => void) => {
    sendUndone = callback;
    return () => {};
  }),
  getSwipeActions: vi.fn(async () => currentSwipeActions),
  onSettingsChanged: vi.fn(async (callback: () => void) => {
    settingsChanged = callback;
    return () => {};
  }),
  listScheduled: vi.fn(async () => currentScheduled),
  sendScheduledNow: vi.fn(async () => undefined),
  cancelScheduled: vi.fn(async () => undefined),
  onScheduledMissed: vi.fn(async (callback: () => void) => {
    scheduledMissed = callback;
    return () => {};
  }),
  onScheduledChanged: vi.fn(async (callback: () => void) => {
    scheduledChanged = callback;
    return () => {};
  }),
}));

import * as api from "./lib/api";
import { PANE_WIDTHS_KEY } from "./lib/paneSizes";
import App from "./App.svelte";

beforeEach(() => {
  currentAccounts = [...accounts];
  currentMessages = [...allMessages];
  currentScheduled = [];
  accountsChanged = undefined;
  messagesChanged = undefined;
  sendQueued = undefined;
  sendFinished = undefined;
  sendUndone = undefined;
  scheduledMissed = undefined;
  scheduledChanged = undefined;
  settingsChanged = undefined;
  currentSwipeActions = { left: "archive", right: "toggleRead" };
  localStorage.clear();
  vi.clearAllMocks();
  vi.mocked(api.listMailboxes).mockImplementation(defaultListMailboxes);
});

it("loads accounts and the unified inbox on start", async () => {
  render(App);

  expect(
    await screen.findByRole("button", { name: "All Inboxes" }),
  ).toBeInTheDocument();
  expect(screen.getByText("Personal")).toBeInTheDocument();
  expect(screen.getByText("Work")).toBeInTheDocument();
  expect(await screen.findByText("Weekend plans")).toBeInTheDocument();
  expect(screen.getByText("Re: Invoice")).toBeInTheDocument();
  // the list pane header names the current mailbox and counts its messages,
  // calling out how many are still unread
  expect(
    screen.getByRole("heading", { name: "All Inboxes" }),
  ).toBeInTheDocument();
  expect(screen.getByText("2 messages, 1 unread")).toBeInTheDocument();
});

it("shows the selected message in the view pane", async () => {
  render(App);

  expect(screen.getByText("Select a message")).toBeInTheDocument();

  await fireEvent.click(await screen.findByText("Weekend plans"));

  expect(
    await screen.findByRole("heading", { name: "Weekend plans" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("Select a message")).not.toBeInTheDocument();
});

it("filters the list when an account is selected", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  await fireEvent.click(screen.getByText("Work"));

  expect(await screen.findByText("Re: Invoice")).toBeInTheDocument();
  expect(screen.queryByText("Weekend plans")).not.toBeInTheDocument();
});

it("expands an account and opens one of its folders", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  await fireEvent.click(
    screen.getByRole("button", { name: "Toggle folders for Personal" }),
  );
  // why within(nav): the toolbar has an Archive action button too — the
  // sidebar folder is the one this test opens.
  const sidebar = within(screen.getByRole("navigation"));
  await fireEvent.click(await sidebar.findByRole("button", { name: "Archive" }));

  expect(await screen.findByText("Archived note")).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Archive" })).toBeInTheDocument();
  expect(screen.queryByText("Weekend plans")).not.toBeInTheDocument();
});

it("shows decoded folder names but talks to the backend in wire names", async () => {
  const { listMessages } = await import("./lib/api");
  render(App);
  await screen.findByText("Weekend plans");

  await fireEvent.click(
    screen.getByRole("button", { name: "Toggle folders for Personal" }),
  );
  // The sidebar and list header show the decoded label…
  await fireEvent.click(await screen.findByRole("button", { name: "Odoslané" }));
  expect(
    await screen.findByRole("heading", { name: "Odoslané" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("[Gmail]/Odoslan&AOk-")).not.toBeInTheDocument();

  // …while the IMAP wire name is what the backend receives.
  expect(listMessages).toHaveBeenLastCalledWith(1, "[Gmail]/Odoslan&AOk-");
});

it("hides the folder toggle for accounts with no extra folders", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  expect(
    screen.getByRole("button", { name: "Toggle folders for Personal" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Toggle folders for Work" }),
  ).not.toBeInTheDocument();
});

it("clears the selected message when switching accounts", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });

  await fireEvent.click(screen.getByText("Work"));

  expect(await screen.findByText("Select a message")).toBeInTheDocument();
});

it("opens the settings window from the toolbar gear", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  await fireEvent.click(screen.getByRole("button", { name: "Settings" }));

  await waitFor(() => expect(api.openSettings).toHaveBeenCalled());
});

it("a new message starts from the account's default send-as identity", async () => {
  currentAccounts = [{ ...accounts[0], defaultAliasId: 5 }, accounts[1]];
  render(App);
  await screen.findByText("Weekend plans");

  await fireEvent.click(screen.getByRole("button", { name: "New Message" }));

  await waitFor(() =>
    expect(api.openCompose).toHaveBeenCalledWith(
      expect.objectContaining({ accountId: 1, aliasId: 5 }),
    ),
  );
});

it("opens a compose window for a new message", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  await fireEvent.click(screen.getByRole("button", { name: "New Message" }));

  // the unified inbox has no account selected — From falls back to the first
  await waitFor(() =>
    expect(api.openCompose).toHaveBeenCalledWith({
      accountId: 1,
      to: "",
      subject: "",
      body: "",
    }),
  );
});

it("opens a reply draft from the account the message arrived on", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });
  await screen.findByText("body text");

  await fireEvent.click(screen.getByRole("button", { name: "Reply" }));

  await waitFor(() =>
    expect(api.openCompose).toHaveBeenCalledWith({
      accountId: 1,
      to: "alice@example.com",
      subject: "Re: Weekend plans",
      body: expect.stringContaining("> body text"),
    }),
  );
});

it("opens a reply-all draft without the receiving account's address", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });
  await screen.findByText("body text");

  await fireEvent.click(screen.getByRole("button", { name: "Reply All" }));

  // Message 1 arrived on account 1 (domco@example.com) — that address
  // must vanish while Bob and the Cc line survive.
  await waitFor(() =>
    expect(api.openCompose).toHaveBeenCalledWith({
      accountId: 1,
      to: "Alice <alice@example.com>, Bob <bob@example.com>",
      cc: "carol@example.com",
      subject: "Re: Weekend plans",
      body: expect.stringContaining("> body text"),
    }),
  );
});

it("opens a forward draft with the original below a header block", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });
  await screen.findByText("body text");

  await fireEvent.click(screen.getByRole("button", { name: "Forward" }));

  await waitFor(() =>
    expect(api.openCompose).toHaveBeenCalledWith({
      accountId: 1,
      to: "",
      subject: "Fwd: Weekend plans",
      body: expect.stringContaining("---------- Forwarded message ----------"),
    }),
  );
});

it("starts a sync for every account on launch", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  await waitFor(() => {
    expect(api.syncAccount).toHaveBeenCalledWith(1);
    expect(api.syncAccount).toHaveBeenCalledWith(2);
  });
});

it("syncs accounts in parallel, not one after another", async () => {
  const started: number[] = [];
  let releaseSyncs!: () => void;
  const gate = new Promise<void>((resolve) => (releaseSyncs = resolve));
  // why mockImplementationOnce: consumed by this test's two calls, so later
  // tests fall back to the factory's instantly-resolving sync.
  const gatedSync = async (id: number) => {
    started.push(id);
    await gate;
  };
  vi.mocked(api.syncAccount)
    .mockImplementationOnce(gatedSync)
    .mockImplementationOnce(gatedSync);

  render(App);

  // A sequential loop would await account 1's sync (blocked on the gate)
  // before ever starting account 2's.
  await waitFor(() => expect(started).toEqual([1, 2]));
  releaseSyncs();
});

it("refreshes the list on messages-changed and keeps the selection", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });

  currentMessages = [
    ...allMessages,
    {
      id: 3,
      accountId: 1,
      mailbox: "INBOX",
      from: "New Sender",
      to: "hello@vocalio.sk",
      cc: "",
      replyTo: "",
      messageId: "",
      references: "",
      threadCount: 1,
      threadUnread: false,
      subject: "Brand new",
      snippet: "",
      date: "2026-07-09T00:00:00Z",
      read: false,
    },
  ];
  messagesChanged?.();

  expect(await screen.findByText("Brand new")).toBeInTheDocument();
  expect(
    screen.getByRole("heading", { name: "Weekend plans" }),
  ).toBeInTheDocument();
});

// why fake timers mid-test: the initial load must run on real timers
// (findBy* polls with them), only the debounce window itself is faked.
async function typeIntoSearch(value: string) {
  const input = screen.getByRole("searchbox", { name: "Search messages" });
  vi.useFakeTimers();
  await fireEvent.input(input, { target: { value } });
  await vi.advanceTimersByTimeAsync(250);
  vi.useRealTimers();
}

it("runs one debounced search and shows its results", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  const input = screen.getByRole("searchbox", { name: "Search messages" });
  vi.useFakeTimers();
  await fireEvent.input(input, { target: { value: "from:pet" } });
  await fireEvent.input(input, { target: { value: "from:peter" } });
  await vi.advanceTimersByTimeAsync(250);
  vi.useRealTimers();

  expect(api.searchMessages).toHaveBeenCalledTimes(1);
  expect(api.searchMessages).toHaveBeenCalledWith(null, "from:peter");
  expect(await screen.findByText("Re: Invoice")).toBeInTheDocument();
  expect(screen.queryByText("Weekend plans")).not.toBeInTheDocument();
});

it("clearing the search restores the plain list", async () => {
  render(App);
  await screen.findByText("Weekend plans");
  await typeIntoSearch("invoice");
  await screen.findByText("Re: Invoice");

  await typeIntoSearch("");

  expect(await screen.findByText("Weekend plans")).toBeInTheDocument();
  // an empty query never hits the search backend
  expect(api.searchMessages).toHaveBeenCalledTimes(1);
});

it("scopes the search to the selected account", async () => {
  render(App);
  await screen.findByText("Weekend plans");
  await fireEvent.click(screen.getByText("Work"));
  await screen.findByText("Re: Invoice");

  await typeIntoSearch("faktura");

  expect(api.searchMessages).toHaveBeenCalledWith(2, "faktura");
});

it("refreshes accounts when another window changes them", async () => {
  render(App);
  await screen.findByText("Work");

  currentAccounts = [
    ...accounts,
    { ...accounts[0], id: 3, name: "Third", email: "third@example.com" },
  ];
  accountsChanged?.();

  expect(await screen.findByText("Third")).toBeInTheDocument();
});

it("syncs a newly added account immediately", async () => {
  render(App);
  await screen.findByText("Work");

  currentAccounts = [
    ...accounts,
    { ...accounts[0], id: 3, name: "Third", email: "third@example.com" },
  ];
  accountsChanged?.();
  await screen.findByText("Third");

  await waitFor(() => expect(api.syncAccount).toHaveBeenCalledWith(3));
});

// why MouseEvent: jsdom has no PointerEvent constructor; a MouseEvent with a
// pointer event type still reaches the pointerdown/... listeners with clientX.
// bubbles is required — Svelte 5 delegates onpointerdown to the app root.
async function dragSeparator(
  separator: HTMLElement,
  fromX: number,
  toX: number,
) {
  await fireEvent(
    separator,
    new MouseEvent("pointerdown", { clientX: fromX, bubbles: true }),
  );
  await fireEvent(
    separator,
    new MouseEvent("pointermove", { clientX: toX, bubbles: true }),
  );
  await fireEvent(separator, new MouseEvent("pointerup", { bubbles: true }));
}

it("resizes the sidebar by dragging its divider and persists the width", async () => {
  render(App);
  const separator = await screen.findByRole("separator", {
    name: "Resize sidebar",
  });

  await dragSeparator(separator, 208, 258);

  expect(separator).toHaveAttribute("aria-valuenow", "258");
  expect(JSON.parse(localStorage.getItem(PANE_WIDTHS_KEY)!)).toEqual({
    sidebar: 258,
    list: 352,
  });
});

it("clamps a drag past the pane's minimum width", async () => {
  render(App);
  const separator = await screen.findByRole("separator", {
    name: "Resize message list",
  });

  await dragSeparator(separator, 560, 0);

  expect(separator).toHaveAttribute("aria-valuenow", "240");
});

it("restores saved pane widths on start", async () => {
  localStorage.setItem(
    PANE_WIDTHS_KEY,
    JSON.stringify({ sidebar: 300, list: 400 }),
  );
  render(App);

  const separator = await screen.findByRole("separator", {
    name: "Resize sidebar",
  });
  expect(separator).toHaveAttribute("aria-valuenow", "300");
});

it("resizes a pane with arrow keys on the focused divider", async () => {
  render(App);
  const separator = await screen.findByRole("separator", {
    name: "Resize sidebar",
  });

  await fireEvent.keyDown(separator, { key: "ArrowRight" });
  await fireEvent.keyDown(separator, { key: "ArrowRight" });
  await fireEvent.keyDown(separator, { key: "ArrowLeft" });

  expect(separator).toHaveAttribute("aria-valuenow", "224");
  expect(
    JSON.parse(localStorage.getItem(PANE_WIDTHS_KEY)!).sidebar,
  ).toBe(224);
});

it("falls back to the unified inbox when the selected account disappears", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Work"));
  await screen.findByText("Re: Invoice");

  currentAccounts = [accounts[0]];
  accountsChanged?.();

  expect(await screen.findByText("Weekend plans")).toBeInTheDocument();
  expect(screen.queryByText("Work")).not.toBeInTheDocument();
});

it("shows a sending badge whose undo hands the message back", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  sendQueued?.({ id: 4, subject: "Ahoj", error: null, undoMs: 8000 });
  expect(await screen.findByText("Sending: Ahoj")).toBeInTheDocument();

  await fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  expect(api.undoSend).toHaveBeenCalledWith(4);

  // the badge leaves on the backend's confirmation, not on the click
  sendUndone?.({ id: 4, subject: "Ahoj", error: null, undoMs: 0 });
  await waitFor(() =>
    expect(screen.queryByText("Sending: Ahoj")).not.toBeInTheDocument(),
  );
});

it("confirms a delivered send and hides the badge on its own", async () => {
  render(App);
  await screen.findByText("Weekend plans");
  sendQueued?.({ id: 5, subject: "Ahoj", error: null, undoMs: 8000 });
  await screen.findByText("Sending: Ahoj");

  vi.useFakeTimers();
  sendFinished?.({ id: 5, subject: "Ahoj", error: null, undoMs: 0 });
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByText("Sent: Ahoj")).toBeInTheDocument();

  await vi.advanceTimersByTimeAsync(3000);
  vi.useRealTimers();

  expect(screen.queryByText("Sent: Ahoj")).not.toBeInTheDocument();
});

it("shows a failure badge with the delivery error", async () => {
  render(App);
  await screen.findByText("Weekend plans");
  sendQueued?.({ id: 6, subject: "Ahoj", error: null, undoMs: 8000 });
  await screen.findByText("Sending: Ahoj");

  vi.useFakeTimers();
  sendFinished?.({ id: 6, subject: "Ahoj", error: "smtp error: relay refused", undoMs: 0 });
  await vi.advanceTimersByTimeAsync(0);
  vi.useRealTimers();

  expect(screen.getByText("Couldn't send: Ahoj")).toBeInTheDocument();
  expect(screen.getByText("smtp error: relay refused")).toBeInTheDocument();
});

it("navigates the message list with Arrow Down / Up", async () => {
  render(App);
  const first = (await screen.findByText("Weekend plans")).closest(
    '[role="option"]',
  );
  const second = screen.getByText("Re: Invoice").closest('[role="option"]');
  expect(first).toHaveAttribute("aria-selected", "false");

  await fireEvent.keyDown(document.body, { key: "ArrowDown" });
  expect(first).toHaveAttribute("aria-selected", "true");

  await fireEvent.keyDown(document.body, { key: "ArrowDown" });
  expect(second).toHaveAttribute("aria-selected", "true");
  expect(first).toHaveAttribute("aria-selected", "false");

  await fireEvent.keyDown(document.body, { key: "ArrowUp" });
  expect(first).toHaveAttribute("aria-selected", "true");
});

it("marks an unread message read when it is opened", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  expect(api.setMessageRead).toHaveBeenCalledWith(1, true);
});

it("does not touch the read state of an already-read message", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Re: Invoice"));
  expect(api.setMessageRead).not.toHaveBeenCalled();
});

it("marks message rows of colored accounts with a dot", async () => {
  currentAccounts = [{ ...accounts[0], color: "#ff9f0a" }, accounts[1]];
  const { container } = render(App);
  await screen.findByText("Weekend plans");

  // Only account 1 has a color, so only its message shows a dot.
  const dots = container.querySelectorAll<HTMLElement>(".account-dot");
  expect(dots).toHaveLength(1);
  expect(dots[0].style.background).not.toBe("");
});

it("collapses and restores the sidebar with the toggle button", async () => {
  render(App);
  expect(
    await screen.findByRole("button", { name: "All Inboxes" }),
  ).toBeInTheDocument();

  await fireEvent.click(screen.getByRole("button", { name: "Toggle sidebar" }));
  expect(
    screen.queryByRole("button", { name: "All Inboxes" }),
  ).not.toBeInTheDocument();

  await fireEvent.click(screen.getByRole("button", { name: "Toggle sidebar" }));
  expect(
    screen.getByRole("button", { name: "All Inboxes" }),
  ).toBeInTheDocument();
});

it("trashes the open message and steps to its neighbour", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });

  await fireEvent.click(screen.getByRole("button", { name: "Trash" }));

  expect(api.moveToTrash).toHaveBeenCalledWith(1);
  // The row leaves the list at once (optimistic), selection steps to the next.
  expect(screen.queryByText("Weekend plans")).not.toBeInTheDocument();
  await screen.findByRole("heading", { name: "Re: Invoice" });
});

it("routes a thread row's trash and archive to the whole conversation", async () => {
  currentMessages = [
    { ...allMessages[0], threadCount: 2, threadUnread: false },
    allMessages[1],
  ];
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });

  await fireEvent.click(screen.getByRole("button", { name: "Trash" }));

  expect(api.trashThread).toHaveBeenCalledWith(1);
  expect(api.moveToTrash).not.toHaveBeenCalled();
});

it("archives the open message and steps to its neighbour", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });

  await fireEvent.click(screen.getByRole("button", { name: "Archive" }));

  expect(api.archiveMessage).toHaveBeenCalledWith(1);
  expect(screen.queryByText("Weekend plans")).not.toBeInTheDocument();
  await screen.findByRole("heading", { name: "Re: Invoice" });
});

it("restores the message when trashing fails on the server", async () => {
  vi.mocked(api.moveToTrash).mockRejectedValueOnce("imap error: no trash");
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));

  await fireEvent.click(screen.getByRole("button", { name: "Trash" }));

  // Optimistically removed, then brought back by the failure re-query.
  await waitFor(() =>
    expect(screen.getByText("Weekend plans")).toBeInTheDocument(),
  );
});

it("trashes a row when the left swipe is configured to trash", async () => {
  currentSwipeActions = { left: "trash", right: "reply" };
  render(App);
  const row = await screen.findByRole("option", { name: /Weekend plans/ });

  // why fake timers mid-test: only the gesture's settle window is faked —
  // the initial load polls with real timers (findBy*).
  vi.useFakeTimers();
  await fireEvent.wheel(row, { deltaX: 60 });
  await fireEvent.wheel(row, { deltaX: 60 });
  await vi.advanceTimersByTimeAsync(200);
  vi.useRealTimers();

  expect(api.moveToTrash).toHaveBeenCalledWith(1);
  expect(screen.queryByText("Weekend plans")).not.toBeInTheDocument();
});

it("opens a quoted reply when the right swipe is configured to reply", async () => {
  currentSwipeActions = { left: "trash", right: "reply" };
  render(App);
  const row = await screen.findByRole("option", { name: /Weekend plans/ });

  vi.useFakeTimers();
  await fireEvent.wheel(row, { deltaX: -120 });
  await vi.advanceTimersByTimeAsync(200);
  vi.useRealTimers();

  // The list has no body loaded — the reply fetches it to quote it.
  expect(api.getMessageBody).toHaveBeenCalledWith(1);
  await waitFor(() =>
    expect(api.openCompose).toHaveBeenCalledWith({
      accountId: 1,
      to: "alice@example.com",
      subject: "Re: Weekend plans",
      body: expect.stringContaining("> body text"),
    }),
  );
});

it("re-reads the swipe config when settings change", async () => {
  render(App);
  const row = await screen.findByRole("option", { name: /Weekend plans/ });

  currentSwipeActions = { left: "trash", right: "toggleRead" };
  settingsChanged?.();
  await waitFor(() =>
    expect(api.getSwipeActions).toHaveBeenCalledTimes(2),
  );

  vi.useFakeTimers();
  await fireEvent.wheel(row, { deltaX: 120 });
  await vi.advanceTimersByTimeAsync(200);
  vi.useRealTimers();

  expect(api.moveToTrash).toHaveBeenCalledWith(1);
  expect(api.archiveMessage).not.toHaveBeenCalled();
});

function missedEntry(over: Partial<ScheduledMessage> = {}): ScheduledMessage {
  return {
    id: 5,
    accountId: 1,
    aliasId: null,
    to: "alice@example.com",
    cc: "",
    bcc: "",
    subject: "Overdue hello",
    body: "",
    bodyHtml: null,
    attachments: [],
    scheduledAt: 1_752_600_000,
    status: "missed",
    inReplyTo: null,
    references: null,
    ...over,
  };
}

it("surfaces missed scheduled sends on start and sends on confirm", async () => {
  currentScheduled = [missedEntry()];
  render(App);

  expect(await screen.findByRole("alertdialog")).toBeInTheDocument();
  expect(screen.getByText("Overdue hello")).toBeInTheDocument();

  await fireEvent.click(
    screen.getByRole("button", { name: "Send Overdue hello now" }),
  );

  expect(api.sendScheduledNow).toHaveBeenCalledWith(5);
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
});

it("shows newly missed sends when the backend flags them mid-run", async () => {
  render(App);
  await screen.findByRole("button", { name: "All Inboxes" });
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

  currentScheduled = [missedEntry({ subject: "Slept through" })];
  scheduledMissed!();

  expect(await screen.findByRole("alertdialog")).toBeInTheDocument();
  expect(screen.getByText("Slept through")).toBeInTheDocument();

  // Reopening as a draft unschedules the row.
  await fireEvent.click(
    screen.getByRole("button", { name: "Open Slept through as draft" }),
  );
  expect(api.cancelScheduled).toHaveBeenCalledWith(5);
});

it("opens a drafts-folder message in compose instead of the viewer", async () => {
  vi.mocked(api.listMailboxes).mockImplementation(async (accountId: number) =>
    accountId === 1
      ? [
          {
            id: 1,
            accountId: 1,
            name: "INBOX",
            role: "inbox",
            displayName: "INBOX",
            unreadCount: 0,
          },
          {
            id: 4,
            accountId: 1,
            name: "Drafts",
            role: "drafts",
            displayName: "Drafts",
            unreadCount: 0,
          },
        ]
      : [],
  );
  currentMessages = [
    ...allMessages,
    {
      id: 7,
      accountId: 1,
      mailbox: "Drafts",
      from: "",
      to: "jan",
      cc: "",
      replyTo: "",
      messageId: "",
      references: "",
      threadCount: 1,
      threadUnread: false,
      subject: "Rozpísaný návrh",
      snippet: "",
      date: "2026-07-10T00:00:00Z",
      read: true,
    },
  ];
  render(App);

  await fireEvent.click(await screen.findByText("Rozpísaný návrh"));

  await waitFor(() => expect(api.openDraft).toHaveBeenCalledWith(7));
  // The viewer stayed closed — no body fetch, no reading pane heading.
  expect(api.getMessageBody).not.toHaveBeenCalled();
  expect(
    screen.queryByRole("heading", { name: "Rozpísaný návrh" }),
  ).not.toBeInTheDocument();
});

it("lists pending scheduled sends in the sidebar and cancels one", async () => {
  currentScheduled = [
    missedEntry({ id: 7, subject: "Later today", status: "pending" }),
  ];
  render(App);

  const section = await screen.findByRole("list", {
    name: "Scheduled messages",
  });
  expect(section).toBeInTheDocument();
  expect(screen.getByText("Later today")).toBeInTheDocument();
  // pending rows belong to the sidebar, not the missed dialog
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

  await fireEvent.click(
    screen.getByRole("button", { name: "Cancel scheduled Later today" }),
  );

  expect(api.cancelScheduled).toHaveBeenCalledWith(7);
  expect(screen.queryByText("Later today")).not.toBeInTheDocument();
});

it("refreshes the scheduled section when the backend broadcasts a change", async () => {
  render(App);
  await screen.findByRole("button", { name: "All Inboxes" });
  expect(screen.queryByText("Scheduled")).not.toBeInTheDocument();

  currentScheduled = [
    missedEntry({ id: 8, subject: "Tomorrow 8:00 mail", status: "pending" }),
  ];
  scheduledChanged!();

  expect(await screen.findByText("Tomorrow 8:00 mail")).toBeInTheDocument();
});

it("unarchives an archived message via Move to Inbox", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  // Open Personal's Archive folder (the sidebar one, not the toolbar
  // action) and select the archived message.
  await fireEvent.click(
    screen.getByRole("button", { name: "Toggle folders for Personal" }),
  );
  const sidebar = within(screen.getByRole("navigation"));
  await fireEvent.click(await sidebar.findByRole("button", { name: "Archive" }));
  await fireEvent.click(await screen.findByText("Archived note"));

  // On an archived message the archive action flips to Move to Inbox…
  const button = await screen.findByRole("button", { name: "Move to Inbox" });

  // …and moves it back to the account's inbox instead of re-archiving.
  await fireEvent.click(button);
  expect(api.moveMessage).toHaveBeenCalledWith(9, "INBOX");
  expect(api.archiveMessage).not.toHaveBeenCalled();
});
