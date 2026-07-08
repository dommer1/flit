import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Account } from "./types";
import Settings from "./Settings.svelte";

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

function renderSettings(overrides: Record<string, unknown> = {}) {
  const props = {
    accounts,
    onAdd: vi.fn(async () => null),
    onDelete: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  render(Settings, { props });
  return props;
}

it("shows the first account's details by default", () => {
  renderSettings();

  expect(screen.getByText("imap.example.com:993")).toBeInTheDocument();
  expect(screen.getByText("smtp.example.com:587")).toBeInTheDocument();
});

it("switches the detail pane when another account is selected", async () => {
  renderSettings();

  await fireEvent.click(screen.getByText("Work"));

  expect(screen.getByText("imap.vocalio.sk:993")).toBeInTheDocument();
});

it("deletes the selected account", async () => {
  const { onDelete } = renderSettings();

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.click(screen.getByLabelText("Delete account"));

  expect(onDelete).toHaveBeenCalledWith(2);
});

it("submits a new account and closes the form on success", async () => {
  const onAdd = vi.fn(async () => accounts[0]);
  renderSettings({ onAdd });

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

  expect(onAdd).toHaveBeenCalledWith(
    expect.objectContaining({ name: "New", imapHost: "imap.new.com" }),
    "pw",
  );
  expect(
    screen.queryByRole("button", { name: "Add" }),
  ).not.toBeInTheDocument();
});

it("keeps the form open when adding fails", async () => {
  renderSettings(); // default onAdd resolves to null = failure

  await fireEvent.click(screen.getByLabelText("Add account"));
  await fireEvent.input(screen.getByLabelText("Name"), {
    target: { value: "New" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Add" }));

  expect(screen.getByRole("button", { name: "Add" })).toBeInTheDocument();
});

it("calls onClose from the close button", async () => {
  const { onClose } = renderSettings();

  await fireEvent.click(screen.getByLabelText("Close settings"));

  expect(onClose).toHaveBeenCalled();
});

it("shows an empty state and disables delete without accounts", () => {
  renderSettings({ accounts: [] });

  expect(screen.getByText(/No accounts yet/)).toBeInTheDocument();
  expect(screen.getByLabelText("Delete account")).toBeDisabled();
});
