import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { Account, OutgoingMessage } from "./types";

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

vi.mock("./api", () => ({
  listAccounts: vi.fn(async () => accounts),
  takeComposeDraft: vi.fn(async (): Promise<OutgoingMessage | null> => null),
  sendMessage: vi.fn(async (): Promise<void> => undefined),
  closeCompose: vi.fn(async (): Promise<void> => undefined),
}));

import * as api from "./api";
import ComposeWindow from "./ComposeWindow.svelte";

beforeEach(() => {
  vi.clearAllMocks();
});

it("prefills the form from the parked draft", async () => {
  vi.mocked(api.takeComposeDraft).mockResolvedValueOnce({
    accountId: 2,
    to: "alice@example.com",
    subject: "Re: Weekend plans",
    body: "> quoted",
  });

  render(ComposeWindow);

  await waitFor(() =>
    expect(screen.getByLabelText("From")).toHaveValue("2"),
  );
  expect(screen.getByLabelText("To")).toHaveValue("alice@example.com");
  expect(screen.getByLabelText("Subject")).toHaveValue("Re: Weekend plans");
  expect(screen.getByLabelText("Message body")).toHaveValue("> quoted");
});

it("falls back to an empty draft from the first account", async () => {
  render(ComposeWindow);

  await waitFor(() =>
    expect(screen.getByLabelText("From")).toHaveValue("1"),
  );
  expect(screen.getByLabelText("To")).toHaveValue("");
  expect(screen.getByLabelText("Subject")).toHaveValue("");
});

it("sends what the user typed and closes the window", async () => {
  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("1"));

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
    expect(api.sendMessage).toHaveBeenCalledWith({
      accountId: 2,
      to: "bob@example.com",
      subject: "Hello",
      body: "Hi Bob",
    }),
  );
  await waitFor(() => expect(api.closeCompose).toHaveBeenCalled());
});

it("shows the failure and stays open when sending fails", async () => {
  vi.mocked(api.sendMessage).mockRejectedValueOnce("smtp error: relay refused");
  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("1"));
  await fireEvent.input(screen.getByLabelText("To"), {
    target: { value: "bob@example.com" },
  });

  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "smtp error: relay refused",
  );
  expect(api.closeCompose).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
});

it("disables the send button while sending", async () => {
  let finish!: () => void;
  vi.mocked(api.sendMessage).mockImplementationOnce(
    () => new Promise<void>((resolve) => (finish = resolve)),
  );
  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("1"));
  await fireEvent.input(screen.getByLabelText("To"), {
    target: { value: "bob@example.com" },
  });

  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  expect(screen.getByRole("button", { name: "Sending…" })).toBeDisabled();
  finish();
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled(),
  );
});

it("cancels without sending", async () => {
  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("1"));

  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(api.closeCompose).toHaveBeenCalled();
  expect(api.sendMessage).not.toHaveBeenCalled();
});
