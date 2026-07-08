// Mirrors src-tauri/src/models.rs — keep in sync (serde renames to camelCase).

export interface Account {
  id: number;
  name: string;
  email: string;
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
