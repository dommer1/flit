use serde::{Deserialize, Serialize};

// why: rename_all = "camelCase" so the frontend sees idiomatic TS field names
// (accountId) while Rust keeps idiomatic snake_case (account_id).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    // why: i64, not u32 — SQLite rowids are 64-bit signed and sqlx decodes
    // them as i64; matching the storage type avoids lossy casts.
    pub id: i64,
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
    /// Error message from the last connection check; `None` = healthy.
    pub last_error: Option<String>,
    /// Unix seconds of the last check; `None` = never checked yet.
    pub checked_at: Option<i64>,
}

/// Payload for creating an account. Deliberately has no password field —
/// the secret travels as a separate command argument and never enters the
/// storage layer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAccount {
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
}

/// Payload for send_message: what the compose form submits. Deliberately has
/// no From field — the sender address always comes from the account row, so
/// the frontend can never spoof it. Also serves as the draft a compose
/// window opens with, hence Serialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingMessage {
    pub account_id: i64,
    /// One or more recipients, comma-separated.
    pub to: String,
    pub subject: String,
    /// Plain text only for now — no HTML composing.
    pub body: String,
}

/// Body payload for the message viewer. `html`, when present, is already a
/// full sanitized srcdoc document (mail::sanitize) — never raw mail HTML.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageBody {
    pub html: Option<String>,
    pub text: Option<String>,
}

/// One folder of one account, as shown in the sidebar.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    pub id: i64,
    pub account_id: i64,
    /// Full IMAP name — also the value the messages.mailbox column carries.
    pub name: String,
    /// "inbox" | "drafts" | "sent" | "archive" | "junk" | "trash" | null.
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MessageHeader {
    pub id: i64,
    pub account_id: i64,
    pub from: String,
    pub subject: String,
    pub snippet: String,
    // why: RFC3339 string for now — sorts chronologically as plain text and
    // serializes cleanly; becomes a real timestamp with SQLite in Phase 1.
    pub date: String,
    pub read: bool,
}
