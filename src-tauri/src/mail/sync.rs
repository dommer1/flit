use sqlx::SqlitePool;

use crate::error::AppError;
use crate::mail::{imap, parse};
use crate::models::Account;
use crate::storage::messages::{self, FetchedHeader};

const MAILBOX: &str = "INBOX";
const INITIAL_FETCH: u32 = 50;

/// Background prefetch downloads a message's full body only when the whole
/// message is under this size. Bigger almost always means attachments, and
/// those stay on the server until the user opens the message.
const MAX_PREFETCH_BYTES: u32 = 256 * 1024;
/// Bodies fetched per sync run; matches INITIAL_FETCH so the first run can
/// cover a fresh mailbox.
const PREFETCH_BATCH: i64 = 50;

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

/// One full sync pass for an account: discover folders, mirror them into
/// the mailboxes table, then sync each folder over the same connection.
/// Folders run in sidebar order, so INBOX is fresh before slower ones.
pub async fn sync_account(
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

    let found = imap::list_mailboxes(&mut session).await?;
    crate::storage::mailboxes::replace(pool, account.id, &found).await?;

    for mailbox in crate::storage::mailboxes::list(pool, account.id).await? {
        sync_mailbox(pool, account.id, &mut session, &mailbox.name).await?;
    }
    let _ = session.logout().await;
    Ok(())
}

/// Sync one folder on an already-open session: decide, fetch, upsert.
async fn sync_mailbox(
    pool: &SqlitePool,
    account_id: i64,
    session: &mut imap::ImapSession,
    mailbox: &str,
) -> Result<(), AppError> {
    let selected = session
        .select(mailbox)
        .await
        .map_err(|e| AppError::Imap(format!("select {mailbox}: {e}")))?;
    let server_validity = i64::from(selected.uid_validity.unwrap_or(0));

    let stored = messages::stored_uid_validity(pool, account_id, mailbox).await?;
    let last_uid = messages::max_uid(pool, account_id, mailbox).await?;

    let raw = match plan(stored, server_validity, last_uid) {
        SyncPlan::ResetThenInitial => {
            messages::clear_mailbox(pool, account_id, mailbox).await?;
            initial_fetch(session, selected.exists).await?
        }
        SyncPlan::Initial => initial_fetch(session, selected.exists).await?,
        SyncPlan::Incremental { last_uid } => {
            let fetched =
                imap::fetch_headers_by_uid(session, &format!("{}:*", last_uid + 1)).await?;
            imap::new_uids_only(fetched, last_uid)
        }
    };

    let headers: Vec<FetchedHeader> = raw.iter().map(|r| to_fetched(r, server_validity)).collect();
    messages::upsert_headers(pool, account_id, mailbox, &headers).await?;

    // Mirror what other clients did to this folder (moves, deletes, reads):
    // one cheap numbers-only sweep, then apply the differences locally.
    let server = imap::fetch_uid_flags(session, selected.exists).await?;
    let cached = messages::uid_flags(pool, account_id, mailbox).await?;
    let plan = reconcile_plan(&cached, &server);
    for id in plan.delete {
        messages::delete_by_id(pool, id).await?;
    }
    for (id, read) in plan.flag {
        messages::set_read(pool, id, read).await?;
    }
    Ok(())
}

/// What reconciliation must change locally: rows to drop (the message left
/// the folder server-side) and read flags to adopt.
#[derive(Debug, Default, PartialEq)]
struct ReconcilePlan {
    delete: Vec<i64>,
    /// `(message id, new read state)`
    flag: Vec<(i64, bool)>,
}

/// Diff the cached rows `(id, uid, read)` against the server sweep
/// `(uid, seen)`. Rows the server no longer lists are deleted; flag
/// mismatches adopt the server's state — the server is the source of truth.
fn reconcile_plan(cached: &[(i64, i64, bool)], server: &[(i64, bool)]) -> ReconcilePlan {
    let server_by_uid: std::collections::HashMap<i64, bool> = server.iter().copied().collect();
    let mut plan = ReconcilePlan::default();
    for (id, uid, read) in cached {
        match server_by_uid.get(uid) {
            None => plan.delete.push(*id),
            Some(seen) if seen != read => plan.flag.push((*id, *seen)),
            Some(_) => {}
        }
    }
    plan
}

/// Cross the prefetch work-list with the sizes the server reported: keep
/// only messages known to be small enough. No size reported → skipped —
/// never download blind.
fn prefetch_plan(missing: &[(i64, i64)], sizes: &[(i64, u32)], max_bytes: u32) -> Vec<(i64, i64)> {
    missing
        .iter()
        .filter(|(_, uid)| {
            sizes
                .iter()
                .any(|(sized_uid, size)| sized_uid == uid && *size <= max_bytes)
        })
        .copied()
        .collect()
}

/// Download and cache bodies for recent messages that have none, so search
/// covers mail the user never opened. Returns how many bodies were cached.
pub async fn prefetch_bodies(
    pool: &SqlitePool,
    account: &Account,
    password: &str,
) -> Result<usize, AppError> {
    let missing = messages::uids_missing_body(pool, account.id, MAILBOX, PREFETCH_BATCH).await?;
    if missing.is_empty() {
        return Ok(0);
    }

    let mut session = imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        password,
    )
    .await?;
    session
        .select(MAILBOX)
        .await
        .map_err(|e| AppError::Imap(format!("select {MAILBOX}: {e}")))?;

    let uids: Vec<i64> = missing.iter().map(|(_, uid)| *uid).collect();
    let sizes = imap::fetch_sizes(&mut session, &uids).await?;

    let mut cached = 0;
    for (message_id, uid) in prefetch_plan(&missing, &sizes, MAX_PREFETCH_BYTES) {
        // why: a UID can vanish mid-run (deleted on another device) — skip it
        // rather than aborting the whole batch.
        let Some(raw) = imap::fetch_body(&mut session, uid).await? else {
            continue;
        };
        let parsed = parse::parse_body(&raw);
        messages::set_body(
            pool,
            message_id,
            parsed.text.as_deref(),
            parsed.html.as_deref(),
            &parsed.snippet,
        )
        .await?;
        cached += 1;
    }
    let _ = session.logout().await;
    Ok(cached)
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
        to: parsed.to,
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
    fn reconcile_plan_deletes_missing_and_adopts_server_flags() {
        // cached: (row id, uid, read)
        let cached = [(1, 101, false), (2, 102, true), (3, 103, false)];
        // server sweep: (uid, seen) — uid 102 vanished (moved or deleted),
        // uid 101 was read elsewhere, uid 103 is unchanged.
        let server = [(101, true), (103, false)];

        let plan = reconcile_plan(&cached, &server);

        assert_eq!(plan.delete, vec![2]);
        assert_eq!(plan.flag, vec![(1, true)]);
    }

    #[test]
    fn prefetch_plan_keeps_only_small_messages_with_a_known_size() {
        let missing = [(1, 101), (2, 102), (3, 103)];
        // 102 is over the cap; 103 never got a size back from the server.
        let sizes = [(101, 10_000), (102, 999_999)];

        assert_eq!(prefetch_plan(&missing, &sizes, 262_144), vec![(1, 101)]);
    }

    #[test]
    fn to_fetched_maps_parsed_headers_and_leaves_snippet_empty() {
        let raw = imap::RawHeader {
            uid: 12,
            read: true,
            header: b"From: Alice <alice@example.com>\r\n\
                      To: Bob <bob@example.com>\r\n\
                      Subject: Hi\r\n\
                      Date: Tue, 07 Jul 2026 09:15:00 +0000\r\n\r\n"
                .to_vec(),
        };

        let fetched = to_fetched(&raw, 7);

        assert_eq!(fetched.uid, 12);
        assert_eq!(fetched.uid_validity, 7);
        assert_eq!(fetched.from, "Alice <alice@example.com>");
        assert_eq!(fetched.to, "Bob <bob@example.com>");
        assert_eq!(fetched.subject, "Hi");
        assert!(fetched.read);
        assert_eq!(fetched.snippet, "");
    }
}
