//! SECURITY-RELEVANT: SMTP TLS setup lives here. Strict certificate
//! validation via lettre's native-tls defaults — never disable it.

use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::{Mailbox, Mailboxes};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::error::AppError;
use crate::models::OutgoingMessage;

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
pub fn build_message(from_email: &str, outgoing: &OutgoingMessage) -> Result<Message, AppError> {
    let from: Mailbox = from_email
        .parse()
        .map_err(|e| AppError::Smtp(format!("invalid from address: {e}")))?;
    let to = parse_recipients(&outgoing.to, "to")?;
    if to.is_empty() {
        return Err(AppError::Smtp("no recipient given".to_string()));
    }

    let mut builder = Message::builder()
        .from(from)
        .subject(&outgoing.subject)
        .header(ContentType::TEXT_PLAIN);
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
    builder
        .body(outgoing.body.clone())
        .map_err(|e| AppError::Smtp(e.to_string()))
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
        }
    }

    #[test]
    fn builds_a_plain_text_message() {
        let message = build_message("domco@example.com", &outgoing("alice@example.com")).unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(raw.contains("From: domco@example.com"));
        assert!(raw.contains("To: alice@example.com"));
        assert!(raw.contains("Subject: Hello"));
        assert!(raw.contains("Hi there"));
    }

    #[test]
    fn accepts_a_comma_separated_recipient_list() {
        let message = build_message(
            "domco@example.com",
            &outgoing("alice@example.com, Bob <bob@example.com>"),
        )
        .unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(raw.contains("alice@example.com"));
        assert!(raw.contains("bob@example.com"));
    }

    #[test]
    fn cc_recipients_land_in_the_cc_header() {
        let mut out = outgoing("alice@example.com");
        out.cc = "Carol <carol@example.com>, dan@example.com".to_string();

        let message = build_message("domco@example.com", &out).unwrap();

        let raw = String::from_utf8(message.formatted()).unwrap();
        assert!(raw.contains("Cc:"));
        assert!(raw.contains("carol@example.com"));
        assert!(raw.contains("dan@example.com"));
    }

    #[test]
    fn bcc_reaches_the_envelope_but_never_the_headers() {
        let mut out = outgoing("alice@example.com");
        out.bcc = "hidden@example.com".to_string();

        let message = build_message("domco@example.com", &out).unwrap();

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

    #[test]
    fn rejects_an_invalid_cc_or_bcc() {
        let mut out = outgoing("alice@example.com");
        out.cc = "not-an-address".to_string();
        assert!(build_message("domco@example.com", &out)
            .unwrap_err()
            .to_string()
            .contains("cc"));

        let mut out = outgoing("alice@example.com");
        out.bcc = "also bad".to_string();
        assert!(build_message("domco@example.com", &out)
            .unwrap_err()
            .to_string()
            .contains("bcc"));
    }

    #[test]
    fn rejects_an_invalid_recipient() {
        let err = build_message("domco@example.com", &outgoing("not-an-address")).unwrap_err();

        assert!(err.to_string().contains("recipient"));
    }

    #[test]
    fn rejects_an_empty_recipient_list() {
        let err = build_message("domco@example.com", &outgoing("   ")).unwrap_err();

        assert!(err.to_string().contains("recipient"));
    }
}
