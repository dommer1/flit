use mail_parser::{Addr, Address, Message, MessageParser, MimeHeaders};

/// Fields extracted from a message's headers.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedHeader {
    pub from: String,
    /// To recipients, comma-separated.
    pub to: String,
    /// Cc recipients, comma-separated — kept apart from `to` so a reply-all
    /// can rebuild the original To/Cc split.
    pub cc: String,
    pub subject: String,
    /// RFC3339, or empty when the Date header is missing/unparsable —
    /// empty sorts last in the date-desc list instead of inventing a date.
    pub date: String,
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

/// Bodies extracted from a full message.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedBody {
    pub text: Option<String>,
    pub html: Option<String>,
    pub snippet: String,
    pub images: Vec<InlineImage>,
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
        subject: message.subject().unwrap_or_default().to_string(),
        date: message.date().map(|d| d.to_rfc3339()).unwrap_or_default(),
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
    ParsedBody {
        text,
        html,
        snippet,
        images,
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
pub fn snippet_of(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(120).collect()
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

    #[test]
    fn snippet_collapses_whitespace_and_truncates() {
        let long = format!("a  b\r\n\tc {}", "x".repeat(200));

        let snippet = snippet_of(&long);

        assert!(snippet.starts_with("a b c x"));
        assert_eq!(snippet.chars().count(), 120);
    }
}
