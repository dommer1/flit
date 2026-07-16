import { expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { MessageBody, MessageHeader } from "./types";

function body(partial: Partial<MessageBody>): MessageBody {
  return {
    html: null,
    text: null,
    blockedImages: 0,
    canLoadRemote: false,
    ...partial,
  };
}

vi.mock("./api", () => ({
  getMessageBody: vi.fn(async () => ({
    html: null,
    text: null,
    blockedImages: 0,
    canLoadRemote: false,
  })),
}));

import * as api from "./api";
import MessageView from "./MessageView.svelte";

const message: MessageHeader = {
  id: 1,
  accountId: 1,
  from: "Alice <alice@example.com>",
  to: "me@example.com",
  cc: "",
  replyTo: "",
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

  render(MessageView, { props: { message: null } });

  expect(screen.getByText("Select a message")).toBeInTheDocument();
  expect(api.getMessageBody).not.toHaveBeenCalled();
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

it("shows the recipient fields, hiding Cc and Reply-To when empty", async () => {
  const { rerender } = render(MessageView, { props: { message } });

  // From + To always; Cc/Reply-To only when the message carries them.
  expect(await screen.findByText("Alice <alice@example.com>")).toBeInTheDocument();
  expect(screen.getByText("To")).toBeInTheDocument();
  expect(screen.queryByText("Cc")).not.toBeInTheDocument();
  expect(screen.queryByText("Reply-To")).not.toBeInTheDocument();

  await rerender({
    message: {
      ...message,
      cc: "carol@example.com",
      replyTo: "Support <support@example.com>",
    },
  });

  expect(screen.getByText("Cc")).toBeInTheDocument();
  expect(screen.getByText("carol@example.com")).toBeInTheDocument();
  expect(screen.getByText("Reply-To")).toBeInTheDocument();
  expect(
    screen.getByText("Support <support@example.com>"),
  ).toBeInTheDocument();
});

it("toggles read state via the Mark Read / Mark Unread button", async () => {
  const onSetRead = vi.fn();
  const { rerender } = render(MessageView, { props: { message, onSetRead } });

  // The fixture is unread, so the button offers to mark it read.
  await fireEvent.click(screen.getByRole("button", { name: "Mark Read" }));
  expect(onSetRead).toHaveBeenCalledWith(1, true);

  await rerender({ message: { ...message, read: true }, onSetRead });
  await fireEvent.click(screen.getByRole("button", { name: "Mark Unread" }));
  expect(onSetRead).toHaveBeenCalledWith(1, false);
});

it("moves the message to trash via the Trash button", async () => {
  const onTrash = vi.fn();
  render(MessageView, { props: { message, onTrash } });

  await fireEvent.click(screen.getByRole("button", { name: "Trash" }));
  expect(onTrash).toHaveBeenCalledWith(1);
});

it("offers reply, reply all and forward with the loaded text", async () => {
  vi.mocked(api.getMessageBody).mockResolvedValueOnce(
    body({ html: null, text: "hi there" }),
  );
  const onDraft = vi.fn();

  render(MessageView, { props: { message, onDraft } });
  await screen.findByText("hi there");

  await fireEvent.click(screen.getByRole("button", { name: "Reply" }));
  expect(onDraft).toHaveBeenCalledWith("reply", message, "hi there");

  await fireEvent.click(screen.getByRole("button", { name: "Reply All" }));
  expect(onDraft).toHaveBeenCalledWith("reply-all", message, "hi there");

  await fireEvent.click(screen.getByRole("button", { name: "Forward" }));
  expect(onDraft).toHaveBeenCalledWith("forward", message, "hi there");
});
