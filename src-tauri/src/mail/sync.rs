use sqlx::SqlitePool;

use crate::error::AppError;
use crate::mail::{imap, parse};
use crate::models::Account;
use crate::storage::messages::{self, FetchedHeader};

const MAILBOX: &str = "INBOX";
const INITIAL_FETCH: u32 = 50;

/// What a sync run has to do, decided from cache + server state.
#[derive(Debug, PartialEq)]
pub enum SyncPlan {
    /// Nothing cached — fetch the newest N by sequence number.
    Initial,
    /// Cache is valid — fetch only UIDs above the last cached one.
    Incremental { last_uid: i64 },
    /// UIDVALIDITY changed — cached UIDs are meaningless (RFC 3501); wipe
    /// this mailbox's cache and refetch from scratch.
    ResetThenInitial,
}

pub fn plan(stored_validity: Option<i64>, server_validity: i64, last_uid: Option<i64>) -> SyncPlan {
    match stored_validity {
        Some(stored) if stored != server_validity => SyncPlan::ResetThenInitial,
        Some(_) => match last_uid {
            Some(last) => SyncPlan::Incremental { last_uid: last },
            None => SyncPlan::Initial,
        },
        None => SyncPlan::Initial,
    }
}

/// One full sync pass for an account's INBOX: connect, decide, fetch, upsert.
pub async fn sync_inbox(
    pool: &SqlitePool,
    account: &Account,
    password: &str,
) -> Result<(), AppError> {
    let mut session = imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        password,
    )
    .await?;
    let mailbox = session
        .select(MAILBOX)
        .await
        .map_err(|e| AppError::Imap(format!("select {MAILBOX}: {e}")))?;
    let server_validity = i64::from(mailbox.uid_validity.unwrap_or(0));

    let stored = messages::stored_uid_validity(pool, account.id, MAILBOX).await?;
    let last_uid = messages::max_uid(pool, account.id, MAILBOX).await?;

    let raw = match plan(stored, server_validity, last_uid) {
        SyncPlan::ResetThenInitial => {
            messages::clear_mailbox(pool, account.id, MAILBOX).await?;
            initial_fetch(&mut session, mailbox.exists).await?
        }
        SyncPlan::Initial => initial_fetch(&mut session, mailbox.exists).await?,
        SyncPlan::Incremental { last_uid } => {
            let fetched =
                imap::fetch_headers_by_uid(&mut session, &format!("{}:*", last_uid + 1)).await?;
            imap::new_uids_only(fetched, last_uid)
        }
    };
    let _ = session.logout().await;

    let headers: Vec<FetchedHeader> = raw.iter().map(|r| to_fetched(r, server_validity)).collect();
    messages::upsert_headers(pool, account.id, MAILBOX, &headers).await
}

/// Download, parse and cache one message body; returns the parsed body.
pub async fn fetch_body_into_cache(
    pool: &SqlitePool,
    account: &Account,
    password: &str,
    message_id: i64,
    mailbox: &str,
    uid: i64,
) -> Result<parse::ParsedBody, AppError> {
    let mut session = imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        password,
    )
    .await?;
    session
        .select(mailbox)
        .await
        .map_err(|e| AppError::Imap(format!("select {mailbox}: {e}")))?;
    let raw = imap::fetch_body(&mut session, uid).await?;
    let _ = session.logout().await;

    let raw = raw.ok_or_else(|| AppError::Imap("message no longer on the server".to_string()))?;
    let parsed = parse::parse_body(&raw);
    messages::set_body(
        pool,
        message_id,
        parsed.text.as_deref(),
        parsed.html.as_deref(),
        &parsed.snippet,
    )
    .await?;
    Ok(parsed)
}

async fn initial_fetch(
    session: &mut imap::ImapSession,
    exists: u32,
) -> Result<Vec<imap::RawHeader>, AppError> {
    match imap::initial_seq_range(exists, INITIAL_FETCH) {
        Some(range) => imap::fetch_headers_by_seq(session, &range).await,
        None => Ok(Vec::new()),
    }
}

fn to_fetched(raw: &imap::RawHeader, uid_validity: i64) -> FetchedHeader {
    let parsed = parse::parse_header(&raw.header);
    FetchedHeader {
        uid: raw.uid,
        uid_validity,
        from: parsed.from,
        subject: parsed.subject,
        date: parsed.date,
        // why: header fetches carry no body — the snippet is filled in when
        // the body is first downloaded (get_message_body).
        snippet: String::new(),
        read: raw.read,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cache_plans_initial_sync() {
        assert_eq!(plan(None, 7, None), SyncPlan::Initial);
    }

    #[test]
    fn matching_validity_plans_incremental_sync() {
        assert_eq!(
            plan(Some(7), 7, Some(42)),
            SyncPlan::Incremental { last_uid: 42 }
        );
    }

    #[test]
    fn changed_validity_plans_reset() {
        assert_eq!(plan(Some(7), 8, Some(42)), SyncPlan::ResetThenInitial);
    }

    #[test]
    fn matching_validity_without_uids_plans_initial() {
        assert_eq!(plan(Some(7), 7, None), SyncPlan::Initial);
    }

    #[test]
    fn to_fetched_maps_parsed_headers_and_leaves_snippet_empty() {
        let raw = imap::RawHeader {
            uid: 12,
            read: true,
            header: b"From: Alice <alice@example.com>\r\n\
                      Subject: Hi\r\n\
                      Date: Tue, 07 Jul 2026 09:15:00 +0000\r\n\r\n"
                .to_vec(),
        };

        let fetched = to_fetched(&raw, 7);

        assert_eq!(fetched.uid, 12);
        assert_eq!(fetched.uid_validity, 7);
        assert_eq!(fetched.from, "Alice <alice@example.com>");
        assert_eq!(fetched.subject, "Hi");
        assert!(fetched.read);
        assert_eq!(fetched.snippet, "");
    }
}
