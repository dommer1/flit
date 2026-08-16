use sqlx::SqlitePool;

use crate::error::AppError;
use crate::mail::{imap, parse};
use crate::models::Account;
use crate::storage::messages::{self, FetchedHeader};

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

/// How long a folder may go without a full flag reconciliation. Ordinary
/// passes only sweep the newest slice, so this is the cadence at which a
/// deletion far below that window is finally noticed.
pub const FULL_SWEEP_INTERVAL: i64 = 60 * 60;

/// How many of a folder's newest messages an ordinary pass reconciles.
/// Changes made on another device land at the top of a folder, so this
/// catches them; anything older waits for the full sweep.
pub const SWEEP_WINDOW: u32 = 1_000;

/// Whether this folder's full sweep is due, from its last one (epoch
/// seconds; `None` = never swept) and the current time.
///
/// why the range check rather than `now - last >= interval`: if the system
/// clock jumps backwards, plain subtraction goes negative and the folder
/// would stop sweeping until real time caught up. A `now` outside the
/// window we trust means sweep — the cheap answer is always the safe one.
pub fn full_sweep_due(last: Option<i64>, now: i64, interval: i64) -> bool {
    match last {
        None => true,
        Some(last) => !(last..last.saturating_add(interval)).contains(&now),
    }
}

/// One message a sync discovered as genuinely new — the payload a new-mail
/// notification is built from.
#[derive(Debug, Clone, PartialEq)]
pub struct NewMail {
    pub from: String,
    pub subject: String,
}

/// One full sync pass for an account: discover folders, mirror them into
/// the mailboxes table, then sync each folder over the caller's connection.
/// Folders run in sidebar order, so INBOX is fresh before slower ones.
///
/// The caller owns the session — and so owns closing it — because one pass
/// is followed by the body prefetch and the header backfill against the
/// same server.
///
/// Returns the new unread inbox messages this pass brought in — only those
/// can warrant a notification. Initial and reset fetches report nothing:
/// a freshly added account must not fire fifty notifications at once.
pub async fn sync_account(
    pool: &SqlitePool,
    account: &Account,
    session: &mut imap::ImapSession,
) -> Result<Vec<NewMail>, AppError> {
    let found = imap::list_mailboxes(session).await?;
    crate::storage::mailboxes::replace(pool, account.id, &found).await?;

    let mut new_mail = Vec::new();
    for mailbox in crate::storage::mailboxes::list(pool, account.id).await? {
        let fetched = sync_mailbox(pool, account.id, session, &mailbox.name).await?;
        if mailbox.role.as_deref() == Some("inbox") {
            new_mail.extend(notifiable(&fetched));
        }
    }
    Ok(new_mail)
}

/// The headers that deserve a notification: unread ones, as payloads.
fn notifiable(headers: &[FetchedHeader]) -> Vec<NewMail> {
    headers
        .iter()
        .filter(|header| !header.read)
        .map(|header| NewMail {
            from: header.from.clone(),
            subject: header.subject.clone(),
        })
        .collect()
}

/// Sync one folder on an already-open session: decide, fetch, upsert.
/// Returns the fetched headers when the plan was incremental — exactly the
/// messages that were not cached before; other plans return nothing (their
/// fetches mostly re-cover mail the user has already seen).
///
/// pub(crate): the draft commands run this for the Drafts folder right
/// after an APPEND/delete, so the cache (and the conversation view) shows
/// the change without waiting for the next full account sync.
pub(crate) async fn sync_mailbox(
    pool: &SqlitePool,
    account_id: i64,
    session: &mut imap::ImapSession,
    mailbox: &str,
) -> Result<Vec<FetchedHeader>, AppError> {
    let selected = session
        .select(mailbox)
        .await
        .map_err(|e| AppError::Imap(format!("select {mailbox}: {e}")))?;
    let server_validity = i64::from(selected.uid_validity.unwrap_or(0));
    // The server's message count feeds the backfill progress indicator.
    crate::storage::mailboxes::set_server_exists(
        pool,
        account_id,
        mailbox,
        i64::from(selected.exists),
    )
    .await?;

    let stored = messages::stored_uid_validity(pool, account_id, mailbox).await?;
    let last_uid = messages::max_uid(pool, account_id, mailbox).await?;

    let sync_plan = plan(stored, server_validity, last_uid);
    let incremental = matches!(sync_plan, SyncPlan::Incremental { .. });
    let raw = match sync_plan {
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
    // one numbers-only sweep, then apply the differences locally.
    //
    // why windowed: this used to sweep every folder end to end on every
    // pass — tens of thousands of FETCH replies and cached rows per pass,
    // for a handful of changes. Ordinary passes now cover only the newest
    // slice, and the wider sweep that can still catch an old deletion runs
    // on FULL_SWEEP_INTERVAL.
    let now = now_epoch();
    let full = full_sweep_due(
        crate::storage::mailboxes::last_full_sweep(pool, account_id, mailbox).await?,
        now,
        FULL_SWEEP_INTERVAL,
    );
    let window = if full { None } else { Some(SWEEP_WINDOW) };

    match imap::sweep_range(selected.exists, window) {
        // The server emptied the folder (someone emptied Trash elsewhere).
        // Drop the cached rows now rather than at the next full sweep.
        None => messages::clear_mailbox(pool, account_id, mailbox).await?,
        Some(range) => {
            let server = imap::fetch_uid_flags(session, &range).await?;
            if let Some(floor) = reconcile_floor(&server) {
                let cached = messages::uid_flags(pool, account_id, mailbox, floor).await?;
                let plan = reconcile_plan(&cached, &server);
                for id in plan.delete {
                    messages::delete_by_id(pool, id).await?;
                }
                for (id, read) in plan.flag {
                    messages::set_read(pool, id, read).await?;
                }
            }
        }
    }
    if full {
        crate::storage::mailboxes::mark_full_sweep(pool, account_id, mailbox, now).await?;
    }

    backfill_thread_headers(pool, account_id, session, mailbox).await?;
    Ok(if incremental { headers } else { Vec::new() })
}

/// One-time repair for rows cached before the threading columns existed:
/// re-fetch just their headers and compute their thread keys. A no-op once
/// every cached row carries a message_id_hdr, i.e. on every sync but the
/// first after updating.
async fn backfill_thread_headers(
    pool: &SqlitePool,
    account_id: i64,
    session: &mut imap::ImapSession,
    mailbox: &str,
) -> Result<(), AppError> {
    let missing = messages::rows_missing_threading(pool, account_id, mailbox).await?;
    if missing.is_empty() {
        return Ok(());
    }
    let uid_set = missing
        .iter()
        .map(|(_, uid)| uid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let fetched = imap::fetch_headers_by_uid(session, &uid_set).await?;
    let row_by_uid: std::collections::HashMap<i64, i64> =
        missing.into_iter().map(|(id, uid)| (uid, id)).collect();
    for raw in &fetched {
        // A uid the cache no longer knows (or that vanished server-side
        // mid-run) is simply skipped; reconciliation owns deletions.
        let Some(&row_id) = row_by_uid.get(&raw.uid) else {
            continue;
        };
        let header = to_fetched(raw, 0);
        messages::backfill_threading(pool, account_id, row_id, &header).await?;
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

/// The lowest uid a sweep returned — the floor of the cached range it may
/// be reconciled against. `None` when the sweep came back empty, which is
/// the signal NOT to reconcile at all: the folder was not empty (or
/// `sweep_range` would have said so), so an empty result is an anomaly, and
/// diffing the whole cache against nothing would delete every cached row.
fn reconcile_floor(server: &[(i64, bool)]) -> Option<i64> {
    server.iter().map(|(uid, _)| *uid).min()
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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

/// The backfill work list: every UID the server lists that the cache lacks —
/// holes inside the cached range included, not just the tail below it.
fn missing_uids(on_server: Vec<i64>, cached: &std::collections::HashSet<i64>) -> Vec<i64> {
    on_server
        .into_iter()
        .filter(|uid| !cached.contains(uid))
        .collect()
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

/// Download and cache bodies for messages that have none, so search covers
/// mail the user never opened. Folders run in sidebar order (INBOX first)
/// against one shared budget per run. Returns how many bodies were cached.
pub async fn prefetch_bodies(
    pool: &SqlitePool,
    account: &Account,
    session: &mut imap::ImapSession,
) -> Result<usize, AppError> {
    if !messages::has_missing_bodies(pool, account.id).await? {
        return Ok(0);
    }
    let folders = crate::storage::mailboxes::list(pool, account.id).await?;

    let mut budget = PREFETCH_BATCH;
    let mut cached = 0;
    for folder in &folders {
        if budget == 0 {
            break;
        }
        let missing = messages::uids_missing_body(pool, account.id, &folder.name, budget).await?;
        if missing.is_empty() {
            continue;
        }
        session
            .select(&folder.name)
            .await
            .map_err(|e| AppError::Imap(format!("select {}: {e}", folder.name)))?;

        let uids: Vec<i64> = missing.iter().map(|(_, uid)| *uid).collect();
        let sizes = imap::fetch_sizes(session, &uids).await?;

        for (message_id, uid) in prefetch_plan(&missing, &sizes, MAX_PREFETCH_BYTES) {
            // why: a UID can vanish mid-run (deleted on another device) —
            // skip it rather than aborting the whole batch.
            let Some(raw) = imap::fetch_body(session, uid).await? else {
                continue;
            };
            let parsed = parse::parse_body(&raw);
            messages::set_body(
                pool,
                message_id,
                parsed.text.as_deref(),
                parsed.html.as_deref(),
                &parsed.snippet,
                &parsed.images,
                &parsed.attachments,
                parsed.auth.as_ref(),
            )
            .await?;
            cached += 1;
            budget -= 1;
        }
    }
    Ok(cached)
}

/// Older headers mirrored per batch before `on_batch` reports progress —
/// big enough to move fast, small enough that the list visibly grows.
// why 100 and not 500: one batch is one write transaction, and each header
// in it runs a key lookup, an insert (which reindexes the body for search)
// and a handful of contact sightings — so 500 headers held SQLite's single
// writer for seconds at a time. Everything else that wants to write waits
// that long. Smaller batches give the writer up more often; the extra
// commits are cheap next to the work inside them.
const BACKFILL_BATCH: usize = 100;

/// Mirror every folder's remaining older headers into the cache, newest
/// first, until the account is complete. `on_batch` fires after each cached
/// batch so the UI can refresh while the loop is still running. Returns how
/// many headers were mirrored.
///
/// why one connection for the whole loop: this can run for minutes over
/// thousands of messages — reconnecting per batch would waste most of the
/// time in TLS handshakes.
pub async fn backfill_headers(
    pool: &SqlitePool,
    account: &Account,
    session: &mut imap::ImapSession,
    on_batch: impl Fn(),
) -> Result<usize, AppError> {
    let folders = crate::storage::mailboxes::incomplete_mailboxes(pool, account.id).await?;
    if folders.is_empty() {
        return Ok(0);
    }
    let mut total = 0;
    for folder in &folders {
        total += backfill_mailbox(pool, account.id, session, folder, &on_batch).await?;
    }
    Ok(total)
}

/// Backfill one folder: one UID SEARCH for the server's full list, diffed
/// against the cache, then page through the missing part newest-first,
/// upserting batch by batch.
///
/// why the full list and not just "below the oldest cached UID": holes can
/// sit inside the cached range too (builds before the batches were atomic
/// could lose arbitrary rows when a run was interrupted), and a below-only
/// sweep never revisits them — the progress strip then hangs short of the
/// server total forever.
async fn backfill_mailbox(
    pool: &SqlitePool,
    account_id: i64,
    session: &mut imap::ImapSession,
    mailbox: &str,
    on_batch: &impl Fn(),
) -> Result<usize, AppError> {
    let selected = session
        .select(mailbox)
        .await
        .map_err(|e| AppError::Imap(format!("select {mailbox}: {e}")))?;
    let server_validity = i64::from(selected.uid_validity.unwrap_or(0));
    crate::storage::mailboxes::set_server_exists(
        pool,
        account_id,
        mailbox,
        i64::from(selected.exists),
    )
    .await?;

    // Nothing cached yet — the regular sync owns a folder's first fetch.
    let cached = messages::uid_set(pool, account_id, mailbox).await?;
    if cached.is_empty() {
        return Ok(0);
    }
    let mut remaining = missing_uids(imap::search_all_uids(session).await?, &cached);
    let mut total = 0;
    while !remaining.is_empty() {
        let page = imap::older_uid_page(remaining.clone(), BACKFILL_BATCH);
        // why: page is never empty here, so the floor always exists and
        // remaining strictly shrinks — the loop terminates.
        let floor = *page.last().unwrap_or(&0);
        remaining.retain(|uid| *uid < floor);

        let uid_set = page
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let raw = imap::fetch_headers_by_uid(session, &uid_set).await?;
        // A whole page can vanish mid-run (deleted elsewhere) — move on.
        if raw.is_empty() {
            continue;
        }
        let headers: Vec<FetchedHeader> =
            raw.iter().map(|r| to_fetched(r, server_validity)).collect();
        messages::upsert_headers(pool, account_id, mailbox, &headers).await?;
        total += headers.len();
        on_batch();
    }
    Ok(total)
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
        &parsed.images,
        &parsed.attachments,
        parsed.auth.as_ref(),
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

/// A header row for a draft that exists only locally so far, parsed from
/// the raw bytes the save just built. `uid` is the caller's provisional
/// (negative) id; validity 0 is never consulted (see storage's uid > 0
/// guards). The snippet is filled by the caller from the parsed body.
pub(crate) fn to_provisional_header(raw: &[u8], uid: i64, has_attachments: bool) -> FetchedHeader {
    let parsed = parse::parse_header(raw);
    FetchedHeader {
        uid,
        uid_validity: 0,
        from: parsed.from,
        to: parsed.to,
        cc: parsed.cc,
        reply_to: parsed.reply_to,
        bcc: parsed.bcc,
        has_attachments,
        subject: parsed.subject,
        date: parsed.date,
        snippet: String::new(),
        // The user wrote this text — it must never look unread.
        read: true,
        message_id: parsed.message_id,
        in_reply_to: parsed.in_reply_to,
        references: parsed.references,
    }
}

fn to_fetched(raw: &imap::RawHeader, uid_validity: i64) -> FetchedHeader {
    let parsed = parse::parse_header(&raw.header);
    FetchedHeader {
        uid: raw.uid,
        uid_validity,
        from: parsed.from,
        to: parsed.to,
        cc: parsed.cc,
        reply_to: parsed.reply_to,
        bcc: parsed.bcc,
        has_attachments: raw.has_attachments,
        subject: parsed.subject,
        date: parsed.date,
        // why: header fetches carry no body — the snippet is filled in when
        // the body is first downloaded (get_message_body).
        snippet: String::new(),
        read: raw.read,
        message_id: parsed.message_id,
        in_reply_to: parsed.in_reply_to,
        references: parsed.references,
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
    fn a_folder_never_swept_is_due() {
        assert!(full_sweep_due(None, 1_000, FULL_SWEEP_INTERVAL));
    }

    #[test]
    fn a_folder_swept_within_the_interval_is_not_due() {
        assert!(!full_sweep_due(Some(1_000), 1_000, 3_600));
        assert!(!full_sweep_due(Some(1_000), 4_599, 3_600));
    }

    #[test]
    fn a_folder_swept_longer_ago_than_the_interval_is_due() {
        assert!(full_sweep_due(Some(1_000), 4_600, 3_600));
        assert!(full_sweep_due(Some(1_000), 99_999, 3_600));
    }

    #[test]
    fn a_clock_jump_backwards_makes_the_sweep_due() {
        // Otherwise the folder would stop reconciling until real time caught
        // up with the stale marker.
        assert!(full_sweep_due(Some(5_000), 1_000, 3_600));
    }

    #[test]
    fn the_reconcile_floor_is_the_lowest_uid_the_sweep_returned() {
        assert_eq!(reconcile_floor(&[(103, false), (101, true)]), Some(101));
        assert_eq!(reconcile_floor(&[(7, false)]), Some(7));
    }

    #[test]
    fn an_empty_sweep_has_no_reconcile_floor() {
        // The folder was not empty (or sweep_range would have said so), yet
        // nothing came back. Reconciling against that would read every
        // cached row as deleted and wipe the folder — skip instead.
        assert_eq!(reconcile_floor(&[]), None);
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
    fn missing_uids_finds_holes_anywhere_not_just_below_the_oldest() {
        let cached = std::collections::HashSet::from([2, 3, 9]);
        // 5 sits inside the cached range, 1 below it, 12 above — all three
        // are absent from the cache and all three must be mirrored.
        assert_eq!(
            missing_uids(vec![1, 2, 3, 5, 9, 12], &cached),
            vec![1, 5, 12]
        );
    }

    #[test]
    fn prefetch_plan_keeps_only_small_messages_with_a_known_size() {
        let missing = [(1, 101), (2, 102), (3, 103)];
        // 102 is over the cap; 103 never got a size back from the server.
        let sizes = [(101, 10_000), (102, 999_999)];

        assert_eq!(prefetch_plan(&missing, &sizes, 262_144), vec![(1, 101)]);
    }

    #[test]
    fn notifiable_keeps_only_unread_headers() {
        let make = |from: &str, subject: &str, read: bool| FetchedHeader {
            uid: 1,
            uid_validity: 7,
            from: from.to_string(),
            subject: subject.to_string(),
            read,
            ..Default::default()
        };
        let headers = [
            make("Alice <alice@example.com>", "Hello", false),
            make("Bob <bob@example.com>", "Read elsewhere", true),
            make("Cara <cara@example.com>", "Second", false),
        ];

        let new_mail = notifiable(&headers);

        assert_eq!(
            new_mail,
            vec![
                NewMail {
                    from: "Alice <alice@example.com>".to_string(),
                    subject: "Hello".to_string(),
                },
                NewMail {
                    from: "Cara <cara@example.com>".to_string(),
                    subject: "Second".to_string(),
                },
            ]
        );
    }

    #[test]
    fn to_fetched_maps_parsed_headers_and_leaves_snippet_empty() {
        let raw = imap::RawHeader {
            uid: 12,
            read: true,
            has_attachments: true,
            header: b"From: Alice <alice@example.com>\r\n\
                      To: Bob <bob@example.com>\r\n\
                      Cc: Cara <cara@example.com>\r\n\
                      Reply-To: Alice Team <team@example.com>\r\n\
                      Subject: Hi\r\n\
                      Message-ID: <mid@example.com>\r\n\
                      In-Reply-To: <parent@example.com>\r\n\
                      References: <root@example.com> <parent@example.com>\r\n\
                      Date: Tue, 07 Jul 2026 09:15:00 +0000\r\n\r\n"
                .to_vec(),
        };

        let fetched = to_fetched(&raw, 7);

        assert_eq!(fetched.uid, 12);
        assert_eq!(fetched.uid_validity, 7);
        assert_eq!(fetched.from, "Alice <alice@example.com>");
        assert_eq!(fetched.to, "Bob <bob@example.com>");
        assert_eq!(fetched.cc, "Cara <cara@example.com>");
        assert_eq!(fetched.reply_to, "Alice Team <team@example.com>");
        assert_eq!(fetched.subject, "Hi");
        assert!(fetched.read);
        assert_eq!(fetched.snippet, "");
        assert_eq!(fetched.message_id, "mid@example.com");
        assert_eq!(fetched.in_reply_to, "parent@example.com");
        assert_eq!(
            fetched.references,
            vec![
                "root@example.com".to_string(),
                "parent@example.com".to_string()
            ]
        );
    }
}
