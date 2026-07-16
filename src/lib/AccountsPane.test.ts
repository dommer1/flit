import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Account } from "./types";
import AccountsPane from "./AccountsPane.svelte";

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
  },
];

function renderPane(overrides: Record<string, unknown> = {}) {
  const props = {
    accounts,
    onAdd: vi.fn(async () => null),
    onDelete: vi.fn(),
    onSetColor: vi.fn(),
    ...overrides,
  };
  render(AccountsPane, { props });
  return props;
}

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
  await fireEvent.click(screen.getByRole("button", { name: "Verify & Save" }));
}

it("shows the first account's details by default", () => {
  renderPane();

  expect(screen.getByText("imap.example.com:993")).toBeInTheDocument();
  expect(screen.getByText("smtp.example.com:587")).toBeInTheDocument();
});

it("switches the detail pane when another account is selected", async () => {
  renderPane();

  await fireEvent.click(screen.getByText("Work"));

  expect(screen.getByText("imap.vocalio.sk:993")).toBeInTheDocument();
});

it("hands the selected account to onDelete", async () => {
  const { onDelete } = renderPane();

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.click(screen.getByLabelText("Delete account"));

  expect(onDelete).toHaveBeenCalledWith(accounts[1]);
});

it("submits a new account and closes the form on success", async () => {
  const onAdd = vi.fn(async () => accounts[0]);
  renderPane({ onAdd });

  await fillAccountForm();

  expect(onAdd).toHaveBeenCalledWith(
    expect.objectContaining({ name: "New", imapHost: "imap.new.com" }),
    "pw",
  );
  expect(
    screen.queryByRole("button", { name: "Verify & Save" }),
  ).not.toBeInTheDocument();
});

it("keeps the form open when adding fails", async () => {
  const { onAdd } = renderPane(); // default onAdd resolves to null = failure

  await fillAccountForm();

  expect(onAdd).toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Verify & Save" })).toBeInTheDocument();
});

it("shows connected status for a healthy account", () => {
  renderPane();

  expect(screen.getByText("Connected")).toBeInTheDocument();
  expect(screen.queryByTitle("Connection problem")).not.toBeInTheDocument();
});

it("shows the error and a list warning for a broken account", async () => {
  const broken = {
    ...accounts[1],
    lastError: "imap error: login: denied",
    checkedAt: 1751990000,
  };
  renderPane({ accounts: [accounts[0], broken] });

  // warning marker in the list, visible without selecting the account
  expect(screen.getByTitle("Connection problem")).toBeInTheDocument();

  await fireEvent.click(screen.getByText("Work"));

  expect(screen.getByText("imap error: login: denied")).toBeInTheDocument();
  expect(screen.queryByText("Connected")).not.toBeInTheDocument();
});

it("shows not-checked-yet for a brand new account", () => {
  const fresh = { ...accounts[0], lastError: null, checkedAt: null };
  renderPane({ accounts: [fresh] });

  expect(screen.getByText("Not checked yet")).toBeInTheDocument();
});

it("shows an empty state and disables delete without accounts", () => {
  renderPane({ accounts: [] });

  expect(screen.getByText(/No accounts yet/)).toBeInTheDocument();
  expect(screen.getByLabelText("Delete account")).toBeDisabled();
});

it("sets an account color from the palette", async () => {
  const props = renderPane();

  await fireEvent.click(screen.getByLabelText("Orange"));
  expect(props.onSetColor).toHaveBeenCalledWith(1, "#ff9f0a");
});

it("clears the color with the no-color swatch", async () => {
  const colored = { ...accounts[0], color: "#ff9f0a" };
  const props = renderPane({ accounts: [colored] });

  await fireEvent.click(screen.getByLabelText("No color"));
  expect(props.onSetColor).toHaveBeenCalledWith(1, null);
});
