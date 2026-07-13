import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { Account, MessageHeader, SendEvent } from "./lib/types";

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
  },
];

const allMessages: MessageHeader[] = [
  {
    id: 1,
    accountId: 1,
    from: "Alice <alice@example.com>",
    to: "hello@vocalio.sk",
    cc: "",
    subject: "Weekend plans",
    snippet: "Are we still on for Saturday?",
    date: "2026-07-07T09:15:00Z",
    read: false,
  },
  {
    id: 2,
    accountId: 2,
    from: "Peter",
    to: "hello@vocalio.sk",
    cc: "",
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
    from: "Old Friend <old@example.com>",
    to: "hello@vocalio.sk",
    cc: "",
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
let accountsChanged: (() => void) | undefined;
let messagesChanged: (() => void) | undefined;
let sendQueued: ((e: SendEvent) => void) | undefined;
let sendFinished: ((e: SendEvent) => void) | undefined;
let sendUndone: ((e: SendEvent) => void) | undefined;

vi.mock("./lib/api", () => ({
  listAccounts: vi.fn(async () => currentAccounts),
  listMailboxes: vi.fn(async (accountId: number) =>
    accountId === 1
      ? [
          { id: 1, accountId: 1, name: "INBOX", role: "inbox", displayName: "INBOX" },
          { id: 2, accountId: 1, name: "Archive", role: "archive", displayName: "Archive" },
          // A Gmail-style folder: the wire name stays modified UTF-7, the
          // backend ships the decoded label alongside it.
          {
            id: 3,
            accountId: 1,
            name: "[Gmail]/Odoslan&AOk-",
            role: "sent",
            displayName: "Odoslané",
          },
        ]
      : [],
  ),
  listMessages: vi.fn(async (accountId: number | null, mailbox = "INBOX") => {
    const pool = mailbox === "Archive" ? archivedMessages : currentMessages;
    return accountId === null
      ? pool
      : pool.filter((m) => m.accountId === accountId);
  }),
  // why: a canned single-hit result — App tests only assert the wiring
  // (what was called with what); real matching is covered by Rust tests.
  searchMessages: vi.fn(async () => [currentMessages[1]]),
  getMessageBody: vi.fn(async () => ({ html: null, text: "body text" })),
  syncAccount: vi.fn(async () => undefined),
  openCompose: vi.fn(async () => undefined),
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
}));

import * as api from "./lib/api";
import { PANE_WIDTHS_KEY } from "./lib/paneSizes";
import App from "./App.svelte";

beforeEach(() => {
  currentAccounts = [...accounts];
  currentMessages = [...allMessages];
  accountsChanged = undefined;
  messagesChanged = undefined;
  sendQueued = undefined;
  sendFinished = undefined;
  sendUndone = undefined;
  localStorage.clear();
  vi.clearAllMocks();
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
  // the list pane header names the current mailbox and counts its messages
  expect(
    screen.getByRole("heading", { name: "All Inboxes" }),
  ).toBeInTheDocument();
  expect(screen.getByText("2 messages")).toBeInTheDocument();
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
  await fireEvent.click(await screen.findByRole("button", { name: "Archive" }));

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

// note: settings open only through the native macOS app menu (Settings…, ⌘,
// — src-tauri lib.rs), so there is no webview trigger left to test here.

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
      from: "New Sender",
      to: "hello@vocalio.sk",
      cc: "",
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
