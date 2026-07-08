use serde::Serialize;

// why: rename_all = "camelCase" so the frontend sees idiomatic TS field names
// (accountId) while Rust keeps idiomatic snake_case (account_id).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: u32,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageHeader {
    pub id: u32,
    pub account_id: u32,
    pub from: String,
    pub subject: String,
    pub snippet: String,
    // why: RFC3339 string for now — sorts chronologically as plain text and
    // serializes cleanly; becomes a real timestamp with SQLite in Phase 1.
    pub date: String,
    pub read: bool,
}
