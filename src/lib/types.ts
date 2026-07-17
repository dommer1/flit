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
  /** User-chosen accent color (e.g. "#ff9f0a"); null = no color set. */
  color: string | null;
  /** Default signature for mail composed from this account; null = none. */
  signatureId: number | null;
  /** New-mail notification override; null = inherit the global setting. */
  notifyEnabled: boolean | null;
  /** Sound override ("none" or a macOS sound name); null = inherit. */
  notifySound: string | null;
  /** Send-as identity new mail starts with; null = the account's address. */
  defaultAliasId: number | null;
}

/** One send-as alias: an extra address the account's mail server accepts
 * as sender. Only stored aliases may appear as From. */
export interface Alias {
  id: number;
  accountId: number;
  /** Display name for the From header; empty = address only. */
  name: string;
  email: string;
}

/** One reusable e-mail signature; `body` is editor HTML. */
export interface Signature {
  id: number;
  name: string;
  body: string;
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
  /** Send-as alias picked in compose; absent/null = the account's address. */
  aliasId?: number | null;
  /** One or more recipients, comma-separated. */
  to: string;
  /** Cc recipients, comma-separated; empty/absent = none. */
  cc?: string;
  /** Bcc recipients — SMTP envelope only, never a visible header. */
  bcc?: string;
  subject: string;
  /** Plain-text body; doubles as the fallback part of an HTML message. */
  body: string;
  /** HTML rendering of the body; absent/empty = plain text only. */
  bodyHtml?: string;
  /** Attached files; absent/empty = none. Bytes are read at send time. */
  attachments?: AttachmentRef[];
  /** Message-ID of this message's autosaved server draft version, if any —
   * the backend deletes it from the Drafts folder after a successful send. */
  draftMessageId?: string;
  /** Message-ID (no angle brackets) of the mail this one answers — becomes
   * the In-Reply-To header. Absent for fresh mail. */
  inReplyTo?: string;
  /** Space-separated ancestor Message-IDs for the References header,
   * oldest first, ending with `inReplyTo`. */
  references?: string;
}

/** One file attached to an outgoing message, referenced by path. */
export interface AttachmentRef {
  path: string;
  /** Filename shown to recipients — the path's final component. */
  name: string;
}

/** inspect_attachments result — chip metadata for one dropped file. */
export interface AttachmentInfo {
  path: string;
  name: string;
  /** File size in bytes, for display only. */
  size: number;
}

/** One "Send Later" message parked in the backend until its delivery time.
 * Mirrors ScheduledMessage in models.rs. */
export interface ScheduledMessage {
  id: number;
  accountId: number;
  /** Send-as alias the message was composed with; null = account address. */
  aliasId: number | null;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
  bodyHtml: string | null;
  attachments: AttachmentRef[];
  /** Unix seconds (UTC) when the message should leave. */
  scheduledAt: number;
  /** Missed rows never send on their own — the user resolves them. */
  status: "pending" | "missed";
  /** Threading identity of a scheduled reply; null for fresh mail. */
  inReplyTo: string | null;
  references: string | null;
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

/** Mirrors NotificationSettings in models.rs. */
export interface NotificationSettings {
  /** Master switch for new-mail notifications. */
  enabled: boolean;
  /** "default" = system sound, "none" = silent, else a macOS sound name. */
  sound: string;
  /** Minutes between background new-mail checks; 0 = manual sync only. */
  syncIntervalMinutes: number;
}

/** What one direction of the message-list swipe gesture does;
 * "none" disables that direction. */
export type SwipeAction = "none" | "toggleRead" | "archive" | "trash" | "reply";

/** Order of messages in the conversation view. */
export type ThreadOrder = "newestLast" | "newestFirst";

/** The configured action for each swipe direction on message-list rows. */
export interface SwipeActions {
  left: SwipeAction;
  right: SwipeAction;
}

/** `html`, when present, is a full sanitized srcdoc document from the backend. */
export interface MessageBody {
  html: string | null;
  text: string | null;
  /** Remote images left blocked in `html`; 0 when none or all loaded. */
  blockedImages: number;
  /** Offer "Load Images" (policy is "ask" and this render blocked some). */
  canLoadRemote: boolean;
  /** Attachments — metadata only; saving re-fetches bytes from the server. */
  attachments: MessageAttachment[];
}

/** One attachment of a received message, as cached metadata. */
export interface MessageAttachment {
  id: number;
  messageId: number;
  /** MIME part index used by the backend to re-extract the bytes. */
  partIndex: number;
  filename: string;
  /** Full MIME type ("application/pdf"). */
  contentType: string;
  /** Decoded size in bytes, for display. */
  size: number;
}

/** One autocomplete suggestion for a compose recipient field. */
export interface Contact {
  email: string;
  /** Latest display name seen for the address; empty when none was. */
  name: string;
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
  /** Unread messages in this folder, from the local cache. */
  unreadCount: number;
}

export interface MessageHeader {
  id: number;
  accountId: number;
  /** Full IMAP name of the folder the message lives in. */
  mailbox: string;
  from: string;
  /** To recipients as displayed ("Name <addr>", comma-separated). */
  to: string;
  /** Cc recipients, same shape; empty when there were none. */
  cc: string;
  /** Reply-To recipients, same shape; empty when the header was absent. */
  replyTo: string;
  /** Bcc recipients, same shape; almost always empty — Sent copies from
   * other clients (or journaled deliveries) can carry the header. */
  bcc: string;
  subject: string;
  snippet: string;
  date: string;
  read: boolean;
  /** Whether the message (in threaded lists: any message of the
   * conversation) carries attachments — drives the list's paperclip. */
  hasAttachments: boolean;
  /** Message-ID without angle brackets; empty when the sender set none. */
  messageId: string;
  /** Space-joined ancestor Message-IDs — a reply appends messageId to this
   * chain to form its own References header. */
  references: string;
  /** Messages in this row's conversation (all folders except trash, junk
   * and drafts). Always ≥ 1 in threaded lists; 0 in flat views. */
  threadCount: number;
  /** Whether any message of the conversation is unread. */
  threadUnread: boolean;
}
