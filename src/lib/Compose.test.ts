import { expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { Account } from "./types";
import Compose from "./Compose.svelte";

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
    checkedAt: null,
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
    checkedAt: null,
  },
];

const emptyDraft = { accountId: 1, to: "", subject: "", body: "" };

function setup(draft = emptyDraft) {
  const onSend = vi.fn(async (): Promise<void> => undefined);
  const onCancel = vi.fn();
  render(Compose, { accounts, draft, onSend, onCancel });
  return { onSend, onCancel };
}

it("prefills the form from the draft", () => {
  setup({
    accountId: 2,
    to: "alice@example.com",
    subject: "Re: Weekend plans",
    body: "> quoted",
  });

  expect(screen.getByLabelText("From")).toHaveValue("2");
  expect(screen.getByLabelText("To")).toHaveValue("alice@example.com");
  expect(screen.getByLabelText("Subject")).toHaveValue("Re: Weekend plans");
  expect(screen.getByLabelText("Message body")).toHaveValue("> quoted");
});

it("sends what the user typed, from the selected account", async () => {
  const { onSend } = setup();

  await fireEvent.change(screen.getByLabelText("From"), {
    target: { value: "2" },
  });
  await fireEvent.input(screen.getByLabelText("To"), {
    target: { value: "bob@example.com" },
  });
  await fireEvent.input(screen.getByLabelText("Subject"), {
    target: { value: "Hello" },
  });
  await fireEvent.input(screen.getByLabelText("Message body"), {
    target: { value: "Hi Bob" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(onSend).toHaveBeenCalledWith({
      accountId: 2,
      to: "bob@example.com",
      subject: "Hello",
      body: "Hi Bob",
    }),
  );
});

it("shows the failure and stays open when sending fails", async () => {
  const { onSend } = setup({ ...emptyDraft, to: "bob@example.com" });
  onSend.mockRejectedValueOnce("smtp error: relay refused");

  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "smtp error: relay refused",
  );
  expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
});

it("disables the send button while sending", async () => {
  const { onSend } = setup({ ...emptyDraft, to: "bob@example.com" });
  let finish!: () => void;
  onSend.mockImplementationOnce(
    () => new Promise<void>((resolve) => (finish = resolve)),
  );

  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  expect(screen.getByRole("button", { name: "Sending…" })).toBeDisabled();
  finish();
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled(),
  );
});

it("cancels without sending", async () => {
  const { onSend, onCancel } = setup();

  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(onCancel).toHaveBeenCalled();
  expect(onSend).not.toHaveBeenCalled();
});
