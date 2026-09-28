//! OAuth 2.0 sign-in for mail providers (Google today): the browser
//! authorization-code flow with PKCE, and access-token refresh.
//!
//! SECURITY-RELEVANT: this module handles refresh and access tokens. They
//! never reach the frontend, a log line or the database — refresh tokens go
//! to the keychain (see auth), access tokens only live in memory.

use std::time::Duration;

use oauth2::basic::{
    BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
    BasicTokenType,
};
use oauth2::url::Url;
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, ExtraTokenFields, HttpClientError, HttpRequest, HttpResponse, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RequestTokenError, Scope, StandardRevocableToken,
    StandardTokenResponse, TokenResponse as _, TokenUrl,
};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::models::AuthKind;

/// Everything that differs between OAuth mail providers. Plain data: the
/// sign-in flow itself is shared, and a provider is one more constant.
#[derive(Clone, Copy)]
pub struct Provider {
    pub kind: AuthKind,
    auth_url: &'static str,
    token_url: &'static str,
    scopes: &'static [&'static str],
    /// Extra query parameters for the authorization URL.
    auth_params: &'static [(&'static str, &'static str)],
    /// Baked in at build time from `.env`; `None` in a build without it,
    /// which then offers no sign-in with this provider.
    client_id: Option<&'static str>,
    client_secret: Option<&'static str>,
    pub imap_host: &'static str,
    pub imap_port: u16,
    pub smtp_host: &'static str,
    pub smtp_port: u16,
}

pub const GOOGLE: Provider = Provider {
    kind: AuthKind::Google,
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    // why mail.google.com: the only scope Gmail accepts for IMAP and SMTP.
    // openid + email put the signed-in address into the id_token, so the
    // account is created for the mailbox the user actually picked.
    scopes: &["https://mail.google.com/", "openid", "email"],
    // why: without offline access Google issues no refresh token, and
    // without prompt=consent a repeated sign-in (reconnect) gets none either.
    auth_params: &[("access_type", "offline"), ("prompt", "consent")],
    // why in the binary: Google treats a desktop app's client secret as not
    // confidential (anyone can extract it); it stays out of the repo only so
    // forks register their own client.
    client_id: option_env!("FLIT_GOOGLE_CLIENT_ID"),
    client_secret: option_env!("FLIT_GOOGLE_CLIENT_SECRET"),
    imap_host: "imap.gmail.com",
    imap_port: 993,
    smtp_host: "smtp.gmail.com",
    smtp_port: 465,
};

/// The id_token the provider returns next to the access token (OpenID
/// Connect). Read once for the signed-in address, never stored.
#[derive(Clone, Deserialize, Serialize)]
pub struct IdTokenFields {
    id_token: Option<String>,
}

impl ExtraTokenFields for IdTokenFields {}

// why by hand: ExtraTokenFields requires Debug, and a derived one would
// print the token. oauth2's own token types redact theirs the same way.
impl std::fmt::Debug for IdTokenFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IdTokenFields([redacted])")
    }
}

type TokenResponse = StandardTokenResponse<IdTokenFields, BasicTokenType>;

// why a type alias: oauth2 records in the type which endpoints a client has
// (auth, device, introspection, revocation, token) — ours sets auth + token.
type OAuthClient = oauth2::Client<
    BasicErrorResponse,
    TokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

/// What the token endpoint hands back. Deliberately not `Debug`: it holds
/// the tokens.
pub struct Tokens {
    pub access_token: String,
    /// Absent when the provider keeps the current one (a refresh at Google).
    pub refresh_token: Option<String>,
    /// How long the access token lives, as the provider states it.
    pub expires_in: Option<Duration>,
    /// The signed-in address, from the id_token.
    pub email: Option<String>,
}

/// A sign-in in progress: the URL to open in the browser, and the two
/// secrets the redirect back must be checked and exchanged with.
pub struct Authorization {
    pub url: Url,
    pub state: CsrfToken,
    pub pkce_verifier: PkceCodeVerifier,
}

impl Provider {
    /// True when this build carries a client id for the provider.
    pub fn is_configured(&self) -> bool {
        self.client_id.is_some()
    }

    fn client(&self, redirect_uri: &str) -> Result<OAuthClient, AppError> {
        let client_id = self.client_id.ok_or_else(|| {
            AppError::OAuth("this build has no OAuth client for the provider".to_string())
        })?;
        let invalid = |e: oauth2::url::ParseError| AppError::OAuth(e.to_string());
        let mut client = oauth2::Client::new(ClientId::new(client_id.to_string()))
            .set_auth_uri(AuthUrl::new(self.auth_url.to_string()).map_err(invalid)?)
            .set_token_uri(TokenUrl::new(self.token_url.to_string()).map_err(invalid)?)
            .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string()).map_err(invalid)?)
            // why: credentials in the form body is how Google documents its
            // desktop flow, and a public client (no secret) needs it anyway.
            .set_auth_type(AuthType::RequestBody);
        if let Some(secret) = self.client_secret {
            client = client.set_client_secret(ClientSecret::new(secret.to_string()));
        }
        Ok(client)
    }

    /// Start a sign-in: the browser URL, with a fresh PKCE challenge and
    /// CSRF state. `login_hint` preselects the account on a reconnect.
    pub fn authorize(
        &self,
        redirect_uri: &str,
        login_hint: Option<&str>,
    ) -> Result<Authorization, AppError> {
        let (challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let client = self.client(redirect_uri)?;
        let mut request = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(challenge)
            .add_scopes(self.scopes.iter().map(|s| Scope::new(s.to_string())));
        for (name, value) in self.auth_params {
            request = request.add_extra_param(*name, *value);
        }
        if let Some(hint) = login_hint {
            request = request.add_extra_param("login_hint", hint);
        }
        let (url, state) = request.url();
        Ok(Authorization {
            url,
            state,
            pkce_verifier,
        })
    }

    /// Trade the code from the redirect (plus the PKCE verifier that proves
    /// this app started the sign-in) for tokens.
    pub async fn exchange_code(
        &self,
        http: &reqwest::Client,
        redirect_uri: &str,
        code: String,
        pkce_verifier: PkceCodeVerifier,
    ) -> Result<Tokens, AppError> {
        let response = self
            .client(redirect_uri)?
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(&|request| send(http.clone(), request))
            .await
            .map_err(token_error)?;
        Ok(Tokens {
            access_token: response.access_token().secret().clone(),
            refresh_token: response.refresh_token().map(|t| t.secret().clone()),
            expires_in: response.expires_in(),
            email: response
                .extra_fields()
                .id_token
                .as_deref()
                .and_then(email_from_id_token),
        })
    }
}

/// A readable reason for a failed token request. Never the reply body: a
/// reply that failed to parse may still have carried a token.
fn token_error(
    err: RequestTokenError<HttpClientError<reqwest::Error>, BasicErrorResponse>,
) -> AppError {
    AppError::OAuth(match err {
        RequestTokenError::ServerResponse(reply) => match reply.error_description() {
            Some(description) => format!("{} ({description})", reply.error()),
            None => reply.error().to_string(),
        },
        RequestTokenError::Request(e) => format!("token request failed: {e}"),
        RequestTokenError::Parse(..) => "unreadable token response".to_string(),
        RequestTokenError::Other(e) => e,
    })
}

/// The `email` claim of an OpenID id_token.
///
/// why the signature is not checked: the token came straight from the
/// provider's token endpoint over TLS, which OpenID Connect accepts in place
/// of signature validation (Core 1.0, §3.1.3.7). The address is then proven
/// again by the IMAP login it is used for.
fn email_from_id_token(id_token: &str) -> Option<String> {
    use base64::Engine;

    #[derive(Deserialize)]
    struct Claims {
        email: Option<String>,
    }
    let payload = id_token.split('.').nth(1)?;
    let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice::<Claims>(&json).ok()?.email
}

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

    /// The whole request — head and, per Content-Length, body — however
    /// many reads it takes to arrive.
    async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = socket.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&request).into_owned();
            if let Some(head_end) = text.find("\r\n\r\n") {
                let body_len = text[..head_end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if n == 0 || request.len() >= head_end + 4 + body_len {
                    return text;
                }
            } else if n == 0 {
                return text;
            }
        }
    }

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
            let request = read_request(&mut socket).await;
            let _ = seen_tx.send(request);
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

    /// Google, as a build with a client id would have it.
    const TEST_GOOGLE: Provider = Provider {
        client_id: Some("test-client.apps.googleusercontent.com"),
        client_secret: Some("test-secret"),
        ..GOOGLE
    };

    fn query(url: &Url) -> std::collections::HashMap<String, String> {
        url.query_pairs().into_owned().collect()
    }

    #[test]
    fn the_google_sign_in_url_asks_for_gmail_offline_with_pkce() {
        let auth = TEST_GOOGLE
            .authorize("http://127.0.0.1:5555", None)
            .unwrap();

        assert_eq!(auth.url.host_str(), Some("accounts.google.com"));
        assert_eq!(auth.url.path(), "/o/oauth2/v2/auth");
        let q = query(&auth.url);
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "test-client.apps.googleusercontent.com");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:5555");
        assert_eq!(q["scope"], "https://mail.google.com/ openid email");
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "consent");
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(!q["code_challenge"].is_empty());
        assert_eq!(&q["state"], auth.state.secret());
        assert!(!q.contains_key("login_hint"));
        // The secret half of PKCE and the client secret never go to the browser.
        assert!(!auth.url.as_str().contains(auth.pkce_verifier.secret()));
        assert!(!auth.url.as_str().contains("test-secret"));
    }

    #[test]
    fn every_sign_in_gets_fresh_state_and_pkce() {
        let first = TEST_GOOGLE.authorize("http://127.0.0.1:1", None).unwrap();
        let second = TEST_GOOGLE.authorize("http://127.0.0.1:1", None).unwrap();

        assert_ne!(first.state.secret(), second.state.secret());
        assert_ne!(first.pkce_verifier.secret(), second.pkce_verifier.secret());
    }

    #[test]
    fn a_reconnect_preselects_the_account() {
        let auth = TEST_GOOGLE
            .authorize("http://127.0.0.1:1", Some("jan@gmail.com"))
            .unwrap();

        assert_eq!(query(&auth.url)["login_hint"], "jan@gmail.com");
    }

    #[test]
    fn a_build_without_a_client_id_cannot_start_a_sign_in() {
        let unconfigured = Provider {
            client_id: None,
            ..GOOGLE
        };

        assert!(!unconfigured.is_configured());
        assert!(unconfigured.authorize("http://127.0.0.1:1", None).is_err());
    }

    /// An unsigned JWT with `claims` as its payload — enough for the
    /// decoder, which reads but does not verify (see email_from_id_token).
    fn id_token(claims: &str) -> String {
        use base64::Engine;
        let b64 = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        format!("{}.{}.sig", b64(r#"{"alg":"RS256"}"#), b64(claims))
    }

    #[test]
    fn the_signed_in_address_comes_from_the_id_token() {
        let token = id_token(r#"{"iss":"https://accounts.google.com","email":"jan@gmail.com"}"#);

        assert_eq!(
            email_from_id_token(&token).as_deref(),
            Some("jan@gmail.com")
        );
    }

    #[test]
    fn a_malformed_id_token_yields_no_address() {
        assert_eq!(email_from_id_token("not-a-jwt"), None);
        assert_eq!(email_from_id_token("a.%%%.c"), None);
        assert_eq!(email_from_id_token(&id_token(r#"{"sub":"1"}"#)), None);
    }

    /// TEST_GOOGLE pointed at a local plain-http token endpoint.
    fn local_google(token_url: &str) -> Provider {
        Provider {
            token_url: Box::leak(token_url.to_string().into_boxed_str()),
            ..TEST_GOOGLE
        }
    }

    #[tokio::test]
    async fn a_code_is_exchanged_with_its_pkce_verifier_for_tokens() {
        let body: &'static str = Box::leak(
            format!(
                r#"{{"access_token":"ya29.a","expires_in":3599,"refresh_token":"1//r",
                    "scope":"https://mail.google.com/ openid email","token_type":"Bearer",
                    "id_token":"{}"}}"#,
                id_token(r#"{"email":"jan@gmail.com"}"#)
            )
            .into_boxed_str(),
        );
        let (url, seen) = serve_once("200 OK", body).await;
        let auth = TEST_GOOGLE
            .authorize("http://127.0.0.1:5555", None)
            .unwrap();
        let verifier = auth.pkce_verifier.secret().clone();

        let tokens = local_google(&url)
            .exchange_code(
                &reqwest::Client::new(),
                "http://127.0.0.1:5555",
                "4/code".to_string(),
                auth.pkce_verifier,
            )
            .await
            .unwrap();

        assert_eq!(tokens.access_token, "ya29.a");
        assert_eq!(tokens.refresh_token.as_deref(), Some("1//r"));
        assert_eq!(tokens.expires_in, Some(Duration::from_secs(3599)));
        assert_eq!(tokens.email.as_deref(), Some("jan@gmail.com"));
        let form = seen.await.unwrap();
        for field in [
            "grant_type=authorization_code".to_string(),
            "code=4%2Fcode".to_string(),
            format!("code_verifier={verifier}"),
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A5555".to_string(),
            "client_id=test-client.apps.googleusercontent.com".to_string(),
            "client_secret=test-secret".to_string(),
        ] {
            assert!(form.contains(&field), "missing {field} in {form}");
        }
    }

    #[tokio::test]
    async fn a_refused_exchange_names_the_error_but_not_the_reply() {
        let (url, _seen) = serve_once(
            "400 Bad Request",
            r#"{"error":"invalid_grant","error_description":"Bad Request"}"#,
        )
        .await;
        let auth = TEST_GOOGLE
            .authorize("http://127.0.0.1:5555", None)
            .unwrap();

        let err = local_google(&url)
            .exchange_code(
                &reqwest::Client::new(),
                "http://127.0.0.1:5555",
                "4/code".to_string(),
                auth.pkce_verifier,
            )
            .await
            .err()
            .unwrap()
            .to_string();

        assert!(err.contains("invalid_grant"), "{err}");
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
