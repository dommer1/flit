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
  },
];

const allMessages: MessageHeader[] = [
  {
    id: 1,
    accountId: 1,
    from: "Alice",
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
import App from "./App.svelte";

beforeEach(() => {
  currentAccounts = [...accounts];
  currentMessages = [...allMessages];
  accountsChanged = undefined;
  messagesChanged = undefined;
  vi.clearAllMocks();
});

it("loads accounts and the unified inbox on start", async () => {
  render(App);

  expect(await screen.findByText("All Inboxes")).toBeInTheDocument();
  expect(screen.getByText("Personal")).toBeInTheDocument();
  expect(screen.getByText("Work")).toBeInTheDocument();
  expect(await screen.findByText("Weekend plans")).toBeInTheDocument();
  expect(screen.getByText("Re: Invoice")).toBeInTheDocument();
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

it("starts a sync for every account on launch", async () => {
  render(App);
  await screen.findByText("Weekend plans");

  await waitFor(() => {
    expect(api.syncInbox).toHaveBeenCalledWith(1);
    expect(api.syncInbox).toHaveBeenCalledWith(2);
  });
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

it("falls back to the unified inbox when the selected account disappears", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Work"));
  await screen.findByText("Re: Invoice");

  currentAccounts = [accounts[0]];
  accountsChanged?.();

  expect(await screen.findByText("Weekend plans")).toBeInTheDocument();
  expect(screen.queryByText("Work")).not.toBeInTheDocument();
});
