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
    color: null,
    signatureId: null,
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
    color: null,
    signatureId: null,
  },
];

// Captured by the onFileDrop mock so tests can simulate a native file drop.
let dropCallbacks: {
  onHover: (hovering: boolean) => void;
  onDrop: (paths: string[]) => void;
} | null = null;

// Captured by the window mock so tests can simulate the red traffic light.
type CloseEvent = { preventDefault: () => void };
let closeHandler: ((event: CloseEvent) => Promise<void>) | null = null;
const destroyWindow = vi.fn(async (): Promise<void> => undefined);

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: async (handler: (event: CloseEvent) => Promise<void>) => {
      closeHandler = handler;
      return () => {};
    },
    destroy: destroyWindow,
  }),
}));

vi.mock("./api", () => ({
  listAccounts: vi.fn(async () => accounts),
  takeComposeDraft: vi.fn(async (): Promise<OutgoingMessage | null> => null),
  queueSend: vi.fn(async (): Promise<void> => undefined),
  scheduleSend: vi.fn(async (): Promise<void> => undefined),
  closeCompose: vi.fn(async (): Promise<void> => undefined),
  saveDraft: vi.fn(async (): Promise<string> => "draft-id-1@flit.local"),
  inspectAttachments: vi.fn(async (paths: string[]) =>
    paths.map((path) => ({
      path,
      name: path.split("/").pop() ?? path,
      size: 2048,
    })),
  ),
  onFileDrop: vi.fn(
    async (callbacks: {
      onHover: (hovering: boolean) => void;
      onDrop: (paths: string[]) => void;
    }) => {
      dropCallbacks = callbacks;
      return () => {};
    },
  ),
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
  expect(
    await screen.findByRole("textbox", { name: "Message body" }),
  ).toHaveTextContent("> quoted");
});

it("sends the html rendering alongside the plain text body", async () => {
  vi.mocked(api.takeComposeDraft).mockResolvedValueOnce({
    accountId: 1,
    to: "alice@example.com",
    subject: "Lists",
    body: "hello",
  });

  render(ComposeWindow);

  // Format the prefilled paragraph as a bullet list via the toolbar.
  await fireEvent.click(
    await screen.findByRole("button", { name: "Bullet list" }),
  );
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.queueSend).toHaveBeenCalledWith(
      expect.objectContaining({
        body: "hello",
        bodyHtml: expect.stringContaining("<ul"),
      }),
    ),
  );
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
      attachments: [],
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
      attachments: [],
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

it("attaches dropped files and sends them with the message", async () => {
  await renderLoaded();

  dropCallbacks!.onDrop(["/tmp/report.pdf", "/tmp/photo.jpg"]);

  expect(await screen.findByText("report.pdf")).toBeInTheDocument();
  expect(screen.getByText("photo.jpg")).toBeInTheDocument();
  expect(screen.getAllByText("2 KB")).toHaveLength(2);

  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.queueSend).toHaveBeenCalledWith(
      expect.objectContaining({
        attachments: [
          { path: "/tmp/report.pdf", name: "report.pdf" },
          { path: "/tmp/photo.jpg", name: "photo.jpg" },
        ],
      }),
    ),
  );
});

it("removes an attachment chip and drops repeat paths", async () => {
  await renderLoaded();

  dropCallbacks!.onDrop(["/tmp/report.pdf"]);
  expect(await screen.findByText("report.pdf")).toBeInTheDocument();
  // The same file dropped again must not attach twice.
  dropCallbacks!.onDrop(["/tmp/report.pdf", "/tmp/photo.jpg"]);
  expect(await screen.findByText("photo.jpg")).toBeInTheDocument();
  expect(screen.getAllByText("report.pdf")).toHaveLength(1);

  await fireEvent.click(
    screen.getByRole("button", { name: "Remove report.pdf" }),
  );
  expect(screen.queryByText("report.pdf")).toBeNull();

  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.queueSend).toHaveBeenCalledWith(
      expect.objectContaining({
        attachments: [{ path: "/tmp/photo.jpg", name: "photo.jpg" }],
      }),
    ),
  );
});

it("restores a reopened draft's attachments as chips", async () => {
  vi.mocked(api.takeComposeDraft).mockResolvedValueOnce({
    accountId: 1,
    to: "alice@example.com",
    subject: "Re: Files",
    body: "",
    attachments: [{ path: "/tmp/report.pdf", name: "report.pdf" }],
  });

  render(ComposeWindow);

  expect(await screen.findByText("report.pdf")).toBeInTheDocument();
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

it("autosaves the draft after the idle window", async () => {
  await renderLoaded();

  vi.useFakeTimers();
  try {
    await fireEvent.input(screen.getByLabelText("Subject"), {
      target: { value: "Rozpísané" },
    });
    await vi.advanceTimersByTimeAsync(30_000);

    expect(api.saveDraft).toHaveBeenCalledWith(
      expect.objectContaining({ subject: "Rozpísané", to: "bob@example.com" }),
      null,
    );
  } finally {
    vi.useRealTimers();
  }
});

it("replaces the previous draft version on the next autosave", async () => {
  await renderLoaded();

  vi.useFakeTimers();
  try {
    await fireEvent.input(screen.getByLabelText("Subject"), {
      target: { value: "v1" },
    });
    await vi.advanceTimersByTimeAsync(30_000);
    await fireEvent.input(screen.getByLabelText("Subject"), {
      target: { value: "v2" },
    });
    await vi.advanceTimersByTimeAsync(30_000);

    expect(api.saveDraft).toHaveBeenLastCalledWith(
      expect.objectContaining({ subject: "v2" }),
      "draft-id-1@flit.local",
    );
  } finally {
    vi.useRealTimers();
  }
});

it("never autosaves an untouched or emptied-out new draft", async () => {
  await renderLoaded();

  vi.useFakeTimers();
  try {
    await fireEvent.input(screen.getByLabelText("To"), {
      target: { value: "   " },
    });
    await vi.advanceTimersByTimeAsync(60_000);

    expect(api.saveDraft).not.toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
});

it("saves a dirty draft before the window closes", async () => {
  await renderLoaded();

  const preventDefault = vi.fn();
  await closeHandler!({ preventDefault });

  expect(preventDefault).toHaveBeenCalled();
  expect(api.saveDraft).toHaveBeenCalledWith(
    expect.objectContaining({ to: "bob@example.com" }),
    null,
  );
  expect(destroyWindow).toHaveBeenCalled();
});

it("lets a clean window close without saving", async () => {
  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("1"));

  const preventDefault = vi.fn();
  await closeHandler!({ preventDefault });

  expect(preventDefault).not.toHaveBeenCalled();
  expect(api.saveDraft).not.toHaveBeenCalled();
  expect(destroyWindow).not.toHaveBeenCalled();
});

it("stays open and shows the failure when the close-save fails", async () => {
  vi.mocked(api.saveDraft).mockRejectedValueOnce("imap error: offline");
  await renderLoaded();

  const preventDefault = vi.fn();
  await closeHandler!({ preventDefault });

  expect(preventDefault).toHaveBeenCalled();
  expect(destroyWindow).not.toHaveBeenCalled();
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Draft not saved: imap error: offline",
  );
});

it("does not snapshot a sent message back into drafts on close", async () => {
  await renderLoaded();

  await fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(api.closeCompose).toHaveBeenCalled());

  const preventDefault = vi.fn();
  await closeHandler!({ preventDefault });

  expect(preventDefault).not.toHaveBeenCalled();
  expect(api.saveDraft).not.toHaveBeenCalled();
});

it("schedules the message for a custom time and closes", async () => {
  await renderLoaded();

  await fireEvent.click(screen.getByRole("button", { name: "Send later" }));
  await fireEvent.input(screen.getByLabelText("Send at"), {
    target: { value: "2099-01-01T09:30" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Schedule" }));

  await waitFor(() =>
    expect(api.scheduleSend).toHaveBeenCalledWith(
      expect.objectContaining({ accountId: 1, to: "bob@example.com" }),
      Math.floor(new Date(2099, 0, 1, 9, 30).getTime() / 1000),
    ),
  );
  await waitFor(() => expect(api.closeCompose).toHaveBeenCalled());
});

it("offers presets that schedule a future time", async () => {
  await renderLoaded();

  await fireEvent.click(screen.getByRole("button", { name: "Send later" }));
  await fireEvent.click(screen.getByRole("button", { name: "Tomorrow 8:00" }));

  await waitFor(() => expect(api.scheduleSend).toHaveBeenCalled());
  const [, scheduledAt] = vi.mocked(api.scheduleSend).mock.calls[0];
  expect(scheduledAt).toBeGreaterThan(Date.now() / 1000);
});

it("shows the failure and stays open when scheduling fails", async () => {
  vi.mocked(api.scheduleSend).mockRejectedValueOnce(
    "scheduled time is in the past",
  );
  await renderLoaded();

  await fireEvent.click(screen.getByRole("button", { name: "Send later" }));
  await fireEvent.input(screen.getByLabelText("Send at"), {
    target: { value: "2099-01-01T09:30" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Schedule" }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "scheduled time is in the past",
  );
  expect(api.closeCompose).not.toHaveBeenCalled();
});
