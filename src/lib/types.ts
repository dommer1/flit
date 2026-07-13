// Mirrors src-tauri/src/models.rs — keep in sync (serde renames to camelCase).

export interface Account {
  id: number;
  name: string;
  email: string;
  imapHost: string;
  imapPort: number;
  smtpHost: string;
  smtpPort: number;
  username: string;
  /** Error message from the last connection check; null = healthy. */
  lastError: string | null;
  /** Unix seconds of the last check; null = never checked yet. */
  checkedAt: number | null;
}

/** Payload for add_account — the password travels as a separate argument. */
export interface NewAccount {
  name: string;
  email: string;
  imapHost: string;
  imapPort: number;
  smtpHost: string;
  smtpPort: number;
  username: string;
}

/** Payload for send_message — the From address comes from the account row. */
export interface OutgoingMessage {
  accountId: number;
  /** One or more recipients, comma-separated. */
  to: string;
  /** Cc recipients, comma-separated; empty/absent = none. */
  cc?: string;
  /** Bcc recipients — SMTP envelope only, never a visible header. */
  bcc?: string;
  subject: string;
  /** Plain text only for now. */
  body: string;
}

/** Payload of send-queued / send-finished / send-undone events. */
export interface SendEvent {
  id: number;
  subject: string;
  /** Set only on send-finished when the delivery failed. */
  error: string | null;
  /** Undo window length in ms; only meaningful on send-queued, 0 elsewhere. */
  undoMs: number;
}

/**
 * How remote (http/https) images in mail bodies are treated. Inline cid:
 * images always render — they are part of the message itself.
 */
export type RemoteImagePolicy = "block" | "ask" | "always";

/** `html`, when present, is a full sanitized srcdoc document from the backend. */
export interface MessageBody {
  html: string | null;
  text: string | null;
  /** Remote images left blocked in `html`; 0 when none or all loaded. */
  blockedImages: number;
  /** Offer "Load Images" (policy is "ask" and this render blocked some). */
  canLoadRemote: boolean;
}

/** One folder of one account, as shown in the sidebar. */
export interface Mailbox {
  id: number;
  accountId: number;
  /** Full IMAP name — also the value list_messages expects as `mailbox`. */
  name: string;
  /** "inbox" | "drafts" | "sent" | "archive" | "junk" | "trash" | null. */
  role: string | null;
  /** Decoded, prefix-stripped name for the UI — never send to the backend. */
  displayName: string;
}

export interface MessageHeader {
  id: number;
  accountId: number;
  from: string;
  /** To recipients as displayed ("Name <addr>", comma-separated). */
  to: string;
  /** Cc recipients, same shape; empty when there were none. */
  cc: string;
  subject: string;
  snippet: string;
  date: string;
  read: boolean;
}
