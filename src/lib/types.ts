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

/** `html`, when present, is a full sanitized srcdoc document from the backend. */
export interface MessageBody {
  html: string | null;
  text: string | null;
}

export interface MessageHeader {
  id: number;
  accountId: number;
  from: string;
  subject: string;
  snippet: string;
  date: string;
  read: boolean;
}
