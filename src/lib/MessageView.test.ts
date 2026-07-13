import { expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { MessageHeader } from "./types";

vi.mock("./api", () => ({
  getMessageBody: vi.fn(async () => ({ html: null, text: null })),
}));

import * as api from "./api";
import MessageView from "./MessageView.svelte";

const message: MessageHeader = {
  id: 1,
  accountId: 1,
  from: "Alice <alice@example.com>",
  to: "me@example.com",
  cc: "",
  subject: "Weekend plans",
  snippet: "Hey",
  date: "2026-07-07T09:15:00Z",
  read: false,
};

it("renders html bodies in a fully sandboxed iframe", async () => {
  vi.mocked(api.getMessageBody).mockResolvedValueOnce({
    html: "<!doctype html><html><body><p>hi there</p></body></html>",
    text: "hi there",
  });

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
  vi.mocked(api.getMessageBody).mockResolvedValueOnce({
    html: null,
    text: "plain <b>not html</b>",
  });

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

it("offers reply, reply all and forward with the loaded text", async () => {
  vi.mocked(api.getMessageBody).mockResolvedValueOnce({
    html: null,
    text: "hi there",
  });
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
