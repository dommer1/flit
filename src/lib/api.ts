import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import type {
  Account,
  Mailbox,
  MessageBody,
  MessageHeader,
  NewAccount,
  OutgoingMessage,
  RemoteImagePolicy,
  SendEvent,
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

/** Folders of one account in sidebar order (roles first, then customs). */
export function listMailboxes(accountId: number): Promise<Mailbox[]> {
  return invoke<Mailbox[]>("list_mailboxes", { accountId });
}

/** `accountId: null` = the given mailbox across all accounts. */
export function listMessages(
  accountId: number | null,
  mailbox = "INBOX",
): Promise<MessageHeader[]> {
  return invoke<MessageHeader[]>("list_messages", { accountId, mailbox });
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
