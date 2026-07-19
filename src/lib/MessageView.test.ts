import { expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { MessageBody, MessageHeader } from "./types";

function body(partial: Partial<MessageBody>): MessageBody {
  return {
    html: null,
    text: null,
    quotedText: null,
    blockedImages: 0,
    canLoadRemote: false,
    attachments: [],
    auth: null,
    senderAnomaly: null,
    ...partial,
  };
}

vi.mock("./api", () => ({
  // Per-message re-render used only by the "Load Images" click.
  getMessageBody: vi.fn(async () => ({
    html: null,
    text: null,
    quotedText: null,
    blockedImages: 0,
    canLoadRemote: false,
    attachments: [],
    auth: null,
    senderAnomaly: null,
  })),
  saveAttachment: vi.fn(async () => {}),
  saveAllAttachments: vi.fn(async () => {}),
  // Default: the conversation is just the selected message (the view falls
  // back to it when the thread comes back empty).
  listThread: vi.fn(async () => []),
  // Bodies arrive as one bulk map, keyed by message id.
  threadBodies: vi.fn(async () => ({})),
  setMessageRead: vi.fn(async () => {}),
}));

import * as api from "./api";
import MessageView from "./MessageView.svelte";

const message: MessageHeader = {
  id: 1,
  accountId: 1,
  mailbox: "INBOX",
  from: "Alice <alice@example.com>",
  to: "me@example.com",
  cc: "",
  replyTo: "",
  bcc: "",
  messageId: "",
  references: "",
  threadCount: 1,
  threadUnread: false,
  subject: "Weekend plans",
  snippet: "Hey",
  date: "2026-07-07T09:15:00Z",
  read: false,
  hasAttachments: false,
};

function renderView(props: Record<string, unknown> = {}) {
  return render(MessageView, {
    props: {
      message,
      accountEmails: { 1: "me@example.com" },
      accountColors: {},
      onDraft: vi.fn(),
      ...props,
    },
  });
}

it("renders html bodies in a sandboxed iframe without script rights", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({
      html: "<!doctype html><html><body><p>hi there</p></body></html>",
      text: "hi there",
    }),
  });

  const { container } = renderView();

  const iframe = await waitFor(() => {
    const frame = container.querySelector("iframe");
    expect(frame).not.toBeNull();
    return frame as HTMLIFrameElement;
  });
  // SECURITY tripwire (hard rule): the sandbox must contain EXACTLY
  // allow-same-origin — it only lets the parent measure the document's
  // height. allow-scripts (or any other token) must NEVER appear here:
  // scripts stay blocked by the sandbox, the sanitizer and the srcdoc CSP.
  expect(iframe.getAttribute("sandbox")).toBe("allow-same-origin");
  expect(iframe.getAttribute("referrerpolicy")).toBe("no-referrer");
  expect(iframe.getAttribute("srcdoc")).toContain("<p>hi there</p>");
});

it("renders text-only bodies as escaped text without an iframe", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ html: null, text: "plain <b>not html</b>" }),
  });

  const { container } = renderView();

  expect(await screen.findByText("plain <b>not html</b>")).toBeInTheDocument();
  expect(container.querySelector("iframe")).toBeNull();
});

it("shows an error when the bulk body fetch fails", async () => {
  vi.mocked(api.threadBodies).mockRejectedValueOnce("imap error: gone");

  renderView();

  expect(await screen.findByText("imap error: gone")).toBeInTheDocument();
});

it("shows the empty state and fetches nothing without a message", () => {
  vi.mocked(api.listThread).mockClear();
  vi.mocked(api.threadBodies).mockClear();

  renderView({ message: null });

  expect(screen.getByText("Select a message")).toBeInTheDocument();
  expect(api.listThread).not.toHaveBeenCalled();
  expect(api.threadBodies).not.toHaveBeenCalled();
});

it("offers to load remote images and re-renders with them", async () => {
  vi.mocked(api.getMessageBody).mockClear();
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({
      html: "<!doctype html><html><body>no pics</body></html>",
      blockedImages: 2,
      canLoadRemote: true,
    }),
  });
  vi.mocked(api.getMessageBody).mockResolvedValueOnce(
    body({ html: "<!doctype html><html><body>with pics</body></html>" }),
  );

  renderView();

  await fireEvent.click(
    await screen.findByRole("button", { name: "Load Images" }),
  );

  expect(api.getMessageBody).toHaveBeenLastCalledWith(1, true);
  await waitFor(() => {
    expect(
      screen.queryByRole("button", { name: "Load Images" }),
    ).not.toBeInTheDocument();
  });
});

it("shows recipients like the prototype: To always, Cc and Bcc when present", async () => {
  const { rerender } = renderView();

  expect(await screen.findByText("To: me@example.com")).toBeInTheDocument();
  expect(screen.queryByText(/Cc:/)).not.toBeInTheDocument();
  expect(screen.queryByText(/Bcc:/)).not.toBeInTheDocument();

  await rerender({
    message: {
      ...message,
      cc: "carol@example.com",
      bcc: "archiv@example.com",
      // Reply-To is a sending concern — the prototype never displays it.
      replyTo: "Support <support@example.com>",
    },
  });

  expect(
    screen.getByText("Cc: carol@example.com · Bcc: archiv@example.com"),
  ).toBeInTheDocument();
  expect(screen.queryByText(/Reply-To:/)).not.toBeInTheDocument();
});

it("opens a collapsed message when its sender name is clicked", async () => {
  vi.mocked(api.listThread).mockResolvedValueOnce(conversationForName());
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ text: "the original in full" }),
    2: body({ text: "the answer in full" }),
  });

  renderView({ message: { ...message, id: 2 } });
  await screen.findByText("the answer in full");

  // The prototype's name click on a collapsed row opens the card AND
  // reveals the address.
  await fireEvent.click(screen.getByRole("button", { name: "Old Sender" }));

  expect(await screen.findByText("the original in full")).toBeInTheDocument();
  expect(
    screen.getByText("From: old@example.com · To: me@example.com"),
  ).toBeInTheDocument();
});

function conversationForName(): MessageHeader[] {
  return [
    {
      ...message,
      id: 1,
      from: "Old Sender <old@example.com>",
      snippet: "the original",
      read: true,
    hasAttachments: false,
    },
    { ...message, id: 2, snippet: "the answer", read: true },
  ];
}

it("reveals the sender address on a name click without collapsing", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ text: "the full body" }),
  });
  renderView();
  await screen.findByText("the full body");

  await fireEvent.click(screen.getByRole("button", { name: "Alice" }));

  expect(
    screen.getByText("From: alice@example.com · To: me@example.com"),
  ).toBeInTheDocument();
  // The card stayed open — the name click must not toggle the card.
  expect(screen.getByText("the full body")).toBeInTheDocument();
});

const attachments = [
  {
    id: 11,
    messageId: 1,
    partIndex: 0,
    filename: "report.pdf",
    contentType: "application/pdf",
    size: 1200,
  },
  {
    id: 12,
    messageId: 1,
    partIndex: 2,
    filename: "photo.jpg",
    contentType: "image/jpeg",
    size: 45_600,
  },
];

it("lists attachment chips with type tiles and saves one on click", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ text: "see files", attachments }),
  });

  renderView();

  const chip = await screen.findByRole("button", { name: /report\.pdf/ });
  expect(chip).toHaveTextContent("1.2 kB");
  expect(chip).toHaveTextContent("PDF");
  expect(screen.getByText("2 attachments")).toBeInTheDocument();

  await fireEvent.click(chip);
  expect(api.saveAttachment).toHaveBeenCalledWith(attachments[0]);
});

it("offers Save All only for multiple attachments", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ text: "see files", attachments }),
  });
  renderView();

  await fireEvent.click(await screen.findByRole("button", { name: "Save All" }));
  expect(api.saveAllAttachments).toHaveBeenCalledWith(1);
});

it("shows no attachment strip when a message has none", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ text: "plain" }),
  });
  renderView();
  await screen.findByText("plain");

  expect(
    screen.queryByRole("button", { name: "Save All" }),
  ).not.toBeInTheDocument();
});

it("folds quoted text history behind a toggle", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({
      text: "Uhradené. Ďakujem.",
      quotedText: "On Monday, Tomáš wrote:\n> pôvodná správa",
    }),
  });
  renderView();
  await screen.findByText("Uhradené. Ďakujem.");

  // The history is hidden until the ••• pill is clicked.
  expect(screen.queryByText(/pôvodná správa/)).not.toBeInTheDocument();
  await fireEvent.click(
    screen.getByRole("button", { name: "Show quoted text" }),
  );
  expect(screen.getByText(/pôvodná správa/)).toBeInTheDocument();
});

// ── Conversation view ──────────────────────────────────────────────────

const conversation: MessageHeader[] = [
  { ...message, id: 1, snippet: "the original", read: true },
  {
    ...message,
    id: 2,
    from: "Me <me@example.com>",
    mailbox: "Sent",
    snippet: "my reply",
    read: true,
    hasAttachments: false,
  },
  {
    ...message,
    id: 3,
    snippet: "their answer",
    read: false,
    hasAttachments: false,
  },
];

const conversationBodies = {
  1: body({ text: "the original in full" }),
  2: body({ text: "my reply in full" }),
  3: body({ text: "their answer in full" }),
};

it("opens the newest message and collapses older ones to preview rows", async () => {
  vi.mocked(api.getMessageBody).mockClear();
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.threadBodies).mockResolvedValueOnce(conversationBodies);

  renderView({ message: { ...message, id: 3 } });

  // The newest body is visible; the older two show snippet rows only.
  expect(await screen.findByText("their answer in full")).toBeInTheDocument();
  expect(screen.getByText("the original")).toBeInTheDocument();
  expect(screen.getByText("my reply")).toBeInTheDocument();
  expect(screen.queryByText("the original in full")).not.toBeInTheDocument();
  // Thread header: subject plus the message count.
  expect(
    screen.getByRole("heading", { name: "Weekend plans" }),
  ).toBeInTheDocument();
  expect(screen.getByText("3 messages")).toBeInTheDocument();
  // One bulk call fetched everything; no per-card body fetches.
  expect(api.threadBodies).toHaveBeenCalledWith(3);
  expect(api.getMessageBody).not.toHaveBeenCalled();
});

it("labels own messages with me and marks only the opened one read", async () => {
  vi.mocked(api.setMessageRead).mockClear();
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.threadBodies).mockResolvedValueOnce(conversationBodies);

  renderView({ message: { ...message, id: 3 } });
  await screen.findByText("their answer in full");

  expect(screen.getByText("me")).toBeInTheDocument();
  expect(api.setMessageRead).toHaveBeenCalledWith(3, true);
  expect(api.setMessageRead).toHaveBeenCalledTimes(1);
});

it("expands an older message alongside the newest and collapses it again", async () => {
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.threadBodies).mockResolvedValueOnce(conversationBodies);

  renderView({ message: { ...message, id: 3 } });
  await screen.findByText("their answer in full");

  // Click the collapsed row (its preview) — both messages are now open.
  await fireEvent.click(screen.getByText("the original"));
  expect(await screen.findByText("the original in full")).toBeInTheDocument();
  expect(screen.getByText("their answer in full")).toBeInTheDocument();

  // Click the open card's header (meta line) to fold it back to a preview.
  const metas = screen.getAllByText("To: me@example.com");
  await fireEvent.click(metas[0]);
  expect(screen.queryByText("the original in full")).not.toBeInTheDocument();
  expect(screen.getByText("the original")).toBeInTheDocument();
});

it("renders the newest message on top when the setting says so", async () => {
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.threadBodies).mockResolvedValueOnce(conversationBodies);

  renderView({ message: { ...message, id: 3 }, threadOrder: "newestFirst" });

  // The open newest card comes before the collapsed original in the DOM.
  const newest = await screen.findByText("their answer in full");
  const original = screen.getByText("the original");
  expect(
    newest.compareDocumentPosition(original) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

it("opens a reply draft for the message whose card action was clicked", async () => {
  const onDraft = vi.fn();
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.threadBodies).mockResolvedValueOnce(conversationBodies);

  renderView({ message: { ...message, id: 3 }, onDraft });
  await screen.findByText("their answer in full");

  await fireEvent.click(screen.getByRole("button", { name: "Reply to this message" }));
  expect(onDraft).toHaveBeenCalledWith(
    "reply",
    expect.objectContaining({ id: 3 }),
  );

  await fireEvent.click(screen.getByRole("button", { name: "Reply all to this message" }));
  expect(onDraft).toHaveBeenCalledWith(
    "reply-all",
    expect.objectContaining({ id: 3 }),
  );
});

it("warns when a familiar sender writes from an unusual address", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({
      text: "please pay this invoice",
      senderAnomaly: { name: "Alice", usualEmail: "alice@example.com" },
    }),
  });

  renderView();

  expect(
    await screen.findByText(/Alice does not usually use this email address/),
  ).toBeInTheDocument();
});

it("warns when authentication checks failed", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({
      text: "hello",
      auth: { spf: "pass", dkim: "fail", dmarc: "fail" },
    }),
  });

  renderView();

  // Only the failing checks are named, in SPF/DKIM/DMARC order.
  expect(
    await screen.findByText(/failed DKIM, DMARC authentication/),
  ).toBeInTheDocument();
});

it("stays silent on passing or unknown authentication", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({
      text: "hello",
      // softfail and none are common on legitimate mail — no warning.
      auth: { spf: "softfail", dkim: "pass", dmarc: null },
    }),
  });

  renderView();

  await screen.findByText("hello");
  expect(screen.queryByText(/authentication/)).not.toBeInTheDocument();
  expect(screen.queryByText(/does not usually use/)).not.toBeInTheDocument();
});
