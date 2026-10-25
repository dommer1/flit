//! The experimental MCP server: Flit's mail offered to an MCP client the user
//! connects (Settings → Experimental). Streamable HTTP on 127.0.0.1 only,
//! behind a bearer token kept in the Keychain — see the hard rule in
//! CLAUDE.md for what it may and may not do.

mod tools;

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::ServerHandler;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

use crate::error::AppError;
use crate::models::{McpSettings, McpSetupKind, McpStatus};
use crate::state::AppState;
use crate::{auth, storage};

/// Where clients connect: `http://127.0.0.1:<port>/mcp`.
pub const PATH: &str = "/mcp";

/// The listener while it runs, and what Settings shows about it.
#[derive(Default)]
pub struct Server {
    // why a tokio Mutex held across a whole restart: two quick toggles must
    // not race each other into binding the port twice.
    running: tokio::sync::Mutex<Option<Running>>,
    status: std::sync::Mutex<McpStatus>,
}

struct Running {
    stop: CancellationToken,
    task: tauri::async_runtime::JoinHandle<()>,
}

impl Server {
    pub fn status(&self) -> McpStatus {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set_status(&self, status: McpStatus) {
        *self
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = status;
    }
}

/// Bring the server in line with the stored settings: stop whatever runs,
/// then listen again if the switch is on. Called at launch and after every
/// MCP settings change — connected clients drop and reconnect to the new
/// tool set.
pub async fn restart(app: &AppHandle) -> McpStatus {
    let state = app.state::<AppState>();
    let mut running = state.mcp.running.lock().await;
    if let Some(old) = running.take() {
        old.stop.cancel();
        // why wait: the old listener has to let go of the port before a new
        // one can bind it; the bound only covers a client that never closes.
        let _ = tokio::time::timeout(Duration::from_secs(2), old.task).await;
    }
    let status = match start(app, &state).await {
        Ok(Some((port, run))) => {
            *running = Some(run);
            McpStatus {
                running: true,
                port: Some(port),
                error: None,
            }
        }
        Ok(None) => McpStatus::default(),
        Err(err) => McpStatus {
            running: false,
            port: None,
            error: Some(err.to_string()),
        },
    };
    state.mcp.set_status(status.clone());
    status
}

/// Store new settings and restart the server under them.
pub async fn apply_settings(
    app: &AppHandle,
    settings: &McpSettings,
) -> Result<McpStatus, AppError> {
    let state = app.state::<AppState>();
    storage::settings::set_mcp_settings(&state.pool, settings).await?;
    Ok(restart(app).await)
}

/// A new token, then a restart: every client still holding the old token
/// loses its session and is refused from then on.
pub async fn regenerate_token(app: &AppHandle) -> Result<McpStatus, AppError> {
    auth::regenerate_mcp_token().await?;
    Ok(restart(app).await)
}

/// Put what a client needs to connect on the clipboard.
///
/// why the backend copies: the token then never enters the settings
/// webview at all — the window only says which text it wants.
pub async fn copy_setup(app: &AppHandle, kind: McpSetupKind) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let port = storage::settings::mcp_settings(&state.pool).await?.port;
    let token = auth::mcp_token().await?;
    copy_to_clipboard(&setup_text(kind, port, &token)).await
}

fn setup_text(kind: McpSetupKind, port: u16, token: &str) -> String {
    let url = format!("http://127.0.0.1:{port}{PATH}");
    match kind {
        McpSetupKind::Token => token.to_string(),
        McpSetupKind::ClaudeCode => format!(
            "claude mcp add --transport http flit {url} --header \"Authorization: Bearer {token}\""
        ),
        McpSetupKind::Json => {
            let config = serde_json::json!({
                "mcpServers": {
                    "flit": {
                        "type": "http",
                        "url": url,
                        "headers": { "Authorization": format!("Bearer {token}") },
                    }
                }
            });
            serde_json::to_string_pretty(&config).unwrap_or_default()
        }
    }
}

/// macOS's own `pbcopy`: the pasteboard without pulling in a clipboard
/// plugin for one button.
async fn copy_to_clipboard(text: &str) -> Result<(), AppError> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new("/usr/bin/pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes()).await?;
    }
    let status = child.wait().await?;
    if !status.success() {
        return Err(AppError::Invalid(
            "Could not copy to the clipboard.".to_string(),
        ));
    }
    Ok(())
}

/// Listen per the stored settings; None when the switch is off.
async fn start(app: &AppHandle, state: &AppState) -> Result<Option<(u16, Running)>, AppError> {
    let settings = storage::settings::mcp_settings(&state.pool).await?;
    if !settings.enabled {
        return Ok(None);
    }
    let token = auth::mcp_token().await?;
    // why Ipv4Addr::LOCALHOST: the hard rule — nothing off this machine may
    // reach the server, whatever the firewall says.
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, settings.port))
        .await
        .map_err(|err| {
            AppError::Invalid(format!("Port {} is not available: {err}", settings.port))
        })?;
    let stop = CancellationToken::new();
    let handler_app = app.clone();
    let access = settings.access;
    let router = router(
        move || Ok(tools::FlitMcp::new(handler_app.clone(), access)),
        token,
        stop.clone(),
    );
    let task = tauri::async_runtime::spawn(serve(listener, router, stop.clone()));
    Ok(Some((settings.port, Running { stop, task })))
}

async fn serve(listener: tokio::net::TcpListener, router: axum::Router, stop: CancellationToken) {
    let shutdown = stop.cancelled_owned();
    if let Err(err) = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
    {
        eprintln!("MCP server stopped: {err}");
    }
}

/// The HTTP side: rmcp's streamable-HTTP service at `PATH`, behind the
/// bearer-token check. `handler` builds one MCP handler per client session.
fn router<S, F>(handler: F, token: String, stop: CancellationToken) -> axum::Router
where
    S: ServerHandler + Send + 'static,
    F: Fn() -> Result<S, std::io::Error> + Send + Sync + 'static,
{
    let config = StreamableHttpServerConfig::default()
        // why refuse every Origin: browsers send one, MCP clients do not —
        // so a web page that guessed the port gets nowhere, token or not.
        // rmcp's default Host check (loopback names only) stays on too,
        // against DNS rebinding.
        .enforce_origin_validation()
        // Stopping the server also ends the open sessions' event streams,
        // which would otherwise hold the graceful shutdown open forever.
        .with_cancellation_token(stop.child_token());
    let service =
        StreamableHttpService::new(handler, Arc::new(LocalSessionManager::default()), config);
    axum::Router::new()
        .nest_service(PATH, service)
        .layer(axum::middleware::from_fn_with_state(
            Arc::<str>::from(token),
            require_token,
        ))
}

async fn require_token(State(token): State<Arc<str>>, request: Request, next: Next) -> Response {
    if bearer_matches(request.headers(), &token) {
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
    )
        .into_response()
}

fn bearer_matches(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|given| constant_time_eq(given.as_bytes(), token.as_bytes()))
}

/// why constant time: an early-exit comparison answers a little faster the
/// more leading characters are right, which a patient local process could
/// measure. The length is not secret — every token is 64 hex digits.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for Flit's handler: the HTTP guard does not care what the
    /// server behind it offers.
    #[derive(Clone)]
    struct Probe;
    impl ServerHandler for Probe {}

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#;

    /// Serve a probe on a free loopback port; returns its /mcp URL.
    async fn spawn() -> (String, CancellationToken) {
        let stop = CancellationToken::new();
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}{PATH}", listener.local_addr().unwrap());
        let router = router(|| Ok(Probe), TOKEN.to_string(), stop.clone());
        tokio::spawn(serve(listener, router, stop.clone()));
        (url, stop)
    }

    fn initialize(url: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .post(url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(INITIALIZE)
    }

    #[tokio::test]
    async fn a_client_with_the_token_gets_in() {
        let (url, stop) = spawn().await;

        let response = initialize(&url).bearer_auth(TOKEN).send().await.unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        stop.cancel();
    }

    #[tokio::test]
    async fn no_token_or_a_wrong_one_is_refused() {
        let (url, stop) = spawn().await;

        let none = initialize(&url).send().await.unwrap();
        assert_eq!(none.status(), reqwest::StatusCode::UNAUTHORIZED);

        let wrong = initialize(&url)
            .bearer_auth(TOKEN.replace('0', "1"))
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), reqwest::StatusCode::UNAUTHORIZED);
        stop.cancel();
    }

    #[tokio::test]
    async fn a_request_a_browser_made_is_refused_even_with_the_token() {
        let (url, stop) = spawn().await;

        let response = initialize(&url)
            .bearer_auth(TOKEN)
            .header("Origin", "https://evil.example")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        stop.cancel();
    }

    #[tokio::test]
    async fn a_rebound_host_name_is_refused() {
        // DNS rebinding: a page's own host name made to resolve to 127.0.0.1.
        let (url, stop) = spawn().await;

        let response = initialize(&url)
            .bearer_auth(TOKEN)
            .header("Host", "evil.example")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        stop.cancel();
    }

    #[test]
    fn the_setup_texts_carry_the_url_and_the_token() {
        let url = "http://127.0.0.1:4729/mcp";
        assert_eq!(setup_text(McpSetupKind::Token, 4729, "abc"), "abc");
        assert_eq!(
            setup_text(McpSetupKind::ClaudeCode, 4729, "abc"),
            format!(
                "claude mcp add --transport http flit {url} --header \"Authorization: Bearer abc\""
            )
        );
        let json: serde_json::Value =
            serde_json::from_str(&setup_text(McpSetupKind::Json, 4729, "abc")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "mcpServers": {
                    "flit": {
                        "type": "http",
                        "url": url,
                        "headers": { "Authorization": "Bearer abc" },
                    }
                }
            })
        );
    }

    #[test]
    fn tokens_compare_whole_and_exact() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
    }
}
