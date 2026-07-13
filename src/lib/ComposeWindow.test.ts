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

// why fake timers mid-test: the initial load must run on real timers
// (waitFor polls with them), only the undo window itself is faked.
async function renderLoaded() {
  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("1"));
  await fireEvent.input(screen.getByLabelText("To"), {
    target: { value: "bob@example.com" },
  });
}

it("holds the send behind an undo countdown", async () => {
  await renderLoaded();

  vi.useFakeTimers();
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  // nothing left the machine yet — the countdown is the whole point
  expect(api.sendMessage).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: /Undo/ })).toBeInTheDocument();
  expect(screen.getByLabelText("To")).toBeDisabled();

  await vi.advanceTimersByTimeAsync(8000);
  vi.useRealTimers();

  expect(api.sendMessage).toHaveBeenCalledWith({
    accountId: 1,
    to: "bob@example.com",
    subject: "",
    body: "",
  });
});

it("undo aborts the send and unlocks the draft", async () => {
  await renderLoaded();

  vi.useFakeTimers();
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await vi.advanceTimersByTimeAsync(3000);
  await fireEvent.click(screen.getByRole("button", { name: /Undo/ }));
  await vi.advanceTimersByTimeAsync(30000);
  vi.useRealTimers();

  expect(api.sendMessage).not.toHaveBeenCalled();
  expect(api.closeCompose).not.toHaveBeenCalled();
  expect(screen.getByLabelText("To")).toBeEnabled();
  expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
});

it("confirms the sent message before closing the window", async () => {
  await renderLoaded();

  vi.useFakeTimers();
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await vi.advanceTimersByTimeAsync(8000);

  expect(api.sendMessage).toHaveBeenCalled();
  expect(screen.getByRole("status")).toHaveTextContent("Message sent");
  expect(api.closeCompose).not.toHaveBeenCalled();

  await vi.advanceTimersByTimeAsync(1500);
  vi.useRealTimers();

  expect(api.closeCompose).toHaveBeenCalled();
});

it("shows the failure and stays open when sending fails", async () => {
  vi.mocked(api.sendMessage).mockRejectedValueOnce("smtp error: relay refused");
  await renderLoaded();

  vi.useFakeTimers();
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await vi.advanceTimersByTimeAsync(8000);
  vi.useRealTimers();

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "smtp error: relay refused",
  );
  expect(api.closeCompose).not.toHaveBeenCalled();
  expect(screen.getByLabelText("To")).toBeEnabled();
  expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
});

it("cancels without sending", async () => {
  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("1"));

  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(api.closeCompose).toHaveBeenCalled();
  expect(api.sendMessage).not.toHaveBeenCalled();
});
