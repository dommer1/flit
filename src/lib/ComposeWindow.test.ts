import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type {
  Account,
  Alias,
  Contact,
  OutgoingMessage,
  Signature,
} from "./types";

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
    notifyEnabled: null,
    notifySound: null,
    defaultAliasId: null,
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
    notifyEnabled: null,
    notifySound: null,
    defaultAliasId: null,
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

const aliases: Alias[] = [
  { id: 10, accountId: 2, name: "Igor", email: "igor@vocalio.sk" },
];

vi.mock("./api", () => ({
  listAccounts: vi.fn(async () => accounts),
  listAliases: vi.fn(async (): Promise<Alias[]> => []),
  listSignatures: vi.fn(async (): Promise<Signature[]> => []),
  listContacts: vi.fn(async (): Promise<Contact[]> => []),
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
  attachmentPreview: vi.fn(async (path: string) =>
    /\.(png|jpe?g|webp)$/.test(path) ? "data:image/png;base64,dGh1bWI=" : null,
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
  // Prefilled recipients arrive committed — a chip, not editable text.
  expect(await screen.findByText("alice@example.com")).toBeInTheDocument();
  expect(screen.getByLabelText("To")).toHaveValue("");
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
    expect(screen.getByText("carol@example.com")).toBeInTheDocument(),
  );
  expect(screen.getByLabelText("Cc")).toHaveValue("");
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

it("shows an image thumbnail on the attachment card", async () => {
  await renderLoaded();

  dropCallbacks!.onDrop(["/tmp/photo.jpg"]);

  const thumb = await screen.findByRole("img", { name: "photo.jpg" });
  expect(thumb).toHaveAttribute("src", "data:image/png;base64,dGh1bWI=");
});

it("shows an extension placeholder when there is no preview", async () => {
  await renderLoaded();

  dropCallbacks!.onDrop(["/tmp/report.pdf"]);

  expect(await screen.findByText("PDF")).toBeInTheDocument();
  expect(screen.queryByRole("img", { name: "report.pdf" })).toBeNull();
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

it("hands the autosaved draft id to the send queue", async () => {
  await renderLoaded();

  vi.useFakeTimers();
  try {
    await fireEvent.input(screen.getByLabelText("Subject"), {
      target: { value: "v1" },
    });
    await vi.advanceTimersByTimeAsync(30_000);
  } finally {
    vi.useRealTimers();
  }
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.queueSend).toHaveBeenCalledWith(
      expect.objectContaining({ draftMessageId: "draft-id-1@flit.local" }),
    ),
  );
});

it("keeps replacing the same server draft after an undo hands it back", async () => {
  vi.mocked(api.takeComposeDraft).mockResolvedValueOnce({
    accountId: 1,
    to: "alice@example.com",
    subject: "Rozpísané",
    body: "text",
    draftMessageId: "draft-id-0@flit.local",
  });
  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("1"));

  // No edits at all — closing must still re-save, because the handed-back
  // text may be newer than the last autosaved version.
  const preventDefault = vi.fn();
  await closeHandler!({ preventDefault });

  expect(api.saveDraft).toHaveBeenCalledWith(
    expect.objectContaining({ subject: "Rozpísané" }),
    "draft-id-0@flit.local",
  );
  expect(destroyWindow).toHaveBeenCalled();
});

// --- signatures in compose ---

const signatures: Signature[] = [
  { id: 7, name: "Personal", body: "<p>— Dominik</p>" },
  { id: 8, name: "Vocalio", body: "<p>Vocalio tím</p>" },
];

/** Accounts where the first one defaults to signature 7. */
function accountsWithDefaults(): Account[] {
  return [
    { ...accounts[0], signatureId: 7 },
    { ...accounts[1], signatureId: 8 },
  ];
}

async function renderWithSignatures() {
  vi.mocked(api.listAccounts).mockResolvedValueOnce(accountsWithDefaults());
  vi.mocked(api.listSignatures).mockResolvedValueOnce(signatures);
  render(ComposeWindow);
  return await screen.findByRole("textbox", { name: "Message body" });
}

it("inserts the From account's default signature into a new message", async () => {
  const box = await renderWithSignatures();

  expect(box).toHaveTextContent("— Dominik");
  // The icon opens a menu where the active signature is marked.
  await fireEvent.click(screen.getByRole("button", { name: "Signature" }));
  expect(screen.getByRole("button", { name: "Personal" })).toHaveClass(
    "active",
  );
  await fireEvent.click(screen.getByRole("button", { name: "Signature" }));
  expect(screen.queryByRole("button", { name: "Personal" })).toBeNull();

  await fireEvent.input(screen.getByLabelText("To"), {
    target: { value: "bob@example.com" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.queueSend).toHaveBeenCalledWith(
      expect.objectContaining({
        body: expect.stringContaining("— Dominik"),
        bodyHtml: expect.stringContaining("— Dominik"),
      }),
    ),
  );
});

it("swaps the inserted block when another signature is picked", async () => {
  const box = await renderWithSignatures();

  await fireEvent.click(screen.getByRole("button", { name: "Signature" }));
  await fireEvent.click(screen.getByRole("button", { name: "Vocalio" }));

  expect(box).toHaveTextContent("Vocalio tím");
  expect(box).not.toHaveTextContent("— Dominik");
  // Picking closes the menu.
  expect(screen.queryByRole("button", { name: "None" })).toBeNull();
});

it("removes the signature when None is picked", async () => {
  const box = await renderWithSignatures();

  await fireEvent.click(screen.getByRole("button", { name: "Signature" }));
  await fireEvent.click(screen.getByRole("button", { name: "None" }));

  expect(box).not.toHaveTextContent("— Dominik");
});

it("follows the From account's default until touched by hand", async () => {
  const box = await renderWithSignatures();

  // Switching From swaps in the other account's default…
  await fireEvent.change(screen.getByLabelText("From"), {
    target: { value: "2" },
  });
  expect(box).toHaveTextContent("Vocalio tím");
  expect(box).not.toHaveTextContent("— Dominik");

  // …but once the user picked one explicitly, From changes leave it alone.
  await fireEvent.click(screen.getByRole("button", { name: "Signature" }));
  await fireEvent.click(screen.getByRole("button", { name: "Personal" }));
  await fireEvent.change(screen.getByLabelText("From"), {
    target: { value: "1" },
  });
  expect(box).toHaveTextContent("— Dominik");
  expect(box).not.toHaveTextContent("Vocalio tím");
});

it("suggests known contacts in To and turns the pick into a chip", async () => {
  vi.mocked(api.listContacts).mockResolvedValue([
    { name: "Ann Boe", email: "ann@example.com" },
    { name: "", email: "anton@example.sk" },
  ]);
  render(ComposeWindow);
  const to = (await screen.findByLabelText("To")) as HTMLInputElement;

  await fireEvent.input(to, { target: { value: "an" } });

  const option = await screen.findByRole("option", { name: /ann@example\.com/ });
  expect(api.listContacts).toHaveBeenCalledWith("an");
  await fireEvent.mouseDown(option);

  expect(
    screen.getByRole("button", { name: "Remove ann@example.com" }),
  ).toBeInTheDocument();
  // The input clears for the next recipient and the dropdown closes.
  expect(to.value).toBe("");
  await waitFor(() =>
    expect(
      screen.queryByRole("option", { name: /ann@example\.com/ }),
    ).not.toBeInTheDocument(),
  );
});

it("navigates suggestions with arrows and picks with Enter", async () => {
  vi.mocked(api.listContacts).mockResolvedValue([
    { name: "Ann Boe", email: "ann@example.com" },
    { name: "", email: "anton@example.sk" },
  ]);
  render(ComposeWindow);
  const to = (await screen.findByLabelText("To")) as HTMLInputElement;

  await fireEvent.input(to, { target: { value: "existing@x.sk, an" } });
  await screen.findByRole("option", { name: /ann@example\.com/ });

  await fireEvent.keyDown(to, { key: "ArrowDown" });
  await fireEvent.keyDown(to, { key: "Enter" });

  // Both the committed address and the pick are chips; Enter must not submit.
  expect(
    screen.getByRole("button", { name: "Remove existing@x.sk" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Remove anton@example.sk" }),
  ).toBeInTheDocument();
  expect(to.value).toBe("");
  expect(api.queueSend).not.toHaveBeenCalled();
});

it("queues every chip plus the still-typed address", async () => {
  await renderLoaded();

  await fireEvent.input(screen.getByLabelText("To"), {
    target: { value: "bob@example.com, carol@example.com," },
  });
  await fireEvent.input(screen.getByLabelText("To"), {
    target: { value: "dan@example.com" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.queueSend).toHaveBeenCalledWith(
      expect.objectContaining({
        to: "bob@example.com, carol@example.com, dan@example.com",
      }),
    ),
  );
});

it("closes the suggestions with Escape", async () => {
  vi.mocked(api.listContacts).mockResolvedValue([
    { name: "Ann Boe", email: "ann@example.com" },
  ]);
  render(ComposeWindow);
  const to = (await screen.findByLabelText("To")) as HTMLInputElement;

  to.value = "an";
  to.setSelectionRange(2, 2);
  await fireEvent.input(to);
  await screen.findByRole("option", { name: /ann@example\.com/ });

  await fireEvent.keyDown(to, { key: "Escape" });
  expect(
    screen.queryByRole("option", { name: /ann@example\.com/ }),
  ).not.toBeInTheDocument();
});

it("sends as the alias picked in the From menu", async () => {
  vi.mocked(api.listAliases).mockResolvedValueOnce(aliases);
  vi.mocked(api.takeComposeDraft).mockResolvedValueOnce({
    accountId: 2,
    to: "alice@example.com",
    subject: "Hi",
    body: "hello",
  });

  render(ComposeWindow);
  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("2"));

  await fireEvent.change(screen.getByLabelText("From"), {
    target: { value: "2:10" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));

  await waitFor(() =>
    expect(api.queueSend).toHaveBeenCalledWith(
      expect.objectContaining({ accountId: 2, aliasId: 10 }),
    ),
  );
});

it("preselects the alias a parked draft was composed with", async () => {
  vi.mocked(api.listAliases).mockResolvedValueOnce(aliases);
  vi.mocked(api.takeComposeDraft).mockResolvedValueOnce({
    accountId: 2,
    aliasId: 10,
    to: "alice@example.com",
    subject: "Hi",
    body: "hello",
  });

  render(ComposeWindow);

  await waitFor(() =>
    expect(screen.getByLabelText("From")).toHaveValue("2:10"),
  );
});

it("degrades a draft's deleted alias to the account's own address", async () => {
  // listAliases resolves [] — alias 10 no longer exists
  vi.mocked(api.takeComposeDraft).mockResolvedValueOnce({
    accountId: 2,
    aliasId: 10,
    to: "alice@example.com",
    subject: "Hi",
    body: "hello",
  });

  render(ComposeWindow);

  await waitFor(() => expect(screen.getByLabelText("From")).toHaveValue("2"));
});

it("starts a blank compose from the account's default identity", async () => {
  vi.mocked(api.listAccounts).mockResolvedValueOnce([
    { ...accounts[0], defaultAliasId: 20 },
    accounts[1],
  ]);
  vi.mocked(api.listAliases).mockResolvedValueOnce([
    { id: 20, accountId: 1, name: "", email: "info@example.com" },
  ]);

  render(ComposeWindow);

  await waitFor(() =>
    expect(screen.getByLabelText("From")).toHaveValue("1:20"),
  );
});
