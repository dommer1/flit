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
    /// User-chosen accent color (e.g. "#ff9f0a"); `None` = no color set.
    pub color: Option<String>,
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
    /// Cc recipients, comma-separated; empty = none.
    // why serde(default): drafts and payloads written before these fields
    // existed must keep deserializing as "no cc/bcc".
    #[serde(default)]
    pub cc: String,
    /// Bcc recipients, comma-separated; empty = none. Bcc travels only in
    /// the SMTP envelope — it never becomes a transmitted header.
    #[serde(default)]
    pub bcc: String,
    pub subject: String,
    /// Plain-text body; always present, doubles as the fallback part when
    /// an HTML body rides along.
    pub body: String,
    /// HTML version of the body; None (or blank) = plain-text-only message.
    #[serde(default)]
    pub body_html: Option<String>,
    /// Files attached by the user. Only paths travel through the app; the
    /// bytes are read from disk when the MIME message is built.
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
}

/// What inspect_attachments returns for one dropped file — the metadata the
/// compose window renders as a removable chip.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub path: String,
    /// The path's final component — becomes AttachmentRef.name on send.
    pub name: String,
    /// File size in bytes, for display only; the 25 MB budget is enforced
    /// against the real bytes at MIME build time.
    pub size: u64,
}

/// One file attached to an outgoing message, referenced by path so drafts
/// (undo, failed sends) stay tiny and re-openable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRef {
    /// Absolute path on the local disk.
    pub path: String,
    /// Filename shown to recipients — the path's final component.
    pub name: String,
}

/// One "Send Later" message parked in SQLite until its delivery time.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledMessage {
    pub id: i64,
    pub account_id: i64,
    // why sqlx(rename): the columns follow the messages-table naming
    // (to_addr/cc_addr) while the struct mirrors OutgoingMessage, so the
    // frontend sees the same field names in drafts and scheduled rows.
    #[sqlx(rename = "to_addr")]
    pub to: String,
    #[sqlx(rename = "cc_addr")]
    pub cc: String,
    #[sqlx(rename = "bcc_addr")]
    pub bcc: String,
    pub subject: String,
    pub body: String,
    /// HTML version of the body; None = plain-text-only message.
    pub body_html: Option<String>,
    /// Attachment references; the column stores them as a JSON array and
    /// #[sqlx(json)] does the (de)serialization on read.
    #[sqlx(json)]
    pub attachments: Vec<AttachmentRef>,
    /// Unix seconds (UTC) when the message should leave.
    pub scheduled_at: i64,
    /// "pending" | "missed" — missed rows never send on their own; the user
    /// resolves them in the catch-up dialog.
    pub status: String,
}

impl ScheduledMessage {
    /// The compose-shaped form: what deliver() and reopened drafts expect.
    pub fn outgoing(&self) -> OutgoingMessage {
        OutgoingMessage {
            account_id: self.account_id,
            to: self.to.clone(),
            cc: self.cc.clone(),
            bcc: self.bcc.clone(),
            subject: self.subject.clone(),
            body: self.body.clone(),
            body_html: self.body_html.clone(),
            attachments: self.attachments.clone(),
        }
    }
}

/// How the viewer treats remote (http/https) images in mail bodies.
/// Inline cid: images always render — they are part of the message and
/// loading them touches no network. Remote images are the tracking vector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteImagePolicy {
    /// Never load; no banner either.
    Block,
    /// Default: banner per message, load only on an explicit click.
    #[default]
    Ask,
    /// Load automatically. Known trackers are still stripped.
    Always,
}

impl RemoteImagePolicy {
    /// The wire/storage form — matches the serde `lowercase` names.
    pub fn as_str(self) -> &'static str {
        match self {
            RemoteImagePolicy::Block => "block",
            RemoteImagePolicy::Ask => "ask",
            RemoteImagePolicy::Always => "always",
        }
    }

    /// why: unknown strings fall back to Ask (the default) — Ask never
    /// touches the network without an explicit click, so a corrupt or
    /// future value can't silently enable auto-loading.
    pub fn parse(value: &str) -> Self {
        match value {
            "block" => RemoteImagePolicy::Block,
            "always" => RemoteImagePolicy::Always,
            _ => RemoteImagePolicy::Ask,
        }
    }
}

/// One attachment of a cached message — metadata only. The bytes stay on
/// the server and are re-fetched by part_index when the user saves the file.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MessageAttachment {
    pub id: i64,
    pub message_id: i64,
    /// Position in mail-parser's attachment enumeration of the raw message.
    pub part_index: i64,
    pub filename: String,
    /// Full MIME type ("application/pdf").
    pub content_type: String,
    /// Decoded size in bytes, for display.
    pub size: i64,
}

/// Body payload for the message viewer. `html`, when present, is already a
/// full sanitized srcdoc document (mail::sanitize) — never raw mail HTML.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageBody {
    pub html: Option<String>,
    pub text: Option<String>,
    /// Remote images left blocked in `html` — 0 when there were none or
    /// they all loaded.
    pub blocked_images: usize,
    /// Whether the viewer should offer "Load images" (policy is Ask and
    /// this render still blocked something).
    pub can_load_remote: bool,
    /// Attachments of this message — metadata only; saving re-fetches the
    /// bytes from the server.
    pub attachments: Vec<MessageAttachment>,
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
    /// Human-readable name for the UI: modified UTF-7 decoded, Gmail's
    /// "[Gmail]/" container prefix stripped. Never send this back to the
    /// server — IMAP commands need `name`.
    #[sqlx(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MessageHeader {
    pub id: i64,
    pub account_id: i64,
    /// Full IMAP name of the folder the message lives in — lets the UI know
    /// a message's home even in unified or search views that span folders.
    pub mailbox: String,
    pub from: String,
    /// To recipients as displayed ("Name <addr>", comma-separated).
    pub to: String,
    /// Cc recipients, same shape; empty when there were none.
    pub cc: String,
    /// Reply-To recipients, same shape; empty when the header was absent.
    pub reply_to: String,
    pub subject: String,
    pub snippet: String,
    // why: RFC3339 string for now — sorts chronologically as plain text and
    // serializes cleanly; becomes a real timestamp with SQLite in Phase 1.
    pub date: String,
    pub read: bool,
}
