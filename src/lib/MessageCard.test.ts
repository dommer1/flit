import { afterEach, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import MessageCard from "./MessageCard.svelte";
import type { MessageBody, MessageHeader } from "./types";

vi.mock("./api", () => ({
  getMessageBody: vi.fn(async () => ({})),
  saveAttachment: vi.fn(async () => {}),
  saveAllAttachments: vi.fn(async () => {}),
}));

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
  isDraft: false,
  threadHasDraft: false,
  subject: "Weekend plans",
  snippet: "Hey",
  date: "2026-07-07T09:15:00Z",
  read: false,
  hasAttachments: false,
};

const body: MessageBody = {
  html: "<!doctype html><html><body><p>hi there</p></body></html>",
  text: "hi there",
  quotedText: null,
  blockedImages: 0,
  canLoadRemote: false,
  attachments: [],
  auth: null,
  senderAnomaly: null,
};

function renderCard() {
  return render(MessageCard, {
    props: {
      message,
      body,
      loading: false,
      error: null,
      expanded: true,
      last: true,
      ownEmail: "me@example.com",
      accountColor: null,
      onToggle: vi.fn(),
      onDraft: vi.fn(),
    },
  });
}

/** Net number of `load` listeners a spied target currently holds. */
function loadListeners(add: ReturnType<typeof vi.spyOn>, remove: ReturnType<typeof vi.spyOn>) {
  const count = (spy: ReturnType<typeof vi.spyOn>) =>
    spy.mock.calls.filter(([type]) => type === "load").length;
  return count(add) - count(remove);
}

afterEach(() => {
  vi.restoreAllMocks();
});

it("hooks each body image once, however often the frame reloads", async () => {
  const { container, unmount } = renderCard();
  const frame = container.querySelector("iframe") as HTMLIFrameElement;
  // why: jsdom never renders srcdoc, so the frame keeps its about:blank
  // document — plant the images the sanitized mail would carry.
  const doc = frame.contentDocument as Document;
  const images = [doc.createElement("img"), doc.createElement("img")];
  const spies = images.map((image) => ({
    add: vi.spyOn(image, "addEventListener"),
    remove: vi.spyOn(image, "removeEventListener"),
  }));
  for (const image of images) doc.body.appendChild(image);

  // srcdoc load, then a "Load Images" swap: the hook runs on every load.
  await fireEvent.load(frame);
  await fireEvent.load(frame);
  for (const { add, remove } of spies) {
    expect(loadListeners(add, remove)).toBe(1);
  }

  unmount();
  for (const { add, remove } of spies) {
    expect(loadListeners(add, remove)).toBe(0);
  }
});

it("removes the frame's own load listener when the card is destroyed", () => {
  const add = vi.spyOn(HTMLIFrameElement.prototype, "addEventListener");
  const remove = vi.spyOn(HTMLIFrameElement.prototype, "removeEventListener");
  const { unmount } = renderCard();
  // Both frame actions (autoSize, interceptBodyLinks) hook the load event.
  expect(loadListeners(add, remove)).toBeGreaterThan(0);

  unmount();
  expect(loadListeners(add, remove)).toBe(0);
});
