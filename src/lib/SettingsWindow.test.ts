import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Account, NewAccount } from "./types";

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

vi.mock("./api", () => ({
  listAccounts: vi.fn(async () => accounts),
  addAccount: vi.fn(async (account: NewAccount, _password: string) => ({
    id: 99,
    ...account,
  })),
  deleteAccount: vi.fn(async () => undefined),
  confirmAccountDeletion: vi.fn(async () => true),
  closeSettings: vi.fn(async () => undefined),
  onAccountsChanged: vi.fn(async () => () => {}),
}));

import * as api from "./api";
import SettingsWindow from "./SettingsWindow.svelte";

// why: every field is `required` and jsdom enforces constraint validation —
// a partially filled form never fires submit.
async function fillAccountForm() {
  await fireEvent.click(screen.getByLabelText("Add account"));
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
}

it("loads accounts and shows the first one's details", async () => {
  render(SettingsWindow);

  expect(
    await screen.findByText("imap.example.com:993"),
  ).toBeInTheDocument();
  expect(screen.getByText("smtp.example.com:587")).toBeInTheDocument();
});

it("adds an account through the form", async () => {
  render(SettingsWindow);
  await screen.findByText("imap.example.com:993");

  await fillAccountForm();

  expect(api.addAccount).toHaveBeenCalledWith(
    expect.objectContaining({ name: "New", imapHost: "imap.new.com" }),
    "pw",
  );
  // the pane refreshed and left the add form — the detail view is back
  expect(
    await screen.findByText("imap.example.com:993"),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Add" }),
  ).not.toBeInTheDocument();
});

it("deletes the selected account after native confirmation", async () => {
  render(SettingsWindow);
  await screen.findByText("Work");

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.click(screen.getByLabelText("Delete account"));

  expect(api.confirmAccountDeletion).toHaveBeenCalledWith(
    expect.objectContaining({ id: 2, name: "Work" }),
  );
  expect(api.deleteAccount).toHaveBeenCalledWith(2);
});

it("keeps the account when the confirmation is declined", async () => {
  vi.mocked(api.confirmAccountDeletion).mockResolvedValueOnce(false);
  vi.mocked(api.deleteAccount).mockClear();
  render(SettingsWindow);
  await screen.findByText("Work");

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.click(screen.getByLabelText("Delete account"));

  expect(api.confirmAccountDeletion).toHaveBeenCalled();
  expect(api.deleteAccount).not.toHaveBeenCalled();
});

it("shows an error and keeps the form open when adding fails", async () => {
  vi.mocked(api.addAccount).mockRejectedValueOnce("keychain says no");
  render(SettingsWindow);
  await screen.findByText("imap.example.com:993");

  await fillAccountForm();

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "keychain says no",
  );
  expect(screen.getByRole("button", { name: "Add" })).toBeInTheDocument();
});

it("closes the window on escape", async () => {
  render(SettingsWindow);
  await screen.findByText("imap.example.com:993");

  await fireEvent.keyDown(window, { key: "Escape" });

  expect(api.closeSettings).toHaveBeenCalled();
});
