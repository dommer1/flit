import { invoke } from "@tauri-apps/api/core";
import type { Account, MessageHeader } from "./types";

export function listAccounts(): Promise<Account[]> {
  return invoke<Account[]>("list_accounts");
}

/** `accountId: null` = unified inbox across all accounts. */
export function listMessages(accountId: number | null): Promise<MessageHeader[]> {
  return invoke<MessageHeader[]>("list_messages", { accountId });
}
