use mail_parser::{Addr, Address, HeaderValue, Message, MessageParser, MimeHeaders};

/// Fields extracted from a message's headers.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedHeader {
    pub from: String,
    /// To recipients, comma-separated.
    pub to: String,
    /// Cc recipients, comma-separated — kept apart from `to` so a reply-all
    /// can rebuild the original To/Cc split.
    pub cc: String,
    /// Reply-To recipients, comma-separated; empty when the header is absent.
    /// Shown in the message detail so the user sees where a reply would go.
    pub reply_to: String,
    pub subject: String,
    /// RFC3339, or empty when the Date header is missing/unparsable —
    /// empty sorts last in the date-desc list instead of inventing a date.
    pub date: String,
    /// Message-ID without angle brackets; empty when the sender set none.
    pub message_id: String,
    /// The Message-ID this message replies to; empty for non-replies.
    pub in_reply_to: String,
    /// The References chain (oldest first) — every Message-ID this message
    /// descends from. Threading unions these with `in_reply_to`.
    pub references: Vec<String>,
}

/// An image embedded in the message itself and referenced from its HTML via
/// `src="cid:..."` (RFC 2392). Part of the message — displaying it leaks
/// nothing, unlike remote images.
#[derive(Debug, PartialEq)]
pub struct InlineImage {
    /// Content-ID without the surrounding angle brackets, as cid: URLs use it.
    pub content_id: String,
    /// Full MIME type ("image/png") — becomes the data: URI media type.
    pub content_type: String,
    /// Decoded bytes (mail-parser undoes the transfer encoding).
    pub data: Vec<u8>,
}

/// A non-inline attachment of a message: metadata only — the bytes stay on
/// the server and are re-fetched on demand when the user saves the file.
#[derive(Debug, PartialEq)]
pub struct AttachmentMeta {
    /// Position within mail-parser's attachment enumeration — the key
    /// `attachment_data` uses to re-extract this part from a fresh fetch
    /// of the same raw message.
    pub part_index: i64,
    pub filename: String,
    /// Full MIME type ("application/pdf"); octet-stream when unspecified.
    pub content_type: String,
    /// Decoded size in bytes.
    pub size: i64,
}

/// Bodies extracted from a full message.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedBody {
    pub text: Option<String>,
    pub html: Option<String>,
    pub snippet: String,
    pub images: Vec<InlineImage>,
    pub attachments: Vec<AttachmentMeta>,
}

/// Parse raw header bytes (from `BODY.PEEK[HEADER]`).
pub fn parse_header(raw: &[u8]) -> ParsedHeader {
    let Some(message) = MessageParser::default().parse(raw) else {
        return ParsedHeader::default();
    };
    ParsedHeader {
        from: format_from(&message),
        to: format_addr_list(message.to()),
        cc: format_addr_list(message.cc()),
        reply_to: format_addr_list(message.reply_to()),
        subject: message.subject().unwrap_or_default().to_string(),
        date: message.date().map(|d| d.to_rfc3339()).unwrap_or_default(),
        message_id: clean_id(message.message_id().unwrap_or_default()),
        in_reply_to: id_list(message.in_reply_to())
            .into_iter()
            .next()
            .unwrap_or_default(),
        references: id_list(message.references()),
    }
}

/// A Message-ID normalized for comparison: angle brackets and whitespace
/// stripped. mail-parser usually removes the brackets already; this guards
/// against senders that nest or double them.
fn clean_id(id: &str) -> String {
    id.trim().trim_matches(['<', '>']).to_string()
}

/// The Message-IDs of an In-Reply-To / References header, in order.
/// mail-parser hands single-id headers back as Text and multi-id ones as
/// TextList — anything else means the header is absent or malformed.
fn id_list(value: &HeaderValue) -> Vec<String> {
    match value {
        HeaderValue::Text(id) => vec![clean_id(id)],
        HeaderValue::TextList(ids) => ids.iter().map(|id| clean_id(id)).collect(),
        _ => Vec::new(),
    }
}

/// Parse a full raw message (from `BODY.PEEK[]`).
///
/// why: mail-parser converts between representations when one part is
/// missing (html-only mail still yields text), so both fields are usually
/// present and the snippet can always come from the text form.
pub fn parse_body(raw: &[u8]) -> ParsedBody {
    let Some(message) = MessageParser::default().parse(raw) else {
        return ParsedBody::default();
    };
    let text = message.body_text(0).map(|t| t.into_owned());
    let html = message.body_html(0).map(|h| h.into_owned());
    let snippet = text.as_deref().map(snippet_of).unwrap_or_default();
    let images = inline_images(&message);
    let attachments = attachment_meta(&message);
    ParsedBody {
        text,
        html,
        snippet,
        images,
        attachments,
    }
}

/// Whether this part renders inline via a `cid:` reference — those live in
/// `images`, not in the attachment list.
fn is_inline_image(part: &mail_parser::MessagePart) -> bool {
    part.content_id().is_some()
        && part
            .content_type()
            .is_some_and(|ct| ct.ctype().eq_ignore_ascii_case("image"))
}

/// Metadata of every attachment worth listing: anything mail-parser treats
/// as an attachment except the inline cid: images already rendered in the
/// body. Enumerated before filtering so `part_index` stays aligned with
/// mail-parser's `attachments()` order for later re-extraction.
fn attachment_meta(message: &Message) -> Vec<AttachmentMeta> {
    message
        .attachments()
        .enumerate()
        .filter(|(_, part)| !is_inline_image(part))
        .map(|(index, part)| AttachmentMeta {
            part_index: index as i64,
            filename: part.attachment_name().unwrap_or("attachment").to_string(),
            content_type: part
                .content_type()
                .map(|ct| match ct.subtype() {
                    Some(subtype) => format!("{}/{subtype}", ct.ctype()).to_ascii_lowercase(),
                    None => ct.ctype().to_ascii_lowercase(),
                })
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            size: part.contents().len() as i64,
        })
        .collect()
}

/// Decoded bytes of one attachment, re-extracted from a raw message by the
/// `part_index` recorded when its metadata was cached. `None` when the
/// message no longer parses or the part vanished (index out of range).
pub fn attachment_data(raw: &[u8], part_index: i64) -> Option<Vec<u8>> {
    let message = MessageParser::default().parse(raw)?;
    let part = message
        .attachments()
        .nth(usize::try_from(part_index).ok()?)?;
    Some(part.contents().to_vec())
}

/// A filename from mail headers made safe to create inside a chosen
/// directory: path separators neutralized, dot-only names replaced. Save All
/// writes `dir/<this>`, so a crafted name must never escape `dir`.
pub fn safe_filename(name: &str) -> String {
    let cleaned = name.replace(['/', '\\'], "_");
    let trimmed = cleaned.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "attachment".to_string()
    } else {
        trimmed.to_string()
    }
}

/// One attachment of a reopened draft: filename plus decoded bytes, ready
/// to be re-staged as a local temp file for the compose window.
#[derive(Debug, PartialEq)]
pub struct DraftAttachment {
    pub name: String,
    pub data: Vec<u8>,
}

/// The compose-shaped fields of a draft message being reopened for editing.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedDraft {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    /// Message-ID without angle brackets — the handle under which the next
    /// save replaces this server version.
    pub message_id: Option<String>,
    /// Threading identity of a reply draft, as OutgoingMessage carries it:
    /// the answered Message-ID and the space-joined ancestor chain.
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub attachments: Vec<DraftAttachment>,
}

/// Parse a raw draft back into compose fields — the inverse of
/// `mail::draft::build_draft`. Bcc and half-typed recipients come back
/// verbatim; an HTML-only body degrades to mail-parser's text rendering
/// (the compose editor takes plain text).
pub fn parse_draft(raw: &[u8]) -> ParsedDraft {
    let Some(message) = MessageParser::default().parse(raw) else {
        return ParsedDraft::default();
    };
    ParsedDraft {
        to: format_addr_list(message.to()),
        cc: format_addr_list(message.cc()),
        bcc: format_addr_list(message.bcc()),
        subject: message.subject().unwrap_or_default().to_string(),
        body: message
            .body_text(0)
            .map(|t| t.into_owned())
            .unwrap_or_default(),
        message_id: message.message_id().map(str::to_string),
        in_reply_to: id_list(message.in_reply_to()).into_iter().next(),
        references: {
            let refs = id_list(message.references());
            (!refs.is_empty()).then(|| refs.join(" "))
        },
        attachments: message
            .attachments()
            .filter(|part| !is_inline_image(part))
            .map(|part| DraftAttachment {
                name: safe_filename(part.attachment_name().unwrap_or("attachment")),
                data: part.contents().to_vec(),
            })
            .collect(),
    }
}

/// Every attachment that an `<img src="cid:...">` could reference: an image
/// part carrying a Content-ID. Anything else (no id, not an image) can't
/// render inline and is left for a future attachment list.
fn inline_images(message: &Message) -> Vec<InlineImage> {
    message
        .attachments()
        .filter_map(|part| {
            let content_id = part.content_id()?;
            let content_type = part.content_type()?;
            if !content_type.ctype().eq_ignore_ascii_case("image") {
                return None;
            }
            let subtype = content_type.subtype()?.to_ascii_lowercase();
            Some(InlineImage {
                // why: defensive trim — mail-parser strips the <> brackets,
                // but a stray pair must never break cid lookup.
                content_id: content_id.trim_matches(['<', '>']).to_string(),
                content_type: format!("image/{subtype}"),
                data: part.contents().to_vec(),
            })
        })
        .collect()
}

/// "Name <addr>" like mail clients show it, degrading to whichever part exists.
fn format_from(message: &Message) -> String {
    message
        .from()
        .and_then(|a| a.first())
        .map(format_addr)
        .unwrap_or_default()
}

/// One address list header, comma-separated.
fn format_addr_list(list: Option<&Address<'_>>) -> String {
    list.into_iter()
        .flat_map(|a| a.iter())
        .map(format_addr)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_addr(addr: &Addr) -> String {
    let email = addr.address().unwrap_or_default();
    match addr.name() {
        Some(name) if !email.is_empty() => format!("{name} <{email}>"),
        Some(name) => name.to_string(),
        None => email.to_string(),
    }
}

/// First ~120 chars of the text with whitespace collapsed.
///
/// why: transactional mail often leads with a bare URL (a "view in browser"
/// or logo link), frequently wrapped in `<...>`. Left in, the preview shows a
/// wall of tracking query params instead of prose, so URL tokens are dropped.
pub fn snippet_of(text: &str) -> String {
    let collapsed = text
        .split_whitespace()
        .filter(|word| !is_url_token(word))
        .collect::<Vec<_>>()
        .join(" ");
    collapsed.chars().take(120).collect()
}

/// A whitespace-delimited token that is just a URL, optionally wrapped in the
/// RFC 3676 angle brackets senders use to delimit bare links in plain text.
fn is_url_token(word: &str) -> bool {
    let unwrapped = word.trim_start_matches('<').trim_end_matches('>');
    unwrapped.starts_with("http://") || unwrapped.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_headers() {
        let raw = b"From: Alice Novak <alice@example.com>\r\n\
                    Subject: Weekend plans\r\n\
                    Date: Tue, 07 Jul 2026 09:15:00 +0000\r\n\
                    \r\n";

        let header = parse_header(raw);

        assert_eq!(header.from, "Alice Novak <alice@example.com>");
        assert_eq!(header.subject, "Weekend plans");
        assert!(header.date.starts_with("2026-07-07T09:15:00"));
        assert_eq!(header.to, "");
        assert_eq!(header.cc, "");
    }

    #[test]
    fn keeps_to_and_cc_recipients_apart() {
        let raw = b"From: a@example.com\r\n\
                    To: Bob <bob@example.com>, carol@example.com\r\n\
                    Cc: Dana <dana@example.com>\r\n\
                    Subject: Hi\r\n\
                    \r\n";

        let header = parse_header(raw);

        assert_eq!(header.to, "Bob <bob@example.com>, carol@example.com");
        assert_eq!(header.cc, "Dana <dana@example.com>");
        assert_eq!(header.reply_to, "");
    }

    #[test]
    fn parses_reply_to_when_present() {
        let raw = b"From: Newsletter <no-reply@example.com>\r\n\
                    Reply-To: Support <support@example.com>\r\n\
                    Subject: Hi\r\n\
                    \r\n";

        assert_eq!(parse_header(raw).reply_to, "Support <support@example.com>");
    }

    #[test]
    fn parses_threading_headers() {
        let raw = b"From: a@example.com\r\n\
                    Subject: Re: Hi\r\n\
                    Message-ID: <reply-1@example.com>\r\n\
                    In-Reply-To: <mid@example.com>\r\n\
                    References: <root@example.com> <mid@example.com>\r\n\
                    \r\n";

        let header = parse_header(raw);

        assert_eq!(header.message_id, "reply-1@example.com");
        assert_eq!(header.in_reply_to, "mid@example.com");
        assert_eq!(
            header.references,
            vec![
                "root@example.com".to_string(),
                "mid@example.com".to_string()
            ]
        );
    }

    #[test]
    fn missing_threading_headers_yield_empty_values() {
        let raw = b"From: a@example.com\r\nSubject: Hi\r\n\r\n";

        let header = parse_header(raw);

        assert_eq!(header.message_id, "");
        assert_eq!(header.in_reply_to, "");
        assert_eq!(header.references, Vec::<String>::new());
    }

    #[test]
    fn single_reference_still_parses_as_list() {
        let raw = b"From: a@example.com\r\n\
                    Subject: Re: Hi\r\n\
                    References: <root@example.com>\r\n\
                    \r\n";

        assert_eq!(
            parse_header(raw).references,
            vec!["root@example.com".to_string()]
        );
    }

    #[test]
    fn decodes_rfc2047_subject() {
        let raw = b"From: a@example.com\r\n\
                    Subject: =?utf-8?B?QWhvaiBzdmV0?=\r\n\
                    Date: Tue, 07 Jul 2026 09:15:00 +0000\r\n\
                    \r\n";

        assert_eq!(parse_header(raw).subject, "Ahoj svet");
    }

    #[test]
    fn missing_date_yields_empty_string() {
        let raw = b"From: a@example.com\r\nSubject: Hi\r\n\r\n";

        assert_eq!(parse_header(raw).date, "");
    }

    #[test]
    fn parses_multipart_text_and_html() {
        let raw = b"From: a@example.com\r\n\
                    Subject: Multi\r\n\
                    MIME-Version: 1.0\r\n\
                    Content-Type: multipart/alternative; boundary=\"b1\"\r\n\
                    \r\n\
                    --b1\r\n\
                    Content-Type: text/plain; charset=utf-8\r\n\
                    \r\n\
                    Hello   plain world\r\n\
                    --b1\r\n\
                    Content-Type: text/html; charset=utf-8\r\n\
                    \r\n\
                    <p>Hello <b>html</b> world</p>\r\n\
                    --b1--\r\n";

        let body = parse_body(raw);

        assert!(body.text.as_deref().unwrap().contains("Hello"));
        assert!(body.html.as_deref().unwrap().contains("<b>html</b>"));
        assert_eq!(body.snippet, "Hello plain world");
    }

    #[test]
    fn html_only_mail_still_yields_text_and_snippet() {
        let raw = b"From: a@example.com\r\n\
                    Subject: HtmlOnly\r\n\
                    MIME-Version: 1.0\r\n\
                    Content-Type: text/html; charset=utf-8\r\n\
                    \r\n\
                    <p>Only html here</p>\r\n";

        let body = parse_body(raw);

        assert!(body.html.as_deref().unwrap().contains("Only html here"));
        assert!(body.text.as_deref().unwrap().contains("Only html here"));
        assert!(body.snippet.contains("Only html here"));
    }

    #[test]
    fn extracts_inline_cid_images() {
        let raw = b"From: a@example.com\r\n\
                    Subject: Pics\r\n\
                    MIME-Version: 1.0\r\n\
                    Content-Type: multipart/related; boundary=\"b1\"\r\n\
                    \r\n\
                    --b1\r\n\
                    Content-Type: text/html; charset=utf-8\r\n\
                    \r\n\
                    <p>Look: <img src=\"cid:photo1\"></p>\r\n\
                    --b1\r\n\
                    Content-Type: image/png\r\n\
                    Content-Transfer-Encoding: base64\r\n\
                    Content-ID: <photo1>\r\n\
                    Content-Disposition: inline; filename=\"p.png\"\r\n\
                    \r\n\
                    iVBORw0KGgo=\r\n\
                    --b1--\r\n";

        let body = parse_body(raw);

        assert_eq!(body.images.len(), 1);
        assert_eq!(body.images[0].content_id, "photo1");
        assert_eq!(body.images[0].content_type, "image/png");
        // base64 of the 8-byte PNG signature, decoded by mail-parser.
        assert_eq!(body.images[0].data, b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn skips_attachments_that_are_not_referencable_images() {
        let raw = b"From: a@example.com\r\n\
                    Subject: Files\r\n\
                    MIME-Version: 1.0\r\n\
                    Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
                    \r\n\
                    --b1\r\n\
                    Content-Type: text/html; charset=utf-8\r\n\
                    \r\n\
                    <p>See attached</p>\r\n\
                    --b1\r\n\
                    Content-Type: application/pdf\r\n\
                    Content-ID: <doc1>\r\n\
                    \r\n\
                    %PDF-fake\r\n\
                    --b1\r\n\
                    Content-Type: image/jpeg\r\n\
                    \r\n\
                    not-referencable-without-a-content-id\r\n\
                    --b1--\r\n";

        // A PDF can't render in an <img>; an image without a Content-ID can't
        // be referenced by any cid: URL — neither belongs in `images`.
        assert_eq!(parse_body(raw).images, Vec::new());
    }

    // A multipart/mixed message with an HTML body, a PDF attachment, an
    // inline cid: image and an image attachment without a Content-ID.
    const RAW_WITH_ATTACHMENTS: &[u8] = b"From: a@example.com\r\n\
        Subject: Files\r\n\
        MIME-Version: 1.0\r\n\
        Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
        \r\n\
        --b1\r\n\
        Content-Type: text/html; charset=utf-8\r\n\
        \r\n\
        <p>See attached <img src=\"cid:logo\"></p>\r\n\
        --b1\r\n\
        Content-Type: application/pdf\r\n\
        Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
        \r\n\
        %PDF-fake\r\n\
        --b1\r\n\
        Content-Type: image/png\r\n\
        Content-Transfer-Encoding: base64\r\n\
        Content-ID: <logo>\r\n\
        Content-Disposition: inline; filename=\"logo.png\"\r\n\
        \r\n\
        iVBORw0KGgo=\r\n\
        --b1\r\n\
        Content-Type: image/jpeg\r\n\
        Content-Disposition: attachment; filename=\"photo.jpg\"\r\n\
        \r\n\
        JPEGDATA\r\n\
        --b1--\r\n";

    #[test]
    fn lists_attachment_metadata_without_inline_images() {
        let body = parse_body(RAW_WITH_ATTACHMENTS);

        // The cid: logo renders inline — it belongs to images, not here.
        assert_eq!(body.images.len(), 1);
        assert_eq!(body.attachments.len(), 2);

        let pdf = &body.attachments[0];
        assert_eq!(pdf.filename, "report.pdf");
        assert_eq!(pdf.content_type, "application/pdf");
        assert_eq!(pdf.size, "%PDF-fake".len() as i64);

        let photo = &body.attachments[1];
        assert_eq!(photo.filename, "photo.jpg");
        assert_eq!(photo.content_type, "image/jpeg");
    }

    #[test]
    fn attachment_data_re_extracts_bytes_by_part_index() {
        let body = parse_body(RAW_WITH_ATTACHMENTS);

        let pdf = attachment_data(RAW_WITH_ATTACHMENTS, body.attachments[0].part_index);
        assert_eq!(pdf.as_deref(), Some(b"%PDF-fake".as_slice()));

        // The photo's index skips over the inline logo in between.
        let photo = attachment_data(RAW_WITH_ATTACHMENTS, body.attachments[1].part_index);
        assert_eq!(photo.as_deref(), Some(b"JPEGDATA".as_slice()));

        assert_eq!(attachment_data(RAW_WITH_ATTACHMENTS, 99), None);
        assert_eq!(attachment_data(b"not mail", 0), None);
    }

    #[test]
    fn attachment_without_headers_gets_fallback_name_and_type() {
        let raw = b"From: a@example.com\r\n\
            Subject: Blob\r\n\
            MIME-Version: 1.0\r\n\
            Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
            \r\n\
            --b1\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            see attachment\r\n\
            --b1\r\n\
            Content-Disposition: attachment\r\n\
            \r\n\
            rawbytes\r\n\
            --b1--\r\n";

        let body = parse_body(raw);

        assert_eq!(body.attachments.len(), 1);
        assert_eq!(body.attachments[0].filename, "attachment");
        assert!(!body.attachments[0].content_type.is_empty());
    }

    #[test]
    fn safe_filename_neutralizes_paths_and_dot_names() {
        assert_eq!(safe_filename("report.pdf"), "report.pdf");
        assert_eq!(safe_filename("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(safe_filename("C:\\boot.ini"), "C:_boot.ini");
        assert_eq!(safe_filename(".."), "attachment");
        assert_eq!(safe_filename("."), "attachment");
        assert_eq!(safe_filename("  "), "attachment");
        assert_eq!(safe_filename(""), "attachment");
    }

    #[test]
    fn snippet_collapses_whitespace_and_truncates() {
        let long = format!("a  b\r\n\tc {}", "x".repeat(200));

        let snippet = snippet_of(&long);

        assert!(snippet.starts_with("a b c x"));
        assert_eq!(snippet.chars().count(), 120);
    }

    #[test]
    fn snippet_drops_bare_and_bracketed_urls() {
        // A Freelo-style transactional body: a leading "open it" link (wrapped
        // in <...>) followed by the human-readable prose we actually want.
        let text = "<https://app.freelo.io/dashboard/?utm_source=transactional&utm_medium=email> \
                    Pavlo assigned you to a task. Open here: https://app.freelo.io/task/42";

        let snippet = snippet_of(text);

        assert_eq!(snippet, "Pavlo assigned you to a task. Open here:");
    }
}
