import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { Account, MessageHeader } from "./lib/types";

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
    subject: "Weekend plans",
    snippet: "Are we still on for Saturday?",
    date: "2026-07-07T09:15:00Z",
    read: false,
  },
  {
    id: 2,
    accountId: 2,
    from: "Peter",
    subject: "Re: Invoice",
    snippet: "Payment went out this morning.",
    date: "2026-07-08T07:45:00Z",
    read: true,
  },
];

// why: mutable + captured by the mock factory, so tests can simulate the
// backend changing state and firing change events.
let currentAccounts: Account[] = [];
let currentMessages: MessageHeader[] = [];
let accountsChanged: (() => void) | undefined;
let messagesChanged: (() => void) | undefined;

vi.mock("./lib/api", () => ({
  listAccounts: vi.fn(async () => currentAccounts),
  listMessages: vi.fn(async (accountId: number | null) =>
    accountId === null
      ? currentMessages
      : currentMessages.filter((m) => m.accountId === accountId),
  ),
  getMessageBody: vi.fn(async () => ({ html: null, text: "body text" })),
  syncInbox: vi.fn(async () => undefined),
  sendMessage: vi.fn(async () => undefined),
  onAccountsChanged: vi.fn(async (callback: () => void) => {
    accountsChanged = callback;
    return () => {};
  }),
  onMessagesChanged: vi.fn(async (callback: () => void) => {
    messagesChanged = callback;
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

it("clears the selected message when switching accounts", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });

  await fireEvent.click(screen.getByText("Work"));

  expect(await screen.findByText("Select a message")).toBeInTheDocument();
});

// note: settings open only through the native macOS app menu (Settings…, ⌘,
// — src-tauri lib.rs), so there is no webview trigger left to test here.

it("composes and sends a new message from the list header", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  await fireEvent.click(screen.getByRole("button", { name: "New Message" }));

  // the unified inbox has no account selected — From falls back to the first
  expect(screen.getByLabelText("From")).toHaveValue("1");
  await fireEvent.input(screen.getByLabelText("To"), {
    target: { value: "bob@example.com" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.sendMessage).toHaveBeenCalledWith({
      accountId: 1,
      to: "bob@example.com",
      subject: "",
      body: "",
    }),
  );
  // a successful send closes the compose sheet
  await waitFor(() =>
    expect(screen.queryByLabelText("To")).not.toBeInTheDocument(),
  );
});

it("prefills a reply from the account the message arrived on", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Weekend plans"));
  await screen.findByRole("heading", { name: "Weekend plans" });
  await screen.findByText("body text");

  await fireEvent.click(screen.getByRole("button", { name: "Reply" }));

  expect(screen.getByLabelText("From")).toHaveValue("1");
  expect(screen.getByLabelText("To")).toHaveValue("alice@example.com");
  expect(screen.getByLabelText("Subject")).toHaveValue("Re: Weekend plans");
  const body = screen.getByLabelText("Message body") as HTMLTextAreaElement;
  expect(body.value).toContain("> body text");
});

it("starts a sync for every account on launch", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  await waitFor(() => {
    expect(api.syncInbox).toHaveBeenCalledWith(1);
    expect(api.syncInbox).toHaveBeenCalledWith(2);
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
  vi.mocked(api.syncInbox)
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

  await waitFor(() => expect(api.syncInbox).toHaveBeenCalledWith(3));
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
