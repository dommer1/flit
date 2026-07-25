import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import type {
  Account,
  Alias,
  AttachmentInfo,
  Contact,
  DateTimeFormat,
  Mailbox,
  MessageAttachment,
  MessageBody,
  MessageQuote,
  MessageHeader,
  NewAccount,
  NotificationSettings,
  OutgoingMessage,
  RemoteImagePolicy,
  ScheduledMessage,
  SendEvent,
  Signature,
  SwipeActions,
  ThreadOrder,
  ViewStatus,
} from "./types";

export function listAccounts(): Promise<Account[]> {
  return invoke<Account[]>("list_accounts");
}

export function addAccount(
  account: NewAccount,
  password: string,
): Promise<Account> {
  return invoke<Account>("add_account", { account, password });
}

export function deleteAccount(id: number): Promise<void> {
  return invoke<void>("delete_account", { id });
}

/** Set (or clear, with `null`) an account's accent color. */
export function setAccountColor(
  id: number,
  color: string | null,
): Promise<void> {
  return invoke<void>("set_account_color", { id, color });
}

/** Verify & Save: prove IMAP + SMTP credentials before saving the account. */
export function testConnection(
  account: NewAccount,
  password: string,
): Promise<void> {
  return invoke<void>("test_connection", { account, password });
}

/** Native confirmation sheet; resolves to true when the user confirms. */
export function confirmAccountDeletion(account: Account): Promise<boolean> {
  return ask(
    `Delete the account “${account.name}” (${account.email})?\n\n` +
      "Its password will be removed from the keychain. " +
      "This cannot be undone.",
    { title: "Delete Account", kind: "warning" },
  );
}

/** Compose autocomplete: locally harvested contacts matching the query. */
export function listContacts(query: string): Promise<Contact[]> {
  return invoke<Contact[]>("list_contacts", { query });
}

/** Folders of one account in sidebar order (roles first, then customs). */
export function listMailboxes(accountId: number): Promise<Mailbox[]> {
  return invoke<Mailbox[]>("list_mailboxes", { accountId });
}

/** `accountId: null` = the given mailbox across all accounts. `limit` caps
 * the rows (the list reveals more on scroll); `null` = all. */
export function listMessages(
  accountId: number | null,
  mailbox = "INBOX",
  limit: number | null = null,
): Promise<MessageHeader[]> {
  return invoke<MessageHeader[]>("list_messages", {
    accountId,
    mailbox,
    limit,
  });
}

/** Counts for the current list view: header totals + backfill progress. */
export function viewStatus(
  accountId: number | null,
  mailbox = "INBOX",
): Promise<ViewStatus> {
  return invoke<ViewStatus>("view_status", { accountId, mailbox });
}

/** The full conversation of one message (all folders except trash, junk
 * and drafts), oldest first — what the conversation view renders. */
export function listThread(messageId: number): Promise<MessageHeader[]> {
  return invoke<MessageHeader[]>("list_thread", { messageId });
}

/** Bodies for the whole conversation, keyed by message id. Missing bodies
 * are fetched over a single server connection instead of one per message. */
export function threadBodies(
  messageId: number,
): Promise<Record<number, MessageBody>> {
  return invoke<Record<number, MessageBody>>("thread_bodies", { messageId });
}

/** Quote material for a reply to this message — served from the same body
 * cache as the viewer; a miss fetches the body first. */
export function getMessageQuote(messageId: number): Promise<MessageQuote> {
  return invoke<MessageQuote>("get_message_quote", { messageId });
}

/**
 * Search the local cache with a gmail-style query
 * (`from:x is:unread faktúra`). `accountId: null` searches all accounts.
 */
export function searchMessages(
  accountId: number | null,
  query: string,
): Promise<MessageHeader[]> {
  return invoke<MessageHeader[]>("search_messages", { accountId, query });
}

/**
 * Body from cache, lazily fetched from the server on first open.
 * `loadRemote` is the per-message "Load Images" click; the backend honors
 * it only under the "ask" policy.
 */
export function getMessageBody(
  messageId: number,
  loadRemote = false,
): Promise<MessageBody> {
  return invoke<MessageBody>("get_message_body", { messageId, loadRemote });
}

/** Fire-and-forget sync of one account: folder list + all folders' headers. */
export function syncAccount(accountId: number): Promise<void> {
  return invoke<void>("sync_account", { accountId });
}

/**
 * Mark a message read/unread. Updates the local cache and fires
 * messages-changed at once; the `\Seen` flag is pushed to the server in the
 * background by the backend.
 */
export function setMessageRead(
  messageId: number,
  read: boolean,
): Promise<void> {
  return invoke<void>("set_message_read", { messageId, read });
}

/**
 * Move a message to the account's Trash folder on the server, then drop it
 * from the local cache. Rejects (and keeps the row) if there is no trash
 * folder or the server move fails.
 */
export function moveToTrash(messageId: number): Promise<void> {
  return invoke<void>("move_to_trash", { messageId });
}

/** Trash the whole conversation as shown in the message's folder. */
export function trashThread(messageId: number): Promise<void> {
  return invoke<void>("trash_thread", { messageId });
}

/** Archive the whole conversation as shown in the message's folder. */
export function archiveThread(messageId: number): Promise<void> {
  return invoke<void>("archive_thread", { messageId });
}

/** Move the whole conversation (as shown in the message's folder). */
export function moveThread(messageId: number, mailbox: string): Promise<void> {
  return invoke<void>("move_thread", { messageId, mailbox });
}

/**
 * Archive a message: move it to the account's Archive folder on the server,
 * then drop it from the local cache. Rejects (and keeps the row) if there is
 * no archive folder or the server move fails.
 */
export function archiveMessage(messageId: number): Promise<void> {
  return invoke<void>("archive_message", { messageId });
}

/**
 * Move a message to a folder of its account on the server, then drop it from
 * the local cache. Rejects (and keeps the row) if the folder is unknown or
 * the server move fails.
 */
export function moveMessage(
  messageId: number,
  mailbox: string,
): Promise<void> {
  return invoke<void>("move_message", { messageId, mailbox });
}

/**
 * Save one received attachment: a native save dialog picks the destination,
 * then the backend re-fetches the message from the server and writes the
 * extracted part there. Resolves without saving when the dialog is
 * cancelled.
 */
export async function saveAttachment(
  attachment: MessageAttachment,
): Promise<void> {
  const path = await save({ defaultPath: attachment.filename });
  if (path === null) return;
  await invoke<void>("save_attachment", { attachmentId: attachment.id, path });
}

/**
 * Save every attachment of a message into a folder picked in a native
 * dialog; one server fetch for all of them. Resolves without saving when
 * the dialog is cancelled.
 */
export async function saveAllAttachments(messageId: number): Promise<void> {
  const dir = await open({ directory: true, title: "Save Attachments" });
  if (dir === null) return;
  await invoke<void>("save_all_attachments", { messageId, dir });
}

/**
 * Stat dropped file paths into chip metadata (name + size). Duplicates and
 * directories drop out; rejects if a file is missing or unreadable.
 */
export function inspectAttachments(
  paths: string[],
): Promise<AttachmentInfo[]> {
  return invoke<AttachmentInfo[]>("inspect_attachments", { paths });
}

/**
 * Best-effort thumbnail for one attachment as a `data:` URI; null when the
 * file is not a previewable image (png/jpeg/webp).
 */
export function attachmentPreview(path: string): Promise<string | null> {
  return invoke<string | null>("attachment_preview", { path });
}

/**
 * Native file drag & drop over this window. Tauri intercepts the OS drag
 * before the DOM sees it (HTML5 drop events never carry paths in a webview),
 * so dropped file paths arrive through this listener instead.
 */
export function onFileDrop(callbacks: {
  onHover: (hovering: boolean) => void;
  onDrop: (paths: string[]) => void;
}): Promise<UnlistenFn> {
  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop") {
      callbacks.onHover(false);
      callbacks.onDrop(event.payload.paths);
    } else if (event.payload.type === "leave") {
      callbacks.onHover(false);
    } else {
      // "enter" and "over" both mean a drag is above the window.
      callbacks.onHover(true);
    }
  });
}

/**
 * Queue a message for sending after its undo window. Resolves as soon as
 * the message is validated and queued; progress arrives via onSendQueued /
 * onSendFinished / onSendUndone.
 */
export function queueSend(message: OutgoingMessage): Promise<void> {
  return invoke<void>("queue_send", { message });
}

/** Cancel a queued send — the draft reopens in a new compose window. */
export function undoSend(id: number): Promise<void> {
  return invoke<void>("undo_send", { id });
}

/**
 * Save a draft into its account's Drafts folder on the server (visible to
 * webmail and other clients). Returns the Message-ID of the saved version;
 * pass it back as `previousDraftId` on the next save so the server copy is
 * replaced instead of duplicated.
 */
export function saveDraft(
  message: OutgoingMessage,
  previousDraftId: string | null,
): Promise<string> {
  return invoke<string>("save_draft", { message, previousDraftId });
}

/**
 * "Don't Save" on compose close: delete the autosaved draft version from
 * the account's server Drafts folder.
 */
export function discardDraft(
  accountId: number,
  draftMessageId: string,
): Promise<void> {
  return invoke<void>("discard_draft", { accountId, draftMessageId });
}

/**
 * Park a message until `scheduledAt` (unix seconds) — "Send Later". The
 * message is validated now; delivery happens in the backend scheduler.
 */
export function scheduleSend(
  message: OutgoingMessage,
  scheduledAt: number,
): Promise<void> {
  return invoke<void>("schedule_send", { message, scheduledAt });
}

/** Every parked send-later message, soonest first (pending and missed). */
export function listScheduled(): Promise<ScheduledMessage[]> {
  return invoke<ScheduledMessage[]>("list_scheduled");
}

/** Deliver one parked message immediately (the catch-up dialog's confirm). */
export function sendScheduledNow(id: number): Promise<void> {
  return invoke<void>("send_scheduled_now", { id });
}

/** Unschedule one parked message — it reopens as a compose window. */
export function cancelScheduled(id: number): Promise<void> {
  return invoke<void>("cancel_scheduled", { id });
}

/** The set of parked messages changed (scheduled, sent, or cancelled). */
export function onScheduledChanged(callback: () => void): Promise<UnlistenFn> {
  return listen<void>("scheduled-changed", () => callback());
}

/** Some scheduled messages were missed and wait for the user's decision. */
export function onScheduledMissed(callback: () => void): Promise<UnlistenFn> {
  return listen<void>("scheduled-missed", () => callback());
}

/** A message entered its undo window. */
export function onSendQueued(
  callback: (event: SendEvent) => void,
): Promise<UnlistenFn> {
  return listen<SendEvent>("send-queued", (e) => callback(e.payload));
}

/** A queued message finished: delivered when `error` is null, failed otherwise. */
export function onSendFinished(
  callback: (event: SendEvent) => void,
): Promise<UnlistenFn> {
  return listen<SendEvent>("send-finished", (e) => callback(e.payload));
}

/** A queued message was undone and handed back to a compose window. */
export function onSendUndone(
  callback: (event: SendEvent) => void,
): Promise<UnlistenFn> {
  return listen<SendEvent>("send-undone", (e) => callback(e.payload));
}

/** Open a native compose window seeded with the draft. */
export function openCompose(draft: OutgoingMessage): Promise<void> {
  return invoke<void>("open_compose", { draft });
}

/**
 * Reopen a message from a Drafts folder for editing: the backend fetches
 * and parses the raw draft, then opens a compose window that keeps
 * replacing this server version on every save.
 */
export function openDraft(messageId: number): Promise<void> {
  return invoke<void>("open_draft", { messageId });
}

/** One-shot pickup of this compose window's draft; null after a reload. */
export function takeComposeDraft(): Promise<OutgoingMessage | null> {
  return invoke<OutgoingMessage | null>("take_compose_draft");
}

/** Close the compose window this call comes from. */
export function closeCompose(): Promise<void> {
  return invoke<void>("close_compose");
}

/** Fires whenever the message cache changes (any account, any window). */
export function onMessagesChanged(callback: () => void): Promise<UnlistenFn> {
  return listen("messages-changed", callback);
}

/** Current remote-image policy ("ask" until the user picks otherwise). */
export function getRemoteImagePolicy(): Promise<RemoteImagePolicy> {
  return invoke<RemoteImagePolicy>("get_remote_image_policy");
}

export function setRemoteImagePolicy(
  policy: RemoteImagePolicy,
): Promise<void> {
  return invoke<void>("set_remote_image_policy", { policy });
}

/** Global notification defaults + background check cadence. */
export function getNotificationSettings(): Promise<NotificationSettings> {
  return invoke<NotificationSettings>("get_notification_settings");
}

export function setNotificationSettings(
  settings: NotificationSettings,
): Promise<void> {
  return invoke<void>("set_notification_settings", { settings });
}

/** Preview a notification sound pick; "default"/"none" play nothing. */
export function previewNotificationSound(sound: string): Promise<void> {
  return invoke<void>("preview_notification_sound", { sound });
}

/** Per-account notification overrides; null = inherit the global default. */
export function setAccountNotifications(
  id: number,
  enabled: boolean | null,
  sound: string | null,
): Promise<void> {
  return invoke<void>("set_account_notifications", { id, enabled, sound });
}

/** Configured swipe actions (left archives, right toggles read until
 * the user picks otherwise). */
export function getSwipeActions(): Promise<SwipeActions> {
  return invoke<SwipeActions>("get_swipe_actions");
}

export function setSwipeActions(actions: SwipeActions): Promise<void> {
  return invoke<void>("set_swipe_actions", { actions });
}

/** Conversation-view order (newest at the bottom until changed). */
export function getThreadOrder(): Promise<ThreadOrder> {
  return invoke<ThreadOrder>("get_thread_order");
}

export function setThreadOrder(order: ThreadOrder): Promise<void> {
  return invoke<void>("set_thread_order", { order });
}

/** How the UI writes dates and times (the OS locale until changed). */
export function getDateTimeFormat(): Promise<DateTimeFormat> {
  return invoke<DateTimeFormat>("get_date_time_format");
}

export function setDateTimeFormat(format: DateTimeFormat): Promise<void> {
  return invoke<void>("set_date_time_format", { format });
}

/** Fires whenever any window changes an app-wide setting. */
export function onSettingsChanged(callback: () => void): Promise<UnlistenFn> {
  return listen("settings-changed", callback);
}

export function openSettings(): Promise<void> {
  return invoke<void>("open_settings");
}

export function closeSettings(): Promise<void> {
  return invoke<void>("close_settings");
}

/** Fires whenever any window mutates the account list. */
export function onAccountsChanged(callback: () => void): Promise<UnlistenFn> {
  return listen("accounts-changed", callback);
}

export function listSignatures(): Promise<Signature[]> {
  return invoke<Signature[]>("list_signatures");
}

export function createSignature(name: string): Promise<Signature> {
  return invoke<Signature>("create_signature", { name });
}

export function updateSignature(
  id: number,
  name: string,
  body: string,
): Promise<void> {
  return invoke<void>("update_signature", { id, name, body });
}

export function deleteSignature(id: number): Promise<void> {
  return invoke<void>("delete_signature", { id });
}

/** Make one signature the default for exactly the given accounts. */
export function setSignatureAccounts(
  id: number,
  accountIds: number[],
): Promise<void> {
  return invoke<void>("set_signature_accounts", { id, accountIds });
}

export function listAliases(): Promise<Alias[]> {
  return invoke<Alias[]>("list_aliases");
}

export function addAlias(
  accountId: number,
  name: string,
  email: string,
): Promise<Alias> {
  return invoke<Alias>("add_alias", { accountId, name, email });
}

export function updateAlias(
  id: number,
  name: string,
  email: string,
): Promise<void> {
  return invoke<void>("update_alias", { id, name, email });
}

export function deleteAlias(id: number): Promise<void> {
  return invoke<void>("delete_alias", { id });
}

/** Pick the identity new mail from this account starts with;
 * null = the account's own address. */
export function setDefaultAlias(
  accountId: number,
  aliasId: number | null,
): Promise<void> {
  return invoke<void>("set_default_alias", { accountId, aliasId });
}

/** Fires whenever any window mutates the signature list. */
export function onSignaturesChanged(callback: () => void): Promise<UnlistenFn> {
  return listen("signatures-changed", callback);
}
