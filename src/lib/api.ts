import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import type {
  Account,
  MessageBody,
  MessageHeader,
  NewAccount,
  OutgoingMessage,
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

/** `accountId: null` = unified inbox across all accounts. */
export function listMessages(accountId: number | null): Promise<MessageHeader[]> {
  return invoke<MessageHeader[]>("list_messages", { accountId });
}

/** Body from cache, lazily fetched from the server on first open. */
export function getMessageBody(messageId: number): Promise<MessageBody> {
  return invoke<MessageBody>("get_message_body", { messageId });
}

/** Fire-and-forget header sync for one account's inbox. */
export function syncInbox(accountId: number): Promise<void> {
  return invoke<void>("sync_inbox", { accountId });
}

/** Send a composed message via the sending account's SMTP server. */
export function sendMessage(message: OutgoingMessage): Promise<void> {
  return invoke<void>("send_message", { message });
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
