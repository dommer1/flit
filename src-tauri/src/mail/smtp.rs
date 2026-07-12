//! SECURITY-RELEVANT: SMTP TLS setup lives here. Strict certificate
//! validation via lettre's native-tls defaults — never disable it.

use std::time::Duration;

use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, Tokio1Executor};

use crate::error::AppError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

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
}
