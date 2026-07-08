import { invoke } from "@tauri-apps/api/core";
import type { Account, MessageHeader, NewAccount } from "./types";

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

/** `accountId: null` = unified inbox across all accounts. */
export function listMessages(accountId: number | null): Promise<MessageHeader[]> {
  return invoke<MessageHeader[]>("list_messages", { accountId });
}
