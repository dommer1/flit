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
  queueSend: vi.fn(async (): Promise<void> => undefined),
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

async function renderLoaded() {
  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("1"));
  await fireEvent.input(screen.getByLabelText("To"), {
    target: { value: "bob@example.com" },
  });
}

it("queues the send and closes the window immediately", async () => {
  await renderLoaded();

  await fireEvent.input(screen.getByLabelText("Subject"), {
    target: { value: "Hello" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.queueSend).toHaveBeenCalledWith({
      accountId: 1,
      to: "bob@example.com",
      cc: "",
      bcc: "",
      subject: "Hello",
      body: "",
    }),
  );
  await waitFor(() => expect(api.closeCompose).toHaveBeenCalled());
});

it("reveals cc/bcc on demand and queues them with the send", async () => {
  await renderLoaded();

  expect(screen.queryByLabelText("Cc")).toBeNull();
  expect(screen.queryByLabelText("Bcc")).toBeNull();

  await fireEvent.click(screen.getByRole("button", { name: "Cc/Bcc" }));
  await fireEvent.input(screen.getByLabelText("Cc"), {
    target: { value: "carol@example.com" },
  });
  await fireEvent.input(screen.getByLabelText("Bcc"), {
    target: { value: "hidden@example.com" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.queueSend).toHaveBeenCalledWith({
      accountId: 1,
      to: "bob@example.com",
      cc: "carol@example.com",
      bcc: "hidden@example.com",
      subject: "",
      body: "",
    }),
  );
});

it("shows the cc/bcc fields right away when the draft carries them", async () => {
  vi.mocked(api.takeComposeDraft).mockResolvedValueOnce({
    accountId: 1,
    to: "alice@example.com",
    cc: "carol@example.com",
    bcc: "",
    subject: "Re: Weekend plans",
    body: "",
  });

  render(ComposeWindow);

  await waitFor(() =>
    expect(screen.getByLabelText("Cc")).toHaveValue("carol@example.com"),
  );
  expect(screen.getByLabelText("Bcc")).toHaveValue("");
});

it("shows the failure and stays open when queueing fails", async () => {
  vi.mocked(api.queueSend).mockRejectedValueOnce(
    "smtp error: invalid recipient",
  );
  await renderLoaded();

  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "smtp error: invalid recipient",
  );
  expect(api.closeCompose).not.toHaveBeenCalled();
  expect(screen.getByLabelText("To")).toBeEnabled();
  expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
});
