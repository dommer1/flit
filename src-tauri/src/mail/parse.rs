use mail_parser::{Message, MessageParser};

/// Fields extracted from a message's headers.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedHeader {
    pub from: String,
    pub subject: String,
    /// RFC3339, or empty when the Date header is missing/unparsable —
    /// empty sorts last in the date-desc list instead of inventing a date.
    pub date: String,
}

/// Bodies extracted from a full message.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedBody {
    pub text: Option<String>,
    pub html: Option<String>,
    pub snippet: String,
}

/// Parse raw header bytes (from `BODY.PEEK[HEADER]`).
pub fn parse_header(raw: &[u8]) -> ParsedHeader {
    let Some(message) = MessageParser::default().parse(raw) else {
        return ParsedHeader::default();
    };
    ParsedHeader {
        from: format_from(&message),
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
    ParsedBody {
        text,
        html,
        snippet,
    }
}

/// "Name <addr>" like mail clients show it, degrading to whichever part exists.
fn format_from(message: &Message) -> String {
    let Some(addr) = message.from().and_then(|a| a.first()) else {
        return String::new();
    };
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
    fn snippet_collapses_whitespace_and_truncates() {
        let long = format!("a  b\r\n\tc {}", "x".repeat(200));

        let snippet = snippet_of(&long);

        assert!(snippet.starts_with("a b c x"));
        assert_eq!(snippet.chars().count(), 120);
    }
}
