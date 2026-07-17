import { expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { MessageBody, MessageHeader } from "./types";

function body(partial: Partial<MessageBody>): MessageBody {
  return {
    html: null,
    text: null,
    blockedImages: 0,
    canLoadRemote: false,
    attachments: [],
    ...partial,
  };
}

vi.mock("./api", () => ({
  // Per-message re-render used only by the "Load Images" click.
  getMessageBody: vi.fn(async () => ({
    html: null,
    text: null,
    blockedImages: 0,
    canLoadRemote: false,
    attachments: [],
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
  messageId: "",
  references: "",
  threadCount: 1,
  threadUnread: false,
  subject: "Weekend plans",
  snippet: "Hey",
  date: "2026-07-07T09:15:00Z",
  read: false,
};

it("renders html bodies in a sandboxed iframe without script rights", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({
      html: "<!doctype html><html><body><p>hi there</p></body></html>",
      text: "hi there",
    }),
  });

  const { container } = render(MessageView, { props: { message } });

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

  const { container } = render(MessageView, { props: { message } });

  expect(await screen.findByText("plain <b>not html</b>")).toBeInTheDocument();
  expect(container.querySelector("iframe")).toBeNull();
});

it("shows an error when the bulk body fetch fails", async () => {
  vi.mocked(api.threadBodies).mockRejectedValueOnce("imap error: gone");

  render(MessageView, { props: { message } });

  expect(await screen.findByText("imap error: gone")).toBeInTheDocument();
});

it("shows the empty state and fetches nothing without a message", () => {
  vi.mocked(api.listThread).mockClear();
  vi.mocked(api.threadBodies).mockClear();

  render(MessageView, { props: { message: null } });

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

  render(MessageView, { props: { message } });

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

it("shows no banner when nothing was blocked", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ html: "<!doctype html><html><body><p>clean</p></body></html>" }),
  });

  const { container } = render(MessageView, { props: { message } });

  await waitFor(() => expect(container.querySelector("iframe")).not.toBeNull());
  expect(
    screen.queryByRole("button", { name: "Load Images" }),
  ).not.toBeInTheDocument();
});

it("shows the recipient line, hiding Cc and Reply-To when empty", async () => {
  const { rerender } = render(MessageView, { props: { message } });

  // From + To always; Cc/Reply-To only when the message carries them.
  expect(
    await screen.findByText("Alice <alice@example.com>"),
  ).toBeInTheDocument();
  expect(screen.getByText("To:")).toBeInTheDocument();
  expect(screen.queryByText("Cc:")).not.toBeInTheDocument();
  expect(screen.queryByText("Reply-To:")).not.toBeInTheDocument();

  await rerender({
    message: {
      ...message,
      cc: "carol@example.com",
      replyTo: "Support <support@example.com>",
    },
  });

  expect(screen.getByText("Cc:")).toBeInTheDocument();
  expect(screen.getByText("carol@example.com")).toBeInTheDocument();
  expect(screen.getByText("Reply-To:")).toBeInTheDocument();
  expect(
    screen.getByText("Support <support@example.com>"),
  ).toBeInTheDocument();
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

it("lists attachments and saves one on click", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ text: "see files", attachments }),
  });

  render(MessageView, { props: { message } });

  const chip = await screen.findByRole("button", { name: /report\.pdf/ });
  expect(chip).toHaveTextContent("1.2 kB");
  expect(
    screen.getByRole("button", { name: /photo\.jpg/ }),
  ).toBeInTheDocument();

  await fireEvent.click(chip);
  expect(api.saveAttachment).toHaveBeenCalledWith(attachments[0]);
});

it("offers Save All only for multiple attachments", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ text: "see files", attachments }),
  });
  render(MessageView, { props: { message } });

  await fireEvent.click(await screen.findByRole("button", { name: "Save All" }));
  expect(api.saveAllAttachments).toHaveBeenCalledWith(1);
});

it("shows no attachment strip when a message has none", async () => {
  vi.mocked(api.threadBodies).mockResolvedValueOnce({
    1: body({ text: "plain" }),
  });
  render(MessageView, { props: { message } });
  await screen.findByText("plain");

  expect(
    screen.queryByRole("button", { name: "Save All" }),
  ).not.toBeInTheDocument();
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
  },
  {
    ...message,
    id: 3,
    snippet: "their answer",
    read: false,
  },
];

const conversationBodies = {
  1: body({ text: "the original in full" }),
  2: body({ text: "my reply in full" }),
  3: body({ text: "their answer in full" }),
};

it("opens every message of the conversation at once", async () => {
  vi.mocked(api.getMessageBody).mockClear();
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.threadBodies).mockResolvedValueOnce(conversationBodies);

  render(MessageView, { props: { message: { ...message, id: 3 } } });

  // All three bodies are visible — no card starts collapsed.
  expect(await screen.findByText("the original in full")).toBeInTheDocument();
  expect(screen.getByText("my reply in full")).toBeInTheDocument();
  expect(screen.getByText("their answer in full")).toBeInTheDocument();
  // One bulk call fetched everything; no per-card body fetches.
  expect(api.threadBodies).toHaveBeenCalledWith(3);
  expect(api.getMessageBody).not.toHaveBeenCalled();
});

it("marks every unread message of the conversation read", async () => {
  vi.mocked(api.setMessageRead).mockClear();
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.threadBodies).mockResolvedValueOnce(conversationBodies);

  render(MessageView, { props: { message: { ...message, id: 3 } } });

  await waitFor(() => {
    expect(api.setMessageRead).toHaveBeenCalledWith(3, true);
  });
  // The two already-read messages are left alone.
  expect(api.setMessageRead).toHaveBeenCalledTimes(1);
});

it("folds a message via its chevron and reopens it from the row", async () => {
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.threadBodies).mockResolvedValueOnce(conversationBodies);

  render(MessageView, { props: { message: { ...message, id: 3 } } });
  await screen.findByText("the original in full");

  const folds = screen.getAllByRole("button", { name: "Collapse message" });
  await fireEvent.click(folds[0]);

  // The folded card shows its snippet row instead of the body…
  expect(screen.queryByText("the original in full")).not.toBeInTheDocument();
  const row = screen.getByRole("button", { name: /the original/ });

  // …and clicking the row opens it again, with the body still in hand.
  await fireEvent.click(row);
  expect(await screen.findByText("the original in full")).toBeInTheDocument();
});
