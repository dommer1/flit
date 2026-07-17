import { expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/svelte";
import Sidebar from "./Sidebar.svelte";
import type { Account, Mailbox } from "./types";

function account(id: number, name: string): Account {
  return {
    id,
    name,
    email: `${name.toLowerCase()}@example.com`,
    imapHost: "imap.example.com",
    imapPort: 993,
    smtpHost: "smtp.example.com",
    smtpPort: 587,
    username: `${name.toLowerCase()}@example.com`,
    lastError: null,
    checkedAt: null,
    color: null,
    signatureId: null,
    notifyEnabled: null,
    notifySound: null,
    defaultAliasId: null,
  };
}

function mailbox(
  id: number,
  accountId: number,
  name: string,
  role: string | null,
  unreadCount: number,
): Mailbox {
  return { id, accountId, name, role, displayName: name, unreadCount };
}

function renderSidebar(props: Record<string, unknown> = {}) {
  return render(Sidebar, {
    props: {
      accounts: [account(1, "Personal"), account(2, "Work")],
      mailboxes: {
        1: [
          mailbox(1, 1, "INBOX", "inbox", 3),
          mailbox(2, 1, "Archive", "archive", 5),
          mailbox(3, 1, "Sent", "sent", 0),
        ],
        2: [mailbox(4, 2, "INBOX", "inbox", 2)],
      },
      selectedAccountId: null,
      selectedMailbox: "INBOX",
      onSelect: vi.fn(),
      scheduled: [],
      onCancelScheduled: vi.fn(),
      ...props,
    },
  });
}

it("shows each folder's unread count and hides zero", () => {
  localStorage.setItem("flit.sidebar.expanded", JSON.stringify({ 1: true }));
  renderSidebar();

  const archive = screen.getByRole("button", { name: /Archive/ });
  expect(within(archive).getByText("5")).toBeInTheDocument();
  const sent = screen.getByRole("button", { name: /^Sent/ });
  expect(within(sent).queryByText("0")).not.toBeInTheDocument();
});

it("badges the account row with its inbox unread count", () => {
  renderSidebar();

  // ^ anchors past the chevron, whose name is "Toggle folders for Personal".
  const personal = screen.getByRole("button", { name: /^Personal/ });
  expect(within(personal).getByText("3")).toBeInTheDocument();
});

it("badges All Inboxes with the sum across accounts", () => {
  renderSidebar();

  const unified = screen.getByRole("button", { name: /All Inboxes/ });
  expect(within(unified).getByText("5")).toBeInTheDocument();
});
