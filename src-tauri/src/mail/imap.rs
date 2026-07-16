//! SECURITY-RELEVANT: this is where TLS for IMAP is set up. Implicit TLS
//! only (port 993 style), strict certificate validation — never add any
//! danger_accept_invalid_* call here.

use std::time::Duration;

use async_imap::types::{Fetch, Flag, Name, NameAttribute};
use async_imap::Session;
use async_native_tls::TlsStream;
use futures::TryStreamExt;
use tokio::net::TcpStream;

use crate::error::AppError;
use crate::storage::mailboxes::DiscoveredMailbox;

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

/// All selectable folders on the server, with their special-use roles.
pub async fn list_mailboxes(session: &mut ImapSession) -> Result<Vec<DiscoveredMailbox>, AppError> {
    let stream = session.list(Some(""), Some("*")).await.map_err(imap_err)?;
    let names: Vec<Name> = stream.try_collect().await.map_err(imap_err)?;
    Ok(names
        .iter()
        .filter_map(|n| discovered(n.name(), n.attributes()))
        .collect())
}

/// Map one LIST line to a folder the app can use; `None` for containers
/// that can't hold mail (\Noselect). SPECIAL-USE attributes (RFC 6154) win;
/// well-known English names are the fallback for servers without them.
fn discovered(name: &str, attributes: &[NameAttribute]) -> Option<DiscoveredMailbox> {
    if attributes.contains(&NameAttribute::NoSelect) {
        return None;
    }
    let role = attributes
        .iter()
        .find_map(|attr| match attr {
            NameAttribute::Drafts => Some("drafts"),
            NameAttribute::Sent => Some("sent"),
            NameAttribute::Archive => Some("archive"),
            // why: Gmail has no \Archive folder — its "All Mail" (\All) is the
            // archive target, so moving there removes the Inbox label.
            NameAttribute::All => Some("all"),
            NameAttribute::Junk => Some("junk"),
            NameAttribute::Trash => Some("trash"),
            _ => None,
        })
        .or_else(|| role_from_name(name));
    Some(DiscoveredMailbox {
        name: name.to_string(),
        role: role.map(str::to_string),
    })
}

fn role_from_name(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "inbox" => Some("inbox"),
        "drafts" => Some("drafts"),
        "sent" | "sent messages" | "sent items" => Some("sent"),
        "archive" => Some("archive"),
        "junk" | "spam" => Some("junk"),
        "trash" | "deleted messages" => Some("trash"),
        _ => None,
    }
}

/// One `(uid, seen)` sweep of the whole selected folder — numbers only, no
/// content — so reconciliation can spot deletions and flag changes made by
/// other clients. `exists` guards the empty-folder case (FETCH 1:* errors).
pub async fn fetch_uid_flags(
    session: &mut ImapSession,
    exists: u32,
) -> Result<Vec<(i64, bool)>, AppError> {
    if exists == 0 {
        return Ok(Vec::new());
    }
    let stream = session
        .fetch("1:*", "(UID FLAGS)")
        .await
        .map_err(imap_err)?;
    let fetches: Vec<Fetch> = stream.try_collect().await.map_err(imap_err)?;
    Ok(fetches
        .iter()
        .filter_map(|f| {
            Some((
                i64::from(f.uid?),
                f.flags().any(|flag| matches!(flag, Flag::Seen)),
            ))
        })
        .collect())
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

/// Set or clear the `\Seen` flag on one message by UID — the server-side
/// read state. `.SILENT` suppresses the echoed FETCH, but the tagged
/// response still has to be drained for the command to complete.
pub async fn set_seen(session: &mut ImapSession, uid: i64, seen: bool) -> Result<(), AppError> {
    let query = if seen {
        "+FLAGS.SILENT (\\Seen)"
    } else {
        "-FLAGS.SILENT (\\Seen)"
    };
    let updates = session
        .uid_store(uid.to_string(), query)
        .await
        .map_err(imap_err)?;
    let _: Vec<Fetch> = updates.try_collect().await.map_err(imap_err)?;
    Ok(())
}

/// Move one message by UID to `dest` (e.g. the Trash folder). Prefers the
/// atomic MOVE (RFC 6851); on servers without it, falls back to
/// COPY + mark `\Deleted` + EXPUNGE, expunging only this UID where UIDPLUS
/// allows so other clients' pending deletions are left untouched.
pub async fn move_message(session: &mut ImapSession, uid: i64, dest: &str) -> Result<(), AppError> {
    let uid = uid.to_string();
    // why: read both capabilities up front — Capabilities is owned, so it
    // holds no borrow on the session, unlike the expunge streams below.
    let caps = session.capabilities().await.ok();
    let supports = |name: &str| caps.as_ref().is_some_and(|c| c.has_str(name));

    if supports("MOVE") {
        return session.uid_mv(&uid, dest).await.map_err(imap_err);
    }

    session.uid_copy(&uid, dest).await.map_err(imap_err)?;
    let updates = session
        .uid_store(&uid, "+FLAGS.SILENT (\\Deleted)")
        .await
        .map_err(imap_err)?;
    let _: Vec<Fetch> = updates.try_collect().await.map_err(imap_err)?;
    // why: UID EXPUNGE removes only this message; without UIDPLUS the plain
    // EXPUNGE clears the whole \Deleted set (the RFC's documented fallback).
    if supports("UIDPLUS") {
        let stream = session.uid_expunge(&uid).await.map_err(imap_err)?;
        let _: Vec<_> = stream.try_collect().await.map_err(imap_err)?;
    } else {
        let stream = session.expunge().await.map_err(imap_err)?;
        let _: Vec<_> = stream.try_collect().await.map_err(imap_err)?;
    }
    Ok(())
}

/// Upload one raw RFC-2822 message into a mailbox with the given IMAP flag
/// set — `(\Seen)` for Sent copies, `(\Draft \Seen)` for drafts.
pub async fn append(
    session: &mut ImapSession,
    mailbox: &str,
    flags: &str,
    message: &[u8],
) -> Result<(), AppError> {
    session
        .append(mailbox, Some(flags), None, message)
        .await
        .map_err(imap_err)
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

    #[test]
    fn discovered_skips_unselectable_folders() {
        assert_eq!(discovered("dovecot", &[NameAttribute::NoSelect]), None);
    }

    #[test]
    fn discovered_maps_special_use_attributes_to_roles() {
        let junk = discovered("Spam", &[NameAttribute::Junk]).unwrap();
        assert_eq!(junk.role.as_deref(), Some("junk"));

        let sent = discovered("Odoslané", &[NameAttribute::Sent]).unwrap();
        assert_eq!(sent.role.as_deref(), Some("sent"));
        assert_eq!(sent.name, "Odoslané");

        // Gmail's localized "All Mail" carries \All, not \Archive.
        let all = discovered("[Gmail]/Všetky správy", &[NameAttribute::All]).unwrap();
        assert_eq!(all.role.as_deref(), Some("all"));
    }

    #[test]
    fn discovered_falls_back_to_well_known_names() {
        // Servers without SPECIAL-USE only send generic attributes.
        let trash = discovered("Trash", &[NameAttribute::Unmarked]).unwrap();
        assert_eq!(trash.role.as_deref(), Some("trash"));

        let spam = discovered("spam", &[]).unwrap();
        assert_eq!(spam.role.as_deref(), Some("junk"));

        assert_eq!(
            discovered("INBOX", &[]).unwrap().role.as_deref(),
            Some("inbox")
        );
        assert_eq!(discovered("Projects", &[]).unwrap().role, None);
    }
}
