//! OAuth 2.0 sign-in for mail providers (Google today): the browser
//! authorization-code flow with PKCE, and access-token refresh.
//!
//! SECURITY-RELEVANT: this module handles refresh and access tokens. They
//! never reach the frontend, a log line or the database — refresh tokens go
//! to the keychain (see auth), access tokens only live in memory.

use std::time::Duration;

use oauth2::{HttpClientError, HttpRequest, HttpResponse};

use crate::error::AppError;

/// Client for the provider's token endpoint: TLS-only, no Referer, no
/// cookie jar (feature not compiled), and no redirects — oauth2 asks for
/// that, so a hostile redirect can't bounce a token request elsewhere.
pub fn http_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .build()
        .map_err(|e| AppError::Http(e.to_string()))
}

/// oauth2's HTTP hook, served by our own reqwest.
///
/// why not oauth2's bundled reqwest support: oauth2 5 is built against
/// reqwest 0.12 and Flit uses 0.13, so enabling it would compile a second
/// reqwest with a second TLS stack. oauth2 accepts any `Fn(HttpRequest) ->
/// Future`, so a closure calling this function is the whole bridge. The
/// body mirrors oauth2's own reqwest implementation.
pub async fn send(
    client: reqwest::Client,
    request: HttpRequest,
) -> Result<HttpResponse, HttpClientError<reqwest::Error>> {
    let response = client
        .execute(request.try_into().map_err(Box::new)?)
        .await
        .map_err(Box::new)?;
    let mut builder = oauth2::http::Response::builder()
        .status(response.status())
        .version(response.version());
    for (name, value) in response.headers() {
        builder = builder.header(name, value);
    }
    builder
        .body(response.bytes().await.map_err(Box::new)?.to_vec())
        .map_err(HttpClientError::Http)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A one-shot HTTP server on localhost: hands the raw request it got to
    /// the returned receiver and answers with `status` and `body`.
    async fn serve_once(
        status: &'static str,
        body: &'static str,
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let n = socket.read(&mut request).await.unwrap();
            let _ = seen_tx.send(String::from_utf8_lossy(&request[..n]).into_owned());
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        (format!("http://{addr}/token"), seen_rx)
    }

    #[tokio::test]
    async fn the_bridge_carries_a_request_and_its_error_reply_through_reqwest() {
        let (url, seen) = serve_once("400 Bad Request", r#"{"error":"invalid_grant"}"#).await;
        let request = oauth2::http::Request::builder()
            .method("POST")
            .uri(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(b"grant_type=refresh_token".to_vec())
            .unwrap();

        // Plain http for the local test server — the production client is
        // TLS-only and cannot talk to it.
        let response = send(reqwest::Client::new(), request).await.unwrap();

        assert_eq!(response.status(), 400);
        assert_eq!(response.body(), br#"{"error":"invalid_grant"}"#);
        let seen = seen.await.unwrap();
        assert!(seen.starts_with("POST /token "));
        assert!(seen.ends_with("grant_type=refresh_token"));
    }
}
