//! Draft MIME building — lenient on purpose. A draft is work in progress:
//! recipients may be missing or half-typed, so unlike `smtp::build_message`
//! nothing here validates. Whatever the user typed is written verbatim and
//! must survive the round trip through the server (Gmail web included).
//!
//! why mail-builder and not lettre: lettre refuses to build a message
//! without at least one valid recipient — correct for sending, wrong for
//! drafts. mail-builder writes arbitrary address text and handles the
//! RFC 2047 encoding of non-ASCII headers (Slovak subjects) for us.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use mail_builder::headers::address::Address;
use mail_builder::headers::message_id::MessageId;
use mail_builder::MessageBuilder;

use crate::error::AppError;
use crate::models::{AttachmentRef, OutgoingMessage};

/// A fresh Message-ID (without the angle brackets) for one draft version.
/// Every save gets a new one — it is how the previous version is found and
/// deleted on the server, where IMAP has no "edit in place".
pub fn generate_message_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("flit-draft-{nanos}-{seq}@flit.local")
}

/// Build the raw RFC 5322 bytes of a draft, stamped with `message_id`.
/// Empty address fields produce no header; Bcc is kept — a draft never
/// leaves for recipients, and losing it on reopen would be data loss.
pub async fn build_draft(
    from_name: &str,
    from_email: &str,
    outgoing: &OutgoingMessage,
    message_id: &str,
) -> Result<Vec<u8>, AppError> {
    // why: the draft's From mirrors what the send will use (account or
    // alias), so webmail shows the right identity and reopening the draft
    // can restore the alias from this header.
    let name = from_name.trim();
    let from = Address::new_address((!name.is_empty()).then_some(name), from_email);
    let mut builder = MessageBuilder::new()
        .message_id(message_id)
        .from(from)
        .subject(outgoing.subject.as_str())
        .text_body(outgoing.body.as_str());
    // Keep the rich-text rendering: mail-builder writes text+html as
    // multipart/alternative, so formatting survives in webmail and reopen.
    if let Some(html) = outgoing
        .body_html
        .as_deref()
        .filter(|h| !h.trim().is_empty())
    {
        builder = builder.html_body(html);
    }
    if let Some(to) = recipient_list(&outgoing.to) {
        builder = builder.to(to);
    }
    if let Some(cc) = recipient_list(&outgoing.cc) {
        builder = builder.cc(cc);
    }
    if let Some(bcc) = recipient_list(&outgoing.bcc) {
        builder = builder.bcc(bcc);
    }
    // A reply draft keeps its threading identity, so sending it later (or
    // from another client that picks the draft up) still threads.
    if let Some(parent) = outgoing
        .in_reply_to
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        builder = builder.in_reply_to(parent);
    }
    let references: Vec<&str> = outgoing
        .references
        .as_deref()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    if !references.is_empty() {
        builder = builder.references(MessageId::new_list(references.into_iter()));
    }
    let mut total = 0usize;
    for attachment in &outgoing.attachments {
        let (content_type, bytes) = load_attachment(attachment, &mut total).await?;
        builder = builder.attachment(content_type, attachment.name.clone(), bytes);
    }
    Ok(builder.write_to_vec()?)
}

/// Split one comma-separated recipient field into addresses, preserving the
/// typed text; `None` when the field is empty. A piece without an `@` after
/// the first entry belongs to a display name containing a comma
/// (`"Novák, Ján" <jan@x>`), so it is glued back onto the previous piece —
/// the same heuristic the compose window uses in draft.ts.
fn recipient_list(field: &str) -> Option<Address<'_>> {
    let mut pieces: Vec<String> = Vec::new();
    for piece in field.split(',') {
        let trimmed = piece.trim();
        if trimmed.is_empty() {
            continue;
        }
        match pieces.last_mut() {
            Some(last) if !last.contains('@') => {
                last.push_str(", ");
                last.push_str(trimmed);
            }
            _ => pieces.push(trimmed.to_string()),
        }
    }
    if pieces.is_empty() {
        return None;
    }
    Some(Address::new_list(
        pieces.into_iter().map(recipient_piece).collect(),
    ))
}

/// One typed recipient: `Name <addr>` splits into display name + address;
/// anything else (a bare address, a half-typed fragment) passes through
/// verbatim as the address.
fn recipient_piece(piece: String) -> Address<'static> {
    if let Some((name, rest)) = piece.rsplit_once('<') {
        if let Some(addr) = rest.strip_suffix('>') {
            let name = name.trim().trim_matches('"').trim();
            let name = (!name.is_empty()).then(|| name.to_string());
            return Address::new_address(name, addr.trim().to_string());
        }
    }
    Address::new_address(None::<String>, piece)
}

/// The wrapper the compose window puts around a draft's quote block in the
/// HTML body — MUST stay in sync with QUOTE_MARKER in src/lib/draft.ts.
const QUOTE_MARKER: &str = r#"<div class="flit-draft-quote">"#;

/// Inline style of the rebuilt quote block — mirrors QUOTE_BLOCK_STYLE in
/// src/lib/draft.ts, so a reopened-then-resaved draft looks unchanged.
const QUOTE_BLOCK_STYLE: &str = "margin:0 0 0 0.8ex;border-left:2px solid #c8ccd4;padding-left:1ex";

/// The parts of a saved draft recovered for the compose window: the user's
/// own content and the quote block parked back behind the ••• toggle.
#[derive(Debug, PartialEq)]
pub struct SavedQuote {
    /// Plain text of the draft with the quote (attribution + "> " lines)
    /// stripped — what the editor reopens with.
    pub own_text: String,
    /// HTML before the marker — the user's own rich-text content.
    /// UNTRUSTED like quote_html: sanitize before it reaches the editor.
    pub own_html: String,
    /// The attribution line ("On …, X wrote:"), HTML entities decoded.
    pub attribution: String,
    /// Inner HTML of the quote block. UNTRUSTED — it comes from the server
    /// and MUST pass mail::sanitize::sanitize_fragment before it may reach
    /// the compose editor (hard rule).
    pub quote_html: String,
    /// The quoted plain text with one "> " level stripped — what the next
    /// save re-quotes, so levels never stack.
    pub quote_text: String,
}

/// Recover the quote block a saved draft carries, splitting both bodies at
/// the marker `composeHtmlBody` wrote. `None` (draft not composed by this
/// app, or bodies edited apart) means "reopen as-is" — never guess.
pub fn split_saved_quote(text: &str, html: &str) -> Option<SavedQuote> {
    let marker_at = html.find(QUOTE_MARKER)?;
    let block = &html[marker_at + QUOTE_MARKER.len()..];
    let attribution = unescape_html(block.strip_prefix("<p>")?.split("</p>").next()?);

    let bq_at = block.find("<blockquote")?;
    let inner_start = bq_at + block[bq_at..].find('>')? + 1;
    let inner_end = block.rfind("</blockquote>")?;
    if inner_end < inner_start {
        return None;
    }
    let quote_html = block[inner_start..inner_end].to_string();

    // The plain body mirrors the same structure: own text, a blank line,
    // the attribution, then "> " lines (composePlainBody). Both must parse,
    // or a reopened draft would duplicate its quote on the next save.
    let split_token = format!("\n\n{attribution}\n");
    let at = text.rfind(&split_token)?;
    let quote_text = text[at + split_token.len()..]
        .lines()
        .map(|line| {
            line.strip_prefix("> ")
                .or(line.strip_prefix(">"))
                .unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string();
    Some(SavedQuote {
        own_text: text[..at].to_string(),
        own_html: html[..marker_at].to_string(),
        attribution,
        quote_html,
        quote_text,
    })
}

/// Fallback for drafts without the HTML marker (saves by older builds,
/// other clients): split the plain body at the quoted-history boundary the
/// viewer already recognizes. Parks only when the boundary line can serve
/// as the attribution — a bare ">" wall has nothing to label the block
/// with. own_html stays empty; the editor rebuilds HTML from the text.
pub fn split_plain_quote(text: &str) -> Option<SavedQuote> {
    let (own_text, Some(quoted)) = crate::mail::quote::split_text_quote(text) else {
        return None;
    };
    let mut lines = quoted.lines();
    let attribution = lines.next()?.trim().to_string();
    if attribution.starts_with('>') {
        return None;
    }
    let quote_text = lines
        .map(|line| {
            line.strip_prefix("> ")
                .or(line.strip_prefix(">"))
                .unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if quote_text.is_empty() {
        return None;
    }
    let quote_html = crate::mail::quote::text_to_quote_html(&quote_text);
    Some(SavedQuote {
        own_text,
        own_html: String::new(),
        attribution,
        quote_html,
        quote_text,
    })
}

/// Rebuild the composed HTML body from (sanitized) parts — the exact shape
/// `composeHtmlBody` in src/lib/draft.ts writes, so the compose window's
/// splitComposedHtml finds the marker again on reopen.
pub fn compose_quoted_html(own_html: &str, attribution: &str, quote_html: &str) -> String {
    format!(
        "{own_html}{QUOTE_MARKER}<p>{}</p>\
         <blockquote type=\"cite\" style=\"{QUOTE_BLOCK_STYLE}\">{quote_html}</blockquote></div>",
        escape_html(attribution)
    )
}

/// Mirrors escapeHtml in src/lib/richtext.ts.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Reverse of escape_html; &amp; last so "&amp;lt;" cannot double-decode.
fn unescape_html(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Read one attachment from disk, keeping the running size total honest
/// against the same budget the send path enforces.
async fn load_attachment(
    attachment: &AttachmentRef,
    total: &mut usize,
) -> Result<(String, Vec<u8>), AppError> {
    let bytes = tokio::fs::read(&attachment.path).await.map_err(|e| {
        AppError::Io(std::io::Error::other(format!(
            "cannot read attachment {}: {e}",
            attachment.name
        )))
    })?;
    *total += bytes.len();
    crate::mail::smtp::ensure_attachment_budget(*total)?;
    let mime = mime_guess::from_path(&attachment.name).first_or_octet_stream();
    Ok((mime.essence_str().to_string(), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_parser::{MessageParser, MimeHeaders};

    #[test]
    fn split_saved_quote_round_trips_the_compose_format() {
        // Exactly what composePlainBody/composeHtmlBody write on save.
        let text = "bbbbb\n\nOn Jul 24, 2026, Peter <p@x> wrote:\n> ahoj\n>\n> čau\n";
        let html = compose_quoted_html(
            "<p>bbbbb</p>",
            "On Jul 24, 2026, Peter <p@x> wrote:",
            "<p>ahoj</p><p>čau</p>",
        );

        let split = split_saved_quote(text, &html).unwrap();

        assert_eq!(split.own_text, "bbbbb");
        assert_eq!(split.own_html, "<p>bbbbb</p>");
        assert_eq!(split.attribution, "On Jul 24, 2026, Peter <p@x> wrote:");
        assert_eq!(split.quote_html, "<p>ahoj</p><p>čau</p>");
        // One "> " level stripped — resaving quotes it again, so levels
        // never stack into "> > >".
        assert_eq!(split.quote_text, "ahoj\n\nčau");
    }

    #[test]
    fn split_plain_quote_rescues_a_markerless_draft() {
        // A draft flattened by an older build: quote merged into the plain
        // text, no HTML marker to split on.
        let text = "flit test\n\nOn July 24, 2026 at 2:20 PM, Obchod wrote:\n> Ahoj Dominik,\n>\n> poprosím ťa o zmeny\n";

        let split = split_plain_quote(text).unwrap();

        assert_eq!(split.own_text, "flit test");
        assert_eq!(split.own_html, "");
        assert_eq!(
            split.attribution,
            "On July 24, 2026 at 2:20 PM, Obchod wrote:"
        );
        assert_eq!(split.quote_text, "Ahoj Dominik,\n\npoprosím ťa o zmeny");
        assert!(split.quote_html.contains("poprosím ťa o zmeny"));
    }

    #[test]
    fn split_plain_quote_leaves_unlabelled_quotes_alone() {
        // A ">" wall with no attribution line — nothing to label the
        // parked block with, so the draft reopens as-is.
        assert_eq!(split_plain_quote("hi\n\n> one\n> two\n> three"), None);
        // No quote at all.
        assert_eq!(split_plain_quote("just text"), None);
    }

    #[test]
    fn split_saved_quote_refuses_foreign_or_mismatched_drafts() {
        // No marker (a draft saved by another client).
        assert_eq!(
            split_saved_quote("hi\n> old", "<p>hi</p><blockquote>old</blockquote>"),
            None
        );
        // Marker present but the plain body no longer mirrors it (edited
        // apart) — reopening as-is beats duplicating the quote.
        let html = compose_quoted_html("<p>hi</p>", "On X, Y wrote:", "<p>q</p>");
        assert_eq!(split_saved_quote("something else entirely", &html), None);
    }

    fn outgoing() -> OutgoingMessage {
        OutgoingMessage {
            account_id: 1,
            alias_id: None,
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            subject: "Pozvánka na obed".to_string(),
            body: "Ahoj, prídeš?".to_string(),
            body_html: None,
            attachments: Vec::new(),
            draft_message_id: None,
            in_reply_to: None,
            references: None,
            quote: None,
        }
    }

    async fn parsed(outgoing: &OutgoingMessage) -> mail_parser::Message<'static> {
        let raw = build_draft("", "domco@example.com", outgoing, "test-id@flit.local")
            .await
            .unwrap();
        // why leak: mail_parser::Message borrows the raw bytes; tests want to
        // return it from this helper, so give the bytes a 'static life.
        MessageParser::default().parse(&*raw.leak()).unwrap()
    }

    #[tokio::test]
    async fn builds_a_draft_with_no_recipients() {
        let message = parsed(&outgoing()).await;

        assert!(message.to().is_none());
        assert_eq!(message.subject(), Some("Pozvánka na obed"));
        assert_eq!(
            message.body_text(0).as_deref(),
            Some("Ahoj, prídeš?"),
            "diacritics must survive the round trip"
        );
    }

    #[tokio::test]
    async fn threading_headers_round_trip_through_a_draft() {
        let mut out = outgoing();
        out.in_reply_to = Some("parent@x".to_string());
        out.references = Some("root@x parent@x".to_string());

        let raw = build_draft("", "domco@example.com", &out, "id@flit.local")
            .await
            .unwrap();
        let parsed = crate::mail::parse::parse_draft(&raw);

        assert_eq!(parsed.in_reply_to.as_deref(), Some("parent@x"));
        assert_eq!(parsed.references.as_deref(), Some("root@x parent@x"));
    }

    #[tokio::test]
    async fn fresh_drafts_round_trip_without_threading_headers() {
        let raw = build_draft("", "domco@example.com", &outgoing(), "id@flit.local")
            .await
            .unwrap();
        let parsed = crate::mail::parse::parse_draft(&raw);

        assert_eq!(parsed.in_reply_to, None);
        assert_eq!(parsed.references, None);
    }

    #[tokio::test]
    async fn keeps_the_html_body_alongside_the_text() {
        let mut out = outgoing();
        out.body_html = Some("<p>Ahoj, <b>prídeš</b>?</p>".to_string());

        let message = parsed(&out).await;

        assert_eq!(
            message.body_html(0).as_deref(),
            Some("<p>Ahoj, <b>prídeš</b>?</p>")
        );
        assert_eq!(message.body_text(0).as_deref(), Some("Ahoj, prídeš?"));
    }

    #[tokio::test]
    async fn skips_a_blank_html_body() {
        // why raw bytes: mail-parser synthesizes an HTML view from the text
        // part, so only the wire format shows whether an html part exists.
        let mut out = outgoing();
        out.body_html = Some("   ".to_string());

        let raw = build_draft("", "domco@example.com", &out, "id@flit.local")
            .await
            .unwrap();

        assert!(!String::from_utf8(raw).unwrap().contains("text/html"));
    }

    #[tokio::test]
    async fn stamps_the_given_message_id() {
        let message = parsed(&outgoing()).await;

        assert_eq!(message.message_id(), Some("test-id@flit.local"));
    }

    #[tokio::test]
    async fn keeps_a_half_typed_recipient_verbatim() {
        let mut out = outgoing();
        out.to = "jan".to_string();

        let message = parsed(&out).await;

        let to = message.to().unwrap().first().unwrap();
        assert_eq!(to.address(), Some("jan"));
    }

    #[tokio::test]
    async fn splits_recipient_lists_and_keeps_display_names() {
        let mut out = outgoing();
        out.to = "Ján Novák <jan@example.sk>, maria@example.sk".to_string();

        let message = parsed(&out).await;

        let to: Vec<_> = message.to().unwrap().iter().collect();
        assert_eq!(to.len(), 2);
        assert_eq!(to[0].name(), Some("Ján Novák"));
        assert_eq!(to[0].address(), Some("jan@example.sk"));
        assert_eq!(to[1].address(), Some("maria@example.sk"));
    }

    #[tokio::test]
    async fn glues_a_comma_inside_a_display_name_back_together() {
        let mut out = outgoing();
        out.to = "\"Novák, Ján\" <jan@example.sk>".to_string();

        let message = parsed(&out).await;

        let to: Vec<_> = message.to().unwrap().iter().collect();
        assert_eq!(to.len(), 1);
        assert_eq!(to[0].address(), Some("jan@example.sk"));
    }

    #[tokio::test]
    async fn keeps_bcc_in_the_draft() {
        // The opposite of the send path: a draft never reaches recipients,
        // and dropping Bcc here would lose it on reopen.
        let mut out = outgoing();
        out.bcc = "hidden@example.com".to_string();

        let message = parsed(&out).await;

        let bcc = message.bcc().unwrap().first().unwrap();
        assert_eq!(bcc.address(), Some("hidden@example.com"));
    }

    #[tokio::test]
    async fn includes_attachment_bytes() {
        let path = std::env::temp_dir().join("flit-draft-test-notes.txt");
        std::fs::write(&path, b"poznamky").unwrap();
        let mut out = outgoing();
        out.attachments = vec![AttachmentRef {
            path: path.to_string_lossy().into_owned(),
            name: "notes.txt".to_string(),
        }];

        let message = parsed(&out).await;

        let attachment = message.attachment(0).unwrap();
        assert_eq!(attachment.attachment_name(), Some("notes.txt"));
        assert_eq!(attachment.contents(), b"poznamky");
    }

    #[tokio::test]
    async fn fails_on_a_missing_attachment_file() {
        let mut out = outgoing();
        out.attachments = vec![AttachmentRef {
            path: "/nonexistent/flit-draft-gone".to_string(),
            name: "gone.txt".to_string(),
        }];

        let err = build_draft("", "domco@example.com", &out, "id@flit.local")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("gone.txt"));
    }

    #[tokio::test]
    async fn a_saved_draft_parses_back_into_the_same_compose_fields() {
        let path = std::env::temp_dir().join("flit-draft-roundtrip.txt");
        std::fs::write(&path, b"data").unwrap();
        let mut out = outgoing();
        out.to = "Ján Novák <jan@example.sk>, maria".to_string();
        out.bcc = "hidden@example.com".to_string();
        out.attachments = vec![AttachmentRef {
            path: path.to_string_lossy().into_owned(),
            name: "roundtrip.txt".to_string(),
        }];

        let raw = build_draft("", "domco@example.com", &out, "rt-id@flit.local")
            .await
            .unwrap();
        let parsed = crate::mail::parse::parse_draft(&raw);

        assert_eq!(parsed.from_addr.as_deref(), Some("domco@example.com"));
        assert_eq!(parsed.to, "Ján Novák <jan@example.sk>, maria");
        assert_eq!(parsed.bcc, "hidden@example.com");
        assert_eq!(parsed.subject, "Pozvánka na obed");
        assert_eq!(parsed.body.trim_end(), "Ahoj, prídeš?");
        assert_eq!(parsed.message_id.as_deref(), Some("rt-id@flit.local"));
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].name, "roundtrip.txt");
        assert_eq!(parsed.attachments[0].data, b"data");
    }

    #[tokio::test]
    async fn draft_from_carries_the_alias_identity() {
        let raw = build_draft("Igor", "igor@vocalio.sk", &outgoing(), "id@flit.local")
            .await
            .unwrap();

        let text = String::from_utf8(raw.clone()).unwrap();
        assert!(text.contains("igor@vocalio.sk"));
        assert!(text.contains("Igor"));
        // The round trip hands the bare address back for alias matching.
        let parsed = crate::mail::parse::parse_draft(&raw);
        assert_eq!(parsed.from_addr.as_deref(), Some("igor@vocalio.sk"));
    }

    #[tokio::test]
    async fn parse_draft_neutralizes_a_traversal_attachment_name() {
        // A foreign draft could carry "../../evil" as a filename; the parsed
        // name is later joined onto a temp dir and must not escape it.
        let raw = concat!(
            "Message-ID: <x@y>\r\n",
            "Subject: t\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"b\"\r\n",
            "\r\n",
            "--b\r\n",
            "Content-Type: text/plain\r\n\r\nhi\r\n",
            "--b\r\n",
            "Content-Type: application/octet-stream\r\n",
            "Content-Disposition: attachment; filename=\"../../evil\"\r\n",
            "\r\npayload\r\n",
            "--b--\r\n",
        );

        let parsed = crate::mail::parse::parse_draft(raw.as_bytes());

        assert_eq!(parsed.attachments.len(), 1);
        assert!(!parsed.attachments[0].name.contains('/'));
        assert!(!parsed.attachments[0].name.contains('\\'));
    }

    #[test]
    fn message_ids_are_unique_and_addressable() {
        let a = generate_message_id();
        let b = generate_message_id();

        assert_ne!(a, b);
        assert!(a.contains('@'));
        assert!(!a.contains('<') && !a.contains('>'));
    }
}
