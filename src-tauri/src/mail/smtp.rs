//! SECURITY-RELEVANT: SMTP TLS setup lives here. Strict certificate
//! validation via lettre's native-tls defaults — never disable it.

use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, Mailboxes, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::error::AppError;
use crate::models::{AttachmentRef, OutgoingMessage};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
// why: a send uploads the whole message after connecting, so it gets more
// headroom than the plain connection check.
const SEND_TIMEOUT: Duration = Duration::from_secs(60);

/// Port 465 speaks TLS from the first byte (SMTPS); everything else starts
/// plain and upgrades via STARTTLS before any credentials are sent.
pub fn uses_implicit_tls(port: u16) -> bool {
    port == 465
}

/// Authenticated TLS transport to the account's SMTP server — the single
/// construction path for both the connection check and real sends.
fn transport(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, AppError> {
    let builder = if uses_implicit_tls(port) {
        AsyncSmtpTransport::<Tokio1Executor>::relay(host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
    }
    .map_err(|e| AppError::Smtp(e.to_string()))?;

    Ok(builder
        .port(port)
        .credentials(Credentials::new(username.to_string(), password.to_string()))
        .build())
}

/// Whether this SMTP server stores its own copy of every sent message.
/// Gmail does (any authenticated send lands in "Sent Mail" server-side), so
/// appending our own copy would duplicate it there.
pub fn server_saves_sent_copy(smtp_host: &str) -> bool {
    let host = smtp_host.to_ascii_lowercase();
    host == "gmail.com"
        || host.ends_with(".gmail.com")
        || host == "googlemail.com"
        || host.ends_with(".googlemail.com")
}

/// Most providers cap incoming messages around 25 MB; failing above that
/// locally beats an opaque server rejection after the upload.
const MAX_ATTACHMENT_TOTAL: usize = 25 * 1024 * 1024;

/// Reject attachment payloads the receiving server would bounce anyway.
/// pub(crate): the draft path enforces the same budget before an APPEND.
pub(crate) fn ensure_attachment_budget(total: usize) -> Result<(), AppError> {
    if total > MAX_ATTACHMENT_TOTAL {
        return Err(AppError::Smtp(
            "attachments exceed the 25 MB limit".to_string(),
        ));
    }
    Ok(())
}

/// Parse one comma-separated address list; empty input is an empty list.
fn parse_recipients(list: &str, field: &str) -> Result<Vec<Mailbox>, AppError> {
    if list.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(list
        .parse::<Mailboxes>()
        .map_err(|e| AppError::Smtp(format!("invalid {field} recipient: {e}")))?
        .into_iter()
        .collect())
}

/// Build the MIME message for an outgoing mail. `to`/`cc`/`bcc` accept
/// comma-separated recipient lists; the body is plain text for now.
// why async: attachment bytes will be read from disk here — file I/O must
// not block the runtime, so the signature is async ahead of that change.
pub async fn build_message(
    from_email: &str,
    outgoing: &OutgoingMessage,
) -> Result<Message, AppError> {
    let from: Mailbox = from_email
        .parse()
        .map_err(|e| AppError::Smtp(format!("invalid from address: {e}")))?;
    let to = parse_recipients(&outgoing.to, "to")?;
    if to.is_empty() {
        return Err(AppError::Smtp("no recipient given".to_string()));
    }

    let mut builder = Message::builder().from(from).subject(&outgoing.subject);
    // why: .to()/.cc()/.bcc() append to their header on every call —
    // lettre's way of setting multiple recipients without hand-building
    // the header.
    for recipient in to {
        builder = builder.to(recipient);
    }
    for recipient in parse_recipients(&outgoing.cc, "cc")? {
        builder = builder.cc(recipient);
    }
    // SECURITY: no .keep_bcc() here. lettre derives the SMTP envelope
    // (RCPT TO) from all recipients, then strips the Bcc header from the
    // formatted bytes — so hidden recipients get the mail but are never
    // named in what other recipients (or the Sent copy) see.
    for recipient in parse_recipients(&outgoing.bcc, "bcc")? {
        builder = builder.bcc(recipient);
    }

    if outgoing.attachments.is_empty() {
        return builder
            .header(ContentType::TEXT_PLAIN)
            .body(outgoing.body.clone())
            .map_err(|e| AppError::Smtp(e.to_string()));
    }

    // multipart/mixed: the typed text first, then one part per file.
    let mut parts = MultiPart::mixed().singlepart(SinglePart::plain(outgoing.body.clone()));
    let mut total = 0usize;
    for attachment in &outgoing.attachments {
        parts = parts.singlepart(load_attachment(attachment, &mut total).await?);
    }
    builder
        .multipart(parts)
        .map_err(|e| AppError::Smtp(e.to_string()))
}

/// Read one attachment from disk into a MIME part, keeping the running
/// size total honest against the budget.
async fn load_attachment(
    attachment: &AttachmentRef,
    total: &mut usize,
) -> Result<SinglePart, AppError> {
    let bytes = tokio::fs::read(&attachment.path)
        .await
        .map_err(|e| AppError::Smtp(format!("cannot read attachment {}: {e}", attachment.name)))?;
    *total += bytes.len();
    ensure_attachment_budget(*total)?;
    // why guess from `name`, not `path`: the recipient only ever sees the
    // filename, so the advertised type must match what they can see.
    let mime = mime_guess::from_path(&attachment.name).first_or_octet_stream();
    let content_type = ContentType::parse(mime.essence_str())
        .map_err(|e| AppError::Smtp(format!("bad content type for {}: {e}", attachment.name)))?;
    Ok(Attachment::new(attachment.name.clone()).body(bytes, content_type))
}

/// Send one built message through the account's SMTP server.
pub async fn send(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    message: Message,
) -> Result<(), AppError> {
    let transport = transport(host, port, username, password)?;
    tokio::time::timeout(SEND_TIMEOUT, transport.send(message))
        .await
        .map_err(|_| AppError::Smtp(format!("sending via {host}:{port} timed out")))?
        .map_err(|e| AppError::Smtp(e.to_string()))?;
    Ok(())
}

/// Connection check for the add-account flow: connect, TLS, EHLO, AUTH, NOOP.
pub async fn verify(host: &str, port: u16, username: &str, password: &str) -> Result<(), AppError> {
    let transport = transport(host, port, username, password)?;

    // why: test_connection authenticates while establishing the pooled
    // connection, so bad credentials fail here — exactly what Verify & Save
    // wants. The timeout guards against black-holed servers.
    let healthy = tokio::time::timeout(CONNECT_TIMEOUT, transport.test_connection())
        .await
        .map_err(|_| AppError::Smtp(format!("connection to {host}:{port} timed out")))?
        .map_err(|e| AppError::Smtp(e.to_string()))?;
    if healthy {
        Ok(())
    } else {
        Err(AppError::Smtp(
            "server refused the connection test".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_465_is_implicit_tls() {
        assert!(uses_implicit_tls(465));
    }

    #[test]
    fn submission_ports_use_starttls() {
        assert!(!uses_implicit_tls(587));
        assert!(!uses_implicit_tls(25));
    }

    #[test]
    fn gmail_hosts_save_their_own_sent_copy() {
        assert!(server_saves_sent_copy("smtp.gmail.com"));
        assert!(server_saves_sent_copy("SMTP.Gmail.com"));
        assert!(server_saves_sent_copy("smtp.googlemail.com"));
    }

    #[test]
    fn other_hosts_need_an_explicit_sent_copy() {
        assert!(!server_saves_sent_copy("smtp.example.com"));
        assert!(!server_saves_sent_copy("smtp.mail.me.com"));
        // A lookalike suffix must not match.
        assert!(!server_saves_sent_copy("smtp.notgmail.com"));
    }

    fn outgoing(to: &str) -> OutgoingMessage {
        OutgoingMessage {
            account_id: 1,
            to: to.to_string(),
            cc: String::new(),
            bcc: String::new(),
            subject: "Hello".to_string(),
            body: "Hi there".to_string(),
            attachments: Vec::new(),
        }
    }

    /// Write a unique temp file and return an AttachmentRef pointing at it.
    fn temp_attachment(name: &str, contents: &[u8]) -> AttachmentRef {
        let path = std::env::temp_dir().join(format!("flit-smtp-test-{name}"));
        std::fs::write(&path, contents).unwrap();
        AttachmentRef {
            path: path.to_string_lossy().into_owned(),
            name: name.to_string(),
        }
    }

    #[tokio::test]
    async fn builds_a_plain_text_message() {
        let message = build_message("domco@example.com", &outgoing("alice@example.com"))
            .await
            .unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(raw.contains("From: domco@example.com"));
        assert!(raw.contains("To: alice@example.com"));
        assert!(raw.contains("Subject: Hello"));
        assert!(raw.contains("Hi there"));
    }

    #[tokio::test]
    async fn accepts_a_comma_separated_recipient_list() {
        let message = build_message(
            "domco@example.com",
            &outgoing("alice@example.com, Bob <bob@example.com>"),
        )
        .await
        .unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(raw.contains("alice@example.com"));
        assert!(raw.contains("bob@example.com"));
    }

    #[tokio::test]
    async fn cc_recipients_land_in_the_cc_header() {
        let mut out = outgoing("alice@example.com");
        out.cc = "Carol <carol@example.com>, dan@example.com".to_string();

        let message = build_message("domco@example.com", &out).await.unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(raw.contains("Cc:"));
        assert!(raw.contains("carol@example.com"));
        assert!(raw.contains("dan@example.com"));
    }

    #[tokio::test]
    async fn bcc_reaches_the_envelope_but_never_the_headers() {
        let mut out = outgoing("alice@example.com");
        out.bcc = "hidden@example.com".to_string();

        let message = build_message("domco@example.com", &out).await.unwrap();

        // The transmitted bytes (also the Sent copy) must not name the
        // hidden recipient anywhere.
        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(!raw.contains("hidden@example.com"));
        // …but SMTP must still deliver to them via RCPT TO.
        let envelope: Vec<String> = message
            .envelope()
            .to()
            .iter()
            .map(|a| a.to_string())
            .collect();
        assert!(envelope.contains(&"hidden@example.com".to_string()));
        assert!(envelope.contains(&"alice@example.com".to_string()));
    }

    #[tokio::test]
    async fn plain_messages_stay_single_part() {
        let message = build_message("domco@example.com", &outgoing("alice@example.com"))
            .await
            .unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(!raw.contains("multipart/mixed"));
    }

    #[tokio::test]
    async fn attachments_produce_a_multipart_mixed_message() {
        // Non-ASCII bytes force lettre's encoder to base64, so the exact
        // transported form is predictable.
        let contents: &[u8] = b"%PDF-1.4\x00\xff binary";
        let mut out = outgoing("alice@example.com");
        out.attachments = vec![temp_attachment("report.pdf", contents)];

        let message = build_message("domco@example.com", &out).await.unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains("Content-Disposition: attachment; filename=\"report.pdf\""));
        assert!(raw.contains("application/pdf"));
        // The typed text still travels alongside the attachment…
        assert!(raw.contains("Hi there"));
        // …and the file bytes go out base64-encoded.
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(contents);
        assert!(raw.contains(&encoded));
    }

    #[tokio::test]
    async fn unknown_extensions_fall_back_to_octet_stream() {
        let mut out = outgoing("alice@example.com");
        out.attachments = vec![temp_attachment("data.flitblob", b"\x00\x01\x02")];

        let message = build_message("domco@example.com", &out).await.unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(raw.contains("application/octet-stream"));
    }

    #[tokio::test]
    async fn missing_attachment_file_fails_the_build() {
        let mut out = outgoing("alice@example.com");
        out.attachments = vec![AttachmentRef {
            path: "/nonexistent/flit/gone.txt".to_string(),
            name: "gone.txt".to_string(),
        }];

        let err = build_message("domco@example.com", &out).await.unwrap_err();

        assert!(err.to_string().contains("gone.txt"));
    }

    #[test]
    fn attachment_budget_has_a_hard_ceiling() {
        assert!(ensure_attachment_budget(MAX_ATTACHMENT_TOTAL).is_ok());
        assert!(ensure_attachment_budget(MAX_ATTACHMENT_TOTAL + 1).is_err());
    }

    #[tokio::test]
    async fn rejects_an_invalid_cc_or_bcc() {
        let mut out = outgoing("alice@example.com");
        out.cc = "not-an-address".to_string();
        assert!(build_message("domco@example.com", &out)
            .await
            .unwrap_err()
            .to_string()
            .contains("cc"));

        let mut out = outgoing("alice@example.com");
        out.bcc = "also bad".to_string();
        assert!(build_message("domco@example.com", &out)
            .await
            .unwrap_err()
            .to_string()
            .contains("bcc"));
    }

    #[tokio::test]
    async fn rejects_an_invalid_recipient() {
        let err = build_message("domco@example.com", &outgoing("not-an-address"))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("recipient"));
    }

    #[tokio::test]
    async fn rejects_an_empty_recipient_list() {
        let err = build_message("domco@example.com", &outgoing("   "))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("recipient"));
    }
}
