import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Account, MessageHeader, NewAccount } from "./lib/types";

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

vi.mock("./lib/api", () => ({
  listAccounts: vi.fn(async () => accounts),
  listMessages: vi.fn(async (accountId: number | null) =>
    accountId === null
      ? allMessages
      : allMessages.filter((m) => m.accountId === accountId),
  ),
  addAccount: vi.fn(async (account: NewAccount, _password: string) => ({
    id: 99,
    ...account,
  })),
  deleteAccount: vi.fn(async () => undefined),
}));

import * as api from "./lib/api";
import App from "./App.svelte";

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

it("opens settings with cmd+comma and closes with escape", async () => {
  render(App);
  await screen.findByText("All Inboxes");

  await fireEvent.keyDown(window, { key: ",", metaKey: true });
  expect(
    await screen.findByRole("dialog", { name: "Settings" }),
  ).toBeInTheDocument();

  await fireEvent.keyDown(window, { key: "Escape" });
  expect(
    screen.queryByRole("dialog", { name: "Settings" }),
  ).not.toBeInTheDocument();
});

it("opens settings from the sidebar button", async () => {
  render(App);
  await screen.findByText("All Inboxes");

  await fireEvent.click(screen.getByText("Settings"));

  expect(
    await screen.findByRole("dialog", { name: "Settings" }),
  ).toBeInTheDocument();
});

it("adds an account through the form", async () => {
  render(App);
  await screen.findByText("All Inboxes");

  await fireEvent.click(screen.getByText("+ Add account"));
  await fireEvent.input(screen.getByLabelText("Name"), {
    target: { value: "New" },
  });
  await fireEvent.input(screen.getByLabelText("Email"), {
    target: { value: "new@example.com" },
  });
  await fireEvent.input(screen.getByLabelText("IMAP host"), {
    target: { value: "imap.new.com" },
  });
  await fireEvent.input(screen.getByLabelText("SMTP host"), {
    target: { value: "smtp.new.com" },
  });
  await fireEvent.input(screen.getByLabelText("Username"), {
    target: { value: "new@example.com" },
  });
  await fireEvent.input(screen.getByLabelText("Password"), {
    target: { value: "pw" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText("New")).toBeInTheDocument();
  expect(api.addAccount).toHaveBeenCalledWith(
    expect.objectContaining({ name: "New", imapHost: "imap.new.com" }),
    "pw",
  );
  expect(screen.queryByRole("button", { name: "Add" })).not.toBeInTheDocument();
});

it("deletes an account and falls back to the unified inbox", async () => {
  render(App);
  await fireEvent.click(await screen.findByText("Work"));
  await screen.findByText("Re: Invoice");

  await fireEvent.click(screen.getByLabelText("Delete Work"));

  expect(api.deleteAccount).toHaveBeenCalledWith(2);
  expect(await screen.findByText("Weekend plans")).toBeInTheDocument();
  expect(screen.queryByText("Work")).not.toBeInTheDocument();
});
