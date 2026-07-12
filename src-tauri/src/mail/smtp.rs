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

/// Build the MIME message for an outgoing mail. `to` accepts a
/// comma-separated recipient list; the body is plain text for now.
pub fn build_message(from_email: &str, outgoing: &OutgoingMessage) -> Result<Message, AppError> {
    let from: Mailbox = from_email
        .parse()
        .map_err(|e| AppError::Smtp(format!("invalid from address: {e}")))?;
    let recipients: Vec<Mailbox> = outgoing
        .to
        .parse::<Mailboxes>()
        .map_err(|e| AppError::Smtp(format!("invalid recipient: {e}")))?
        .into_iter()
        .collect();
    if recipients.is_empty() {
        return Err(AppError::Smtp("no recipient given".to_string()));
    }

    let mut builder = Message::builder()
        .from(from)
        .subject(&outgoing.subject)
        .header(ContentType::TEXT_PLAIN);
    // why: .to() appends to the To header on every call — lettre's way of
    // setting multiple recipients without hand-building the header.
    for recipient in recipients {
        builder = builder.to(recipient);
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

    fn outgoing(to: &str) -> OutgoingMessage {
        OutgoingMessage {
            account_id: 1,
            to: to.to_string(),
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
