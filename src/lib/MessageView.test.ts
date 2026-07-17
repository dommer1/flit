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

it("renders html bodies in a fully sandboxed iframe", async () => {
  vi.mocked(api.getMessageBody).mockResolvedValueOnce(
    body({ html: "<!doctype html><html><body><p>hi there</p></body></html>", text: "hi there" }),
  );

  const { container } = render(MessageView, { props: { message } });

  const iframe = await waitFor(() => {
    const frame = container.querySelector("iframe");
    expect(frame).not.toBeNull();
    return frame as HTMLIFrameElement;
  });
  // SECURITY tripwire (hard rule): the sandbox attribute must exist and be
  // EMPTY — every restriction on, JavaScript disabled, opaque origin.
  expect(iframe.getAttribute("sandbox")).toBe("");
  expect(iframe.getAttribute("referrerpolicy")).toBe("no-referrer");
  expect(iframe.getAttribute("srcdoc")).toContain("<p>hi there</p>");
});

it("renders text-only bodies as escaped text without an iframe", async () => {
  vi.mocked(api.getMessageBody).mockResolvedValueOnce(
    body({ html: null, text: "plain <b>not html</b>" }),
  );

  const { container } = render(MessageView, { props: { message } });

  expect(await screen.findByText("plain <b>not html</b>")).toBeInTheDocument();
  expect(container.querySelector("iframe")).toBeNull();
});

it("shows an error when the body fetch fails", async () => {
  vi.mocked(api.getMessageBody).mockRejectedValueOnce("imap error: gone");

  render(MessageView, { props: { message } });

  expect(await screen.findByText("imap error: gone")).toBeInTheDocument();
});

it("shows the empty state and fetches nothing without a message", () => {
  vi.mocked(api.getMessageBody).mockClear();
  vi.mocked(api.listThread).mockClear();

  render(MessageView, { props: { message: null } });

  expect(screen.getByText("Select a message")).toBeInTheDocument();
  expect(api.getMessageBody).not.toHaveBeenCalled();
  expect(api.listThread).not.toHaveBeenCalled();
});

it("offers to load remote images and re-renders with them", async () => {
  vi.mocked(api.getMessageBody).mockClear();
  vi.mocked(api.getMessageBody)
    .mockResolvedValueOnce(
      body({
        html: "<!doctype html><html><body>no pics</body></html>",
        blockedImages: 2,
        canLoadRemote: true,
      }),
    )
    .mockResolvedValueOnce(
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
  vi.mocked(api.getMessageBody).mockResolvedValueOnce(
    body({ html: "<!doctype html><html><body><p>clean</p></body></html>" }),
  );

  const { container } = render(MessageView, { props: { message } });

  await waitFor(() => expect(container.querySelector("iframe")).not.toBeNull());
  expect(
    screen.queryByRole("button", { name: "Load Images" }),
  ).not.toBeInTheDocument();
});

it("shows the recipient line, hiding Cc and Reply-To when empty", async () => {
  const { rerender } = render(MessageView, { props: { message } });

  // From + To always; Cc/Reply-To only when the message carries them.
  expect(await screen.findByText("Alice <alice@example.com>")).toBeInTheDocument();
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
  vi.mocked(api.getMessageBody).mockResolvedValueOnce(
    body({ text: "see files", attachments }),
  );

  render(MessageView, { props: { message } });

  const chip = await screen.findByRole("button", { name: /report\.pdf/ });
  expect(chip).toHaveTextContent("1.2 kB");
  expect(screen.getByRole("button", { name: /photo\.jpg/ })).toBeInTheDocument();

  await fireEvent.click(chip);
  expect(api.saveAttachment).toHaveBeenCalledWith(attachments[0]);
});

it("offers Save All only for multiple attachments", async () => {
  vi.mocked(api.getMessageBody).mockResolvedValueOnce(
    body({ text: "see files", attachments }),
  );
  render(MessageView, { props: { message } });

  await fireEvent.click(
    await screen.findByRole("button", { name: "Save All" }),
  );
  expect(api.saveAllAttachments).toHaveBeenCalledWith(1);
});

it("shows no attachment strip when a message has none", async () => {
  vi.mocked(api.getMessageBody).mockResolvedValueOnce(
    body({ text: "plain" }),
  );
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

it("renders older messages collapsed and opens the newest", async () => {
  vi.mocked(api.getMessageBody).mockClear();
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.getMessageBody).mockResolvedValueOnce(
    body({ text: "their answer in full" }),
  );

  render(MessageView, { props: { message: { ...message, id: 3 } } });

  // The newest message is open with its body…
  expect(await screen.findByText("their answer in full")).toBeInTheDocument();
  // …the older two are collapsed rows showing their snippets.
  expect(screen.getByText("the original")).toBeInTheDocument();
  expect(screen.getByText("my reply")).toBeInTheDocument();
  // Only the open card fetched a body.
  expect(api.getMessageBody).toHaveBeenCalledTimes(1);
  expect(api.getMessageBody).toHaveBeenCalledWith(3);
});

it("marks the opened unread message read", async () => {
  vi.mocked(api.setMessageRead).mockClear();
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);

  render(MessageView, { props: { message: { ...message, id: 3 } } });

  await waitFor(() => {
    expect(api.setMessageRead).toHaveBeenCalledWith(3, true);
  });
});

it("expands a collapsed message on click and fetches its body", async () => {
  vi.mocked(api.getMessageBody).mockClear();
  vi.mocked(api.listThread).mockResolvedValueOnce(conversation);
  vi.mocked(api.getMessageBody)
    .mockResolvedValueOnce(body({ text: "their answer in full" }))
    .mockResolvedValueOnce(body({ text: "the original in full" }));

  render(MessageView, { props: { message: { ...message, id: 3 } } });
  await screen.findByText("their answer in full");

  await fireEvent.click(
    screen.getByRole("button", { name: /the original/ }),
  );

  expect(await screen.findByText("the original in full")).toBeInTheDocument();
  expect(api.getMessageBody).toHaveBeenLastCalledWith(1);
  // The accordion collapsed the previously open message back to a row.
  expect(screen.getByText("their answer")).toBeInTheDocument();
});

