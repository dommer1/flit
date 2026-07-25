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
    /// Default signature for mail composed from this account; `None` = none.
    pub signature_id: Option<i64>,
    /// New-mail notification override; `None` = inherit the global setting.
    pub notify_enabled: Option<bool>,
    /// Notification sound override ("none" or a macOS sound name);
    /// `None` = inherit the global default sound.
    pub notify_sound: Option<String>,
    /// The send-as identity new mail from this account starts with;
    /// `None` = the account's own address.
    pub default_alias_id: Option<i64>,
}

/// One send-as alias: an extra address the account's mail server accepts as
/// sender (mail to it already lands in the account's inbox). Only stored
/// aliases may appear as From — the send path resolves and checks them.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Alias {
    pub id: i64,
    pub account_id: i64,
    /// Display name for the From header; empty = address only.
    pub name: String,
    pub email: String,
}

/// One reusable e-mail signature. `body` is editor HTML — the compose editor
/// inserts it verbatim into the message body.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    pub id: i64,
    pub name: String,
    pub body: String,
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
    /// Send-as alias picked in the compose window; `None` = the account's
    /// own address. Resolved against the account row at send time, so the
    /// frontend still cannot produce an unconfigured From address.
    #[serde(default)]
    pub alias_id: Option<i64>,
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
    /// Message-ID of this message's autosaved version in the server's
    /// Drafts folder, if any — deleted there after a successful send.
    #[serde(default)]
    pub draft_message_id: Option<String>,
    /// Message-ID (no angle brackets) of the mail this one answers — sent as
    /// the In-Reply-To header so recipients thread the reply. None for fresh
    /// mail.
    #[serde(default)]
    pub in_reply_to: Option<String>,
    /// Space-separated ancestor Message-IDs for the References header,
    /// oldest first, ending with `in_reply_to`.
    #[serde(default)]
    pub references: Option<String>,
    /// The reply-quote riding below the compose editor, still un-merged
    /// into body/body_html. The backend never reads it — it travels so a
    /// reopened draft (undo, failed send) can restore its quote block.
    #[serde(default)]
    pub quote: Option<DraftQuote>,
}

/// A compose draft's quote of the original message: quote material plus the
/// attribution line the frontend built for it ("On …, X wrote:").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftQuote {
    pub attribution: String,
    /// Sanitized HTML fragment of the original (see MessageQuote::html).
    pub html: String,
    /// Cleaned plain text of the original (see MessageQuote::text).
    pub text: String,
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
    /// Send-as alias the message was composed with; None = account address.
    pub alias_id: Option<i64>,
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
    /// Threading identity of a scheduled reply; None for fresh mail.
    pub in_reply_to: Option<String>,
    #[sqlx(rename = "references_hdr")]
    pub references: Option<String>,
}

impl ScheduledMessage {
    /// The compose-shaped form: what deliver() and reopened drafts expect.
    pub fn outgoing(&self) -> OutgoingMessage {
        OutgoingMessage {
            account_id: self.account_id,
            alias_id: self.alias_id,
            to: self.to.clone(),
            cc: self.cc.clone(),
            bcc: self.bcc.clone(),
            subject: self.subject.clone(),
            body: self.body.clone(),
            body_html: self.body_html.clone(),
            attachments: self.attachments.clone(),
            // Scheduled rows never track a server draft version.
            draft_message_id: None,
            in_reply_to: self.in_reply_to.clone(),
            references: self.references.clone(),
            // Scheduled rows store the already-composed body; a cancelled
            // reply reopens with its quote merged into the editor content.
            quote: None,
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

/// Global new-mail notification defaults plus the background poll cadence.
/// Per-account overrides live on the accounts table; NULL there = inherit
/// these values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettings {
    /// Master switch for new-mail notifications.
    pub enabled: bool,
    /// "default" = the system notification sound, "none" = silent banner,
    /// anything else = a macOS sound name such as "Ping".
    pub sound: String,
    /// Minutes between background new-mail checks; 0 = manual sync only.
    pub sync_interval_minutes: i64,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sound: "default".to_string(),
            sync_interval_minutes: 3,
        }
    }
}

/// Order of messages in the conversation view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadOrder {
    /// Default: chronological — the newest message sits at the bottom.
    #[default]
    NewestLast,
    /// Newest message on top.
    NewestFirst,
}

impl ThreadOrder {
    /// The wire/storage form — matches the serde `camelCase` names.
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadOrder::NewestLast => "newestLast",
            ThreadOrder::NewestFirst => "newestFirst",
        }
    }

    /// why: unknown strings fall back to the default — a corrupt row must
    /// never break the conversation view.
    pub fn parse(value: &str) -> Self {
        match value {
            "newestFirst" => ThreadOrder::NewestFirst,
            _ => ThreadOrder::NewestLast,
        }
    }
}

/// How dates are written in the UI. Every variant but `System` is named
/// after exactly what it produces, and that pattern IS its wire form — the
/// value the settings menu shows is the value stored and the value the
/// frontend switches on, so the three can never drift apart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateFormat {
    /// Default: whatever the OS locale writes (25 July 2026, 7/25/2026, …).
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "dd.mm.yyyy")]
    DayMonthYearDot,
    #[serde(rename = "dd.mm.yy")]
    DayMonthShortYearDot,
    #[serde(rename = "dd/mm/yyyy")]
    DayMonthYearSlash,
    #[serde(rename = "mm/dd/yyyy")]
    MonthDayYearSlash,
    #[serde(rename = "yyyy-mm-dd")]
    YearMonthDayDash,
    #[serde(rename = "yyyy/mm/dd")]
    YearMonthDaySlash,
}

impl DateFormat {
    /// The wire/storage form — matches the serde names.
    pub fn as_str(self) -> &'static str {
        match self {
            DateFormat::System => "system",
            DateFormat::DayMonthYearDot => "dd.mm.yyyy",
            DateFormat::DayMonthShortYearDot => "dd.mm.yy",
            DateFormat::DayMonthYearSlash => "dd/mm/yyyy",
            DateFormat::MonthDayYearSlash => "mm/dd/yyyy",
            DateFormat::YearMonthDayDash => "yyyy-mm-dd",
            DateFormat::YearMonthDaySlash => "yyyy/mm/dd",
        }
    }

    /// why: unknown strings fall back to the locale's own format — that is
    /// always readable, so a corrupt row can't leave dates unreadable.
    pub fn parse(value: &str) -> Self {
        match value {
            "dd.mm.yyyy" => DateFormat::DayMonthYearDot,
            "dd.mm.yy" => DateFormat::DayMonthShortYearDot,
            "dd/mm/yyyy" => DateFormat::DayMonthYearSlash,
            "mm/dd/yyyy" => DateFormat::MonthDayYearSlash,
            "yyyy-mm-dd" => DateFormat::YearMonthDayDash,
            "yyyy/mm/dd" => DateFormat::YearMonthDaySlash,
            _ => DateFormat::System,
        }
    }
}

/// Whether clock times read as 14:30 or 2:30 PM.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeFormat {
    /// Default: the OS locale's own convention.
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "24h")]
    Hour24,
    #[serde(rename = "12h")]
    Hour12,
}

impl TimeFormat {
    /// The wire/storage form — matches the serde names.
    pub fn as_str(self) -> &'static str {
        match self {
            TimeFormat::System => "system",
            TimeFormat::Hour24 => "24h",
            TimeFormat::Hour12 => "12h",
        }
    }

    /// why: same as DateFormat — anything unknown reverts to the locale.
    pub fn parse(value: &str) -> Self {
        match value {
            "24h" => TimeFormat::Hour24,
            "12h" => TimeFormat::Hour12,
            _ => TimeFormat::System,
        }
    }
}

/// How the UI writes dates and clock times. The two halves are independent
/// — a 24-hour clock with US dates is a perfectly ordinary combination.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateTimeFormat {
    pub date: DateFormat,
    pub time: TimeFormat,
}

/// What one direction of the message-list swipe gesture does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SwipeAction {
    /// That direction is disabled — no strip, no action.
    None,
    ToggleRead,
    Archive,
    Trash,
    Reply,
}

impl SwipeAction {
    /// The wire/storage form — matches the serde `camelCase` names.
    pub fn as_str(self) -> &'static str {
        match self {
            SwipeAction::None => "none",
            SwipeAction::ToggleRead => "toggleRead",
            SwipeAction::Archive => "archive",
            SwipeAction::Trash => "trash",
            SwipeAction::Reply => "reply",
        }
    }

    /// why Option, not a default: the fallback for a corrupt value differs
    /// per side (left → Archive, right → ToggleRead), so the caller picks.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(SwipeAction::None),
            "toggleRead" => Some(SwipeAction::ToggleRead),
            "archive" => Some(SwipeAction::Archive),
            "trash" => Some(SwipeAction::Trash),
            "reply" => Some(SwipeAction::Reply),
            _ => None,
        }
    }
}

/// The configured action for each swipe direction on message-list rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwipeActions {
    pub left: SwipeAction,
    pub right: SwipeAction,
}

impl Default for SwipeActions {
    /// The gesture as originally shipped: left archives, right toggles read.
    fn default() -> Self {
        SwipeActions {
            left: SwipeAction::Archive,
            right: SwipeAction::ToggleRead,
        }
    }
}

/// One autocomplete suggestion for a compose recipient field — an address
/// harvested from cached or sent mail (see storage::contacts).
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub email: String,
    /// Latest display name seen for the address; empty when none was.
    pub name: String,
}

/// SPF/DKIM/DMARC verdicts read from a message's topmost
/// Authentication-Results header — the one stamped by the user's own
/// receiving server. Each field holds the lowercase verdict token ("pass",
/// "fail", "softfail", …); None = the header did not mention that method.
/// Deserialize too: the verdicts are cached in the DB as JSON.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResults {
    pub spf: Option<String>,
    pub dkim: Option<String>,
    pub dmarc: Option<String>,
}

/// A sender whose display name is well known in the local contact history —
/// but under a different address. Shown as a warning banner on the message
/// ("X does not usually use this email address"). Computed purely from the
/// local contacts table; nothing ever leaves the machine.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SenderAnomaly {
    /// The familiar display name the message arrived under.
    pub name: String,
    /// The address this name usually writes from (its busiest contact row).
    pub usual_email: String,
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
    /// Quoted history split off a plain-text body — the viewer folds it
    /// behind a toggle. None when the body has none (or is all quote).
    pub quoted_text: Option<String>,
    /// Remote images left blocked in `html` — 0 when there were none or
    /// they all loaded.
    pub blocked_images: usize,
    /// Whether the viewer should offer "Load images" (policy is Ask and
    /// this render still blocked something).
    pub can_load_remote: bool,
    /// Attachments of this message — metadata only; saving re-fetches the
    /// bytes from the server.
    pub attachments: Vec<MessageAttachment>,
    /// SPF/DKIM/DMARC verdicts of this message; None = unknown (no
    /// Authentication-Results header, or cached before harvesting). The
    /// viewer warns only on explicit "fail" — unknown stays silent.
    pub auth: Option<AuthResults>,
    /// Set when the From line pairs a familiar name with an address that
    /// name does not usually use (see storage::contacts::sender_anomaly).
    pub sender_anomaly: Option<SenderAnomaly>,
}

/// Quote material for a reply: the original message rendered for embedding
/// below the user's own text in a compose window. Deserialize too — the
/// compose draft carries it back and forth (see OutgoingMessage).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageQuote {
    /// Sanitized HTML fragment of the original — cid images inlined as
    /// data: URIs, remote refs left unresolved, plain-text mail upconverted
    /// to blockquote markup. Empty = nothing quotable.
    pub html: String,
    /// Plain text of the original, invisible padding stripped — source of
    /// the "> " lines in the outgoing text/plain part.
    pub text: String,
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
    /// Unread messages in this folder, from the local cache.
    pub unread_count: i64,
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
    /// Bcc recipients, same shape; almost always empty — Sent copies from
    /// other clients (or journaled deliveries) can carry the header.
    pub bcc: String,
    pub subject: String,
    pub snippet: String,
    // why: RFC3339 string for now — sorts chronologically as plain text and
    // serializes cleanly; becomes a real timestamp with SQLite in Phase 1.
    /// In threaded lists: the newest message of the conversation (my own
    /// reply, filed in Sent, counts) — not necessarily this row's own date.
    pub date: String,
    pub read: bool,
    /// Whether the message (in threaded lists: any message of the
    /// conversation) carries attachments — the list's paperclip.
    pub has_attachments: bool,
    /// Message-ID without angle brackets; empty when the sender set none.
    /// A reply to this message sends it back as In-Reply-To.
    pub message_id: String,
    /// Space-joined ancestor Message-IDs — a reply appends `message_id` to
    /// this chain to form its own References header.
    pub references: String,
    /// Whether the message sits in the account's Drafts folder. Only the
    /// conversation query computes it (badge + click-to-edit routing);
    /// list queries leave it false — the flat drafts view derives
    /// draft-ness from the mailbox role instead.
    #[sqlx(default)]
    pub is_draft: bool,
    /// Messages in this row's conversation — all folders of the account
    /// except trash/junk/drafts (a draft shows in the conversation view
    /// but is not counted as a message of the exchange). Always ≥ 1 in
    /// threaded lists; 0 in flat queries, which don't compute it.
    #[sqlx(default)]
    pub thread_count: i64,
    /// Whether any message of the conversation is unread.
    #[sqlx(default)]
    pub thread_unread: bool,
    /// Whether an unsent draft is saved for this conversation — the list
    /// row's "Draft" pill. Computed by threaded lists only.
    #[sqlx(default)]
    pub thread_has_draft: bool,
}

/// Counts for one list view (a folder, or a mailbox name across all
/// accounts) — what the list header and the backfill progress line show.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewStatus {
    /// Rows the unlimited list query would return: conversations in
    /// threaded views, messages in flat ones.
    pub list_rows: i64,
    /// Unread messages in the view, from the local cache.
    pub unread: i64,
    /// Messages the cache holds across every folder of the view's account
    /// (all accounts in the unified view) — progress is account-wide so the
    /// backfill is visible from any view.
    pub cached: i64,
    /// Messages the server holds in those same folders (sum of EXISTS
    /// counts); `None` until the first sync reports it. `cached <
    /// server_total` means the header backfill is still running.
    pub server_total: Option<i64>,
}
