//! SECURITY-RELEVANT: this is where TLS for IMAP is set up. Implicit TLS
//! only (port 993 style), strict certificate validation — never add any
//! danger_accept_invalid_* call here.

use std::time::Duration;

use async_imap::types::{Fetch, Flag};
use async_imap::Session;
use async_native_tls::TlsStream;
use futures::TryStreamExt;
use tokio::net::TcpStream;

use crate::error::AppError;

pub type ImapSession = Session<TlsStream<TcpStream>>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const HEADER_QUERY: &str = "(UID FLAGS BODY.PEEK[HEADER])";

/// One fetched message header, still raw — parsing lives in mail::parse.
#[derive(Debug)]
pub struct RawHeader {
    pub uid: i64,
    pub read: bool,
    pub header: Vec<u8>,
}

/// TLS connect + LOGIN. The timeout covers the whole handshake so a black-
/// holed server can't hang a sync forever.
pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<ImapSession, AppError> {
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        let tcp = TcpStream::connect((host, port))
            .await
            .map_err(|e| AppError::Imap(format!("connect {host}:{port}: {e}")))?;
        let tls = async_native_tls::TlsConnector::new()
            .connect(host, tcp)
            .await
            .map_err(|e| AppError::Imap(format!("tls handshake: {e}")))?;
        async_imap::Client::new(tls)
            .login(username, password)
            .await
            .map_err(|(e, _)| AppError::Imap(format!("login: {e}")))
    })
    .await
    .map_err(|_| AppError::Imap(format!("connection to {host}:{port} timed out")))?
}

/// Connection check for the add-account flow: connect, LOGIN, LOGOUT.
pub async fn verify(host: &str, port: u16, username: &str, password: &str) -> Result<(), AppError> {
    let mut session = connect(host, port, username, password).await?;
    // why: best effort — the credentials were already proven by LOGIN.
    let _ = session.logout().await;
    Ok(())
}

/// Fetch headers by sequence-number range (initial sync).
pub async fn fetch_headers_by_seq(
    session: &mut ImapSession,
    range: &str,
) -> Result<Vec<RawHeader>, AppError> {
    let stream = session.fetch(range, HEADER_QUERY).await.map_err(imap_err)?;
    let fetches: Vec<Fetch> = stream.try_collect().await.map_err(imap_err)?;
    Ok(fetches.iter().filter_map(raw_header).collect())
}

/// Fetch headers by UID range (incremental sync).
pub async fn fetch_headers_by_uid(
    session: &mut ImapSession,
    uid_range: &str,
) -> Result<Vec<RawHeader>, AppError> {
    let stream = session
        .uid_fetch(uid_range, HEADER_QUERY)
        .await
        .map_err(imap_err)?;
    let fetches: Vec<Fetch> = stream.try_collect().await.map_err(imap_err)?;
    Ok(fetches.iter().filter_map(raw_header).collect())
}

/// RFC822.SIZE for a set of UIDs in one round trip → `(uid, bytes)` pairs.
/// UIDs the server no longer knows simply don't come back.
pub async fn fetch_sizes(
    session: &mut ImapSession,
    uids: &[i64],
) -> Result<Vec<(i64, u32)>, AppError> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    let set = uids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let stream = session
        .uid_fetch(set, "(UID RFC822.SIZE)")
        .await
        .map_err(imap_err)?;
    let fetches: Vec<Fetch> = stream.try_collect().await.map_err(imap_err)?;
    Ok(fetches
        .iter()
        .filter_map(|f| Some((i64::from(f.uid?), f.size?)))
        .collect())
}

/// Fetch one full raw message by UID; `None` when the server has no such UID.
pub async fn fetch_body(session: &mut ImapSession, uid: i64) -> Result<Option<Vec<u8>>, AppError> {
    let stream = session
        .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
        .await
        .map_err(imap_err)?;
    let fetches: Vec<Fetch> = stream.try_collect().await.map_err(imap_err)?;
    Ok(fetches
        .iter()
        .find(|f| f.uid.map(i64::from) == Some(uid))
        .and_then(|f| f.body().map(<[u8]>::to_vec)))
}

fn imap_err(err: async_imap::error::Error) -> AppError {
    AppError::Imap(err.to_string())
}

fn raw_header(fetch: &Fetch) -> Option<RawHeader> {
    Some(RawHeader {
        uid: i64::from(fetch.uid?),
        read: fetch.flags().any(|f| matches!(f, Flag::Seen)),
        header: fetch.header()?.to_vec(),
    })
}

/// Sequence range covering the newest `count` messages of a mailbox holding
/// `exists` messages; `None` for an empty mailbox.
pub fn initial_seq_range(exists: u32, count: u32) -> Option<String> {
    if exists == 0 || count == 0 {
        return None;
    }
    let start = exists.saturating_sub(count - 1).max(1);
    Some(format!("{start}:*"))
}

/// Drop everything at or below the last cached UID.
///
/// why: `UID FETCH last+1:*` always returns at least the mailbox's newest
/// message even when nothing is new (RFC 3501 quirk) — without this filter
/// every incremental sync would re-upsert the newest message.
pub fn new_uids_only(headers: Vec<RawHeader>, last_uid: i64) -> Vec<RawHeader> {
    headers.into_iter().filter(|h| h.uid > last_uid).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(uid: i64) -> RawHeader {
        RawHeader {
            uid,
            read: false,
            header: Vec::new(),
        }
    }

    #[test]
    fn initial_range_covers_the_newest_n() {
        assert_eq!(initial_seq_range(100, 50), Some("51:*".to_string()));
        assert_eq!(initial_seq_range(50, 50), Some("1:*".to_string()));
    }

    #[test]
    fn initial_range_clamps_small_mailboxes() {
        assert_eq!(initial_seq_range(10, 50), Some("1:*".to_string()));
        assert_eq!(initial_seq_range(1, 50), Some("1:*".to_string()));
    }

    #[test]
    fn initial_range_is_none_for_empty_mailbox() {
        assert_eq!(initial_seq_range(0, 50), None);
        assert_eq!(initial_seq_range(10, 0), None);
    }

    #[test]
    fn new_uids_only_drops_already_cached() {
        let filtered = new_uids_only(vec![raw(9), raw(10), raw(11), raw(12)], 10);

        let uids: Vec<i64> = filtered.iter().map(|h| h.uid).collect();
        assert_eq!(uids, vec![11, 12]);
    }
}
