//! The loopback redirect of a desktop OAuth sign-in: a one-shot HTTP
//! listener on 127.0.0.1 that catches the browser coming back from the
//! provider with `?code=…&state=…`.
//!
//! SECURITY-RELEVANT: it binds the loopback interface only, on a port the OS
//! picks, and accepts a code only together with the sign-in's CSRF state.
//! PKCE makes an intercepted code useless on its own.

use std::time::Duration;

use oauth2::url::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::error::AppError;

/// How long one browser connection may take to send its request line.
/// why: browsers open speculative connections that never send anything; a
/// silent one is dropped after this, so it can't hold up the real redirect.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
/// Enough for any redirect URL; a longer request head is not ours.
const MAX_REQUEST: usize = 16 * 1024;

const SIGNED_IN_PAGE: &str = "You're signed in. You can close this tab and return to Flit.";
const CANCELLED_PAGE: &str = "Sign-in was cancelled. You can close this tab and return to Flit.";
const REJECTED_PAGE: &str = "This sign-in link is not valid. Please start again from Flit.";

pub struct RedirectListener {
    listener: TcpListener,
    port: u16,
}

impl RedirectListener {
    /// Listen on a free loopback port.
    pub async fn bind() -> Result<Self, AppError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    /// The redirect URI to register with this sign-in. Google and Microsoft
    /// accept any port on a loopback address for desktop clients.
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Wait for the browser's redirect and return the authorization code.
    /// Runs until a redirect arrives — the caller bounds it with a timeout
    /// and a cancel.
    pub async fn wait_for_code(self, expected_state: &str) -> Result<String, AppError> {
        self.accept_until_code(expected_state, CONNECTION_TIMEOUT)
            .await
    }

    async fn accept_until_code(
        self,
        expected_state: &str,
        connection_timeout: Duration,
    ) -> Result<String, AppError> {
        loop {
            let (mut socket, _) = self.listener.accept().await?;
            let target =
                match tokio::time::timeout(connection_timeout, read_target(&mut socket)).await {
                    Ok(Ok(target)) => target,
                    // Silent, broken or oversized: not the redirect. Keep waiting.
                    Ok(Err(_)) | Err(_) => continue,
                };
            match outcome(&target, expected_state) {
                Outcome::Stray => {
                    let _ = respond(&mut socket, "404 Not Found", "").await;
                }
                Outcome::Code(code) => {
                    let _ = respond(&mut socket, "200 OK", SIGNED_IN_PAGE).await;
                    return Ok(code);
                }
                Outcome::Denied(error) => {
                    let _ = respond(&mut socket, "200 OK", CANCELLED_PAGE).await;
                    return Err(AppError::OAuth(format!("the provider said: {error}")));
                }
                Outcome::Rejected => {
                    let _ = respond(&mut socket, "400 Bad Request", REJECTED_PAGE).await;
                    return Err(AppError::OAuth(
                        "the browser came back with a foreign sign-in".to_string(),
                    ));
                }
            }
        }
    }
}

/// What one request to the listener means for the sign-in.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// Not the redirect (e.g. the browser asking for /favicon.ico).
    Stray,
    Code(String),
    /// The user declined, or the provider refused (`?error=`).
    Denied(String),
    /// A redirect whose state is not this sign-in's: a forged or stale link.
    Rejected,
}

/// The request target (`/?code=…`) of a GET, from the request head.
async fn read_target(socket: &mut TcpStream) -> Result<String, AppError> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut chunk).await?;
        if n == 0 || head.len() + n > MAX_REQUEST {
            return Err(AppError::OAuth("unreadable redirect request".to_string()));
        }
        head.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&head);
    let mut parts = head.lines().next().unwrap_or_default().split(' ');
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) => Ok(target.to_string()),
        _ => Err(AppError::OAuth("not a GET request".to_string())),
    }
}

fn outcome(target: &str, expected_state: &str) -> Outcome {
    // why a dummy base: the target is a path + query; Url needs an origin
    // to parse it and to decode the percent-escaped values.
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Outcome::Stray;
    };
    if url.path() != "/" {
        return Outcome::Stray;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if param("state").as_deref() != Some(expected_state) {
        return match param("state") {
            None if param("code").is_none() && param("error").is_none() => Outcome::Stray,
            _ => Outcome::Rejected,
        };
    }
    match (param("code"), param("error")) {
        (_, Some(error)) => Outcome::Denied(error),
        (Some(code), None) => Outcome::Code(code),
        (None, None) => Outcome::Rejected,
    }
}

async fn respond(socket: &mut TcpStream, status: &str, text: &str) -> std::io::Result<()> {
    // why so bare: the page loads nothing (no scripts, styles or images) and
    // must not be cached — its URL held a one-time code.
    let body = if text.is_empty() {
        String::new()
    } else {
        format!(
            "<!doctype html><meta charset=\"utf-8\"><title>Flit</title>\
             <p style=\"font:16px -apple-system,sans-serif;margin:40px\">{text}</p>"
        )
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(reply.as_bytes()).await?;
    socket.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Play the browser: send one GET to the listener, return the reply.
    async fn browse(port: u16, target: &str) -> String {
        let mut socket = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let request = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut reply = String::new();
        socket.read_to_string(&mut reply).await.unwrap();
        reply
    }

    async fn listen() -> (RedirectListener, u16) {
        let listener = RedirectListener::bind().await.unwrap();
        let port = listener.port;
        (listener, port)
    }

    #[tokio::test]
    async fn the_redirect_uri_is_the_loopback_port() {
        let (listener, port) = listen().await;

        assert_eq!(listener.redirect_uri(), format!("http://127.0.0.1:{port}"));
        assert!(listener.listener.local_addr().unwrap().ip().is_loopback());
    }

    #[tokio::test]
    async fn a_redirect_with_the_right_state_yields_its_code() {
        let (listener, port) = listen().await;
        let waiting = tokio::spawn(async move { listener.wait_for_code("st4te").await });

        let reply = browse(port, "/?state=st4te&code=4%2F0Ab-c&scope=email").await;

        assert_eq!(waiting.await.unwrap().unwrap(), "4/0Ab-c");
        assert!(reply.starts_with("HTTP/1.1 200 OK"));
        assert!(reply.contains(SIGNED_IN_PAGE));
    }

    #[tokio::test]
    async fn a_declined_sign_in_ends_with_the_providers_error() {
        let (listener, port) = listen().await;
        let waiting = tokio::spawn(async move { listener.wait_for_code("st4te").await });

        let reply = browse(port, "/?error=access_denied&state=st4te").await;

        let err = waiting.await.unwrap().unwrap_err().to_string();
        assert!(err.contains("access_denied"), "{err}");
        assert!(reply.contains(CANCELLED_PAGE));
    }

    #[tokio::test]
    async fn a_code_with_a_foreign_state_is_refused() {
        let (listener, port) = listen().await;
        let waiting = tokio::spawn(async move { listener.wait_for_code("st4te").await });

        let reply = browse(port, "/?state=forged&code=stolen").await;

        assert!(waiting.await.unwrap().is_err());
        assert!(reply.starts_with("HTTP/1.1 400"));
    }

    #[tokio::test]
    async fn stray_and_silent_connections_do_not_end_the_wait() {
        let (listener, port) = listen().await;
        let waiting = tokio::spawn(async move {
            listener
                .accept_until_code("st4te", Duration::from_millis(100))
                .await
        });

        // A speculative connection that never says anything…
        let _silent = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        // …and the browser asking for a favicon.
        let favicon = browse(port, "/favicon.ico").await;
        browse(port, "/?state=st4te&code=real").await;

        assert!(favicon.starts_with("HTTP/1.1 404"));
        assert_eq!(waiting.await.unwrap().unwrap(), "real");
    }
}
