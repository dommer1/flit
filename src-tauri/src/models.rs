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
