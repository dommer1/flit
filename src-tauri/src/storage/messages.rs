use sqlx::SqlitePool;

use crate::error::AppError;
use crate::mail::parse::{snippet_of, AttachmentMeta, InlineImage};
use crate::models::{AuthResults, MessageAttachment, MessageHeader};

/// Header data as it arrives from an IMAP fetch, before it has a row id.
#[derive(Debug, Clone, Default)]
pub struct FetchedHeader {
    pub uid: i64,
    pub uid_validity: i64,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub reply_to: String,
    pub bcc: String,
    pub has_attachments: bool,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub read: bool,
    /// Message-ID without angle brackets; empty when the sender set none.
    pub message_id: String,
    /// The Message-ID this message replies to; empty for non-replies.
    pub in_reply_to: String,
    /// The References ancestor chain, root first.
    pub references: Vec<String>,
}

impl FetchedHeader {
    /// The identity set threading matches on: ancestors first (References
    /// starts at the root), then the direct parent, then the message itself.
    /// The first element doubles as the key when a new thread starts here.
    fn thread_refs(&self) -> Vec<String> {
        let mut refs: Vec<String> = Vec::new();
        for id in self
            .references
            .iter()
            .chain([&self.in_reply_to, &self.message_id])
        {
            if !id.is_empty() && !refs.contains(id) {
                refs.push(id.clone());
            }
        }
        refs
    }
}

/// Insert or refresh header rows. On conflict only the read flag is updated —
/// header fields don't change server-side, and body columns must survive.
pub async fn upsert_headers(
    pool: &SqlitePool,
    account_id: i64,
    mailbox: &str,
    headers: &[FetchedHeader],
) -> Result<(), AppError> {
    for header in headers {
        let thread_key = assign_thread_key(pool, account_id, header).await?;
        sqlx::query(
            "INSERT INTO messages
               (account_id, mailbox, uid, uid_validity, from_addr, to_addr, cc_addr, reply_to_addr, bcc_addr, subject, date, snippet, read, has_attachments,
                message_id_hdr, in_reply_to_hdr, references_hdr, thread_key)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (account_id, mailbox, uid) DO UPDATE SET read = excluded.read",
        )
        .bind(account_id)
        .bind(mailbox)
        .bind(header.uid)
        .bind(header.uid_validity)
        .bind(&header.from)
        .bind(&header.to)
        .bind(&header.cc)
        .bind(&header.reply_to)
        .bind(&header.bcc)
        .bind(&header.subject)
        .bind(&header.date)
        .bind(&header.snippet)
        .bind(header.read)
        .bind(header.has_attachments)
        .bind(&header.message_id)
        .bind(&header.in_reply_to)
        .bind(header.references.join(" "))
        .bind(&thread_key)
        .execute(pool)
        .await?;
        // why here: every sync path funnels through this upsert, so one hook
        // keeps the contacts book fed no matter how headers arrive.
        crate::storage::contacts::harvest(
            pool,
            &[
                &header.from,
                &header.to,
                &header.cc,
                &header.reply_to,
                &header.bcc,
            ],
            &header.date,
        )
        .await?;
    }
    Ok(())
}

/// Pick the thread key for an incoming message: adopt the key of any cached
/// relative (a message it references, or one whose thread key its chain
/// names), else start a new thread at the chain's root. A message bridging
/// several threads — its parents arrived out of order and were keyed apart —
/// merges them all onto one key.
///
/// why indexed-match only: a relative is found via message_id_hdr or
/// thread_key (both indexed). Scanning every row's references_hdr would also
/// catch two replies whose common parent never arrived, but that edge is rare
/// (Gmail misses it too) and not worth a table scan on every insert.
pub(crate) async fn assign_thread_key(
    pool: &SqlitePool,
    account_id: i64,
    header: &FetchedHeader,
) -> Result<Option<String>, AppError> {
    let refs = header.thread_refs();
    let Some(root) = refs.first() else {
        return Ok(None);
    };

    // Existing keys in refs order, so adoption prefers the root-most thread.
    let mut keys: Vec<String> = Vec::new();
    for reference in &refs {
        let found: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT thread_key FROM messages
             WHERE account_id = ? AND thread_key IS NOT NULL
               AND (message_id_hdr = ? OR thread_key = ?)",
        )
        .bind(account_id)
        .bind(reference)
        .bind(reference)
        .fetch_all(pool)
        .await?;
        for key in found {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }

    let Some(adopted) = keys.first() else {
        return Ok(Some(root.clone()));
    };
    for other in keys.iter().skip(1) {
        sqlx::query("UPDATE messages SET thread_key = ? WHERE account_id = ? AND thread_key = ?")
            .bind(adopted)
            .bind(account_id)
            .bind(other)
            .execute(pool)
            .await?;
    }
    Ok(Some(adopted.clone()))
}

/// Rows cached before the threading migration, `(row id, uid)` — their
/// header ids were never parsed (message_id_hdr IS NULL; new inserts always
/// store at least ''). Oldest first, so roots get keyed before replies when
/// the backfill walks them.
pub async fn rows_missing_threading(
    pool: &SqlitePool,
    account_id: i64,
    mailbox: &str,
) -> Result<Vec<(i64, i64)>, AppError> {
    Ok(sqlx::query_as(
        "SELECT id, uid FROM messages
         WHERE account_id = ? AND mailbox = ? AND message_id_hdr IS NULL
         ORDER BY date",
    )
    .bind(account_id)
    .bind(mailbox)
    .fetch_all(pool)
    .await?)
}

/// Repair one pre-migration row: store its re-fetched header ids and key it
/// into a thread with the same adoption/merge logic fresh inserts use.
pub async fn backfill_threading(
    pool: &SqlitePool,
    account_id: i64,
    row_id: i64,
    header: &FetchedHeader,
) -> Result<(), AppError> {
    let thread_key = assign_thread_key(pool, account_id, header).await?;
    sqlx::query(
        "UPDATE messages
         SET message_id_hdr = ?, in_reply_to_hdr = ?, references_hdr = ?, thread_key = ?
         WHERE id = ?",
    )
    .bind(&header.message_id)
    .bind(&header.in_reply_to)
    .bind(header.references.join(" "))
    .bind(&thread_key)
    .bind(row_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Recompute stored snippets that still carry a URL, using the current
/// `snippet_of` rules. Runs once at startup: rows already cached by an older
/// build kept the raw URL the parser now strips, and a re-fetch would never
/// touch them (their body is present). The `LIKE '%http%'` filter is both the
/// work-list and the idempotency guard — a corrected snippet no longer matches,
/// so a second startup finds nothing to do.
pub async fn backfill_url_snippets(pool: &SqlitePool) -> Result<u64, AppError> {
    let stale: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, body_text FROM messages
         WHERE body_text IS NOT NULL AND snippet LIKE '%http%'",
    )
    .fetch_all(pool)
    .await?;

    let mut fixed = 0;
    for (id, body_text) in stale {
        let snippet = snippet_of(&body_text);
        sqlx::query("UPDATE messages SET snippet = ? WHERE id = ?")
            .bind(&snippet)
            .bind(id)
            .execute(pool)
            .await?;
        fixed += 1;
    }
    Ok(fixed)
}

/// Recompute stored snippets that still carry quoted history ("Uhradené.
/// \> On Monday, X wrote: …"), using the current snippet rules. Runs once
/// at startup: the LIKE filter doubles as the work-list — a corrected
/// snippet no longer matches it (fully-quoted bodies re-run, as cheap
/// no-ops).
pub async fn backfill_quoted_snippets(pool: &SqlitePool) -> Result<u64, AppError> {
    let stale: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, body_text FROM messages
         WHERE body_text IS NOT NULL
           AND (snippet LIKE '%> %' OR snippet LIKE '%wrote:%'
                OR snippet LIKE '%napísal%' OR snippet LIKE '%Original Message%')",
    )
    .fetch_all(pool)
    .await?;

    let mut fixed = 0;
    for (id, body_text) in stale {
        let snippet = snippet_of(&body_text);
        let changed = sqlx::query("UPDATE messages SET snippet = ? WHERE id = ? AND snippet <> ?")
            .bind(&snippet)
            .bind(id)
            .bind(&snippet)
            .execute(pool)
            .await?;
        fixed += changed.rows_affected();
    }
    Ok(fixed)
}

/// Recompute stored snippets that still carry literal entity padding
/// ("Bistro.sk &zwnj; &zwnj; …"), using the current snippet rules. Runs
/// once at startup: the LIKE filter doubles as the work-list — a corrected
/// snippet no longer matches it (rows with legit "&#" prose re-run, as
/// cheap no-ops).
pub async fn backfill_entity_snippets(pool: &SqlitePool) -> Result<u64, AppError> {
    let stale: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, body_text FROM messages
         WHERE body_text IS NOT NULL
           AND (snippet LIKE '%&zwnj;%' OR snippet LIKE '%&zwj;%'
                OR snippet LIKE '%&shy;%' OR snippet LIKE '%&nbsp;%'
                OR snippet LIKE '%&#%')",
    )
    .fetch_all(pool)
    .await?;

    let mut fixed = 0;
    for (id, body_text) in stale {
        let snippet = snippet_of(&body_text);
        let changed = sqlx::query("UPDATE messages SET snippet = ? WHERE id = ? AND snippet <> ?")
            .bind(&snippet)
            .bind(id)
            .bind(&snippet)
            .execute(pool)
            .await?;
        fixed += changed.rows_affected();
    }
    Ok(fixed)
}

/// Headers of one mailbox for one account — or across all accounts when
/// `account_id` is `None` (the "All Inboxes" view), newest first. `limit`
/// caps the rows (the UI reveals more as the user scrolls); `None` = all.
pub async fn list(
    pool: &SqlitePool,
    account_id: Option<i64>,
    mailbox: &str,
    limit: Option<i64>,
) -> Result<Vec<MessageHeader>, AppError> {
    // why unwrap_or(-1): SQLite treats a negative LIMIT as "no limit", which
    // keeps the query one static string (sqlx 0.9 rejects runtime-built SQL).
    let limit = limit.unwrap_or(-1);
    let headers = match account_id {
        Some(id) => {
            sqlx::query_as(
                r#"SELECT id, account_id, mailbox, from_addr AS "from", to_addr AS "to",
                          cc_addr AS cc, reply_to_addr AS reply_to, bcc_addr AS bcc, subject, snippet, date, read, has_attachments,
                          COALESCE(message_id_hdr, '') AS message_id,
                          references_hdr AS "references"
                   FROM messages WHERE account_id = ? AND mailbox = ? ORDER BY date DESC LIMIT ?"#,
            )
            .bind(id)
            .bind(mailbox)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                r#"SELECT id, account_id, mailbox, from_addr AS "from", to_addr AS "to",
                          cc_addr AS cc, reply_to_addr AS reply_to, bcc_addr AS bcc, subject, snippet, date, read, has_attachments,
                          COALESCE(message_id_hdr, '') AS message_id,
                          references_hdr AS "references"
                   FROM messages WHERE mailbox = ? ORDER BY date DESC LIMIT ?"#,
            )
            .bind(mailbox)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(headers)
}

/// One row per conversation for the viewed mailbox, newest first — the
/// newest message in the folder represents its thread. Unlike `list`, the
/// count/unread columns look across the whole account (minus trash, junk
/// and drafts), matching what the conversation view shows on click.
///
/// why 'row:'||id as the fallback key: rows without a Message-ID can never
/// thread, so each groups under a synthetic key only it can match.
/// why COUNT(DISTINCT mkey): servers list one RFC message in several folders
/// (Gmail's All Mail, aliased Sent) — copies share a Message-ID and must
/// count once. Stats come from one GROUP BY pass instead of two correlated
/// subqueries per row, which re-scanned the whole account for every row.
pub async fn list_threaded(
    pool: &SqlitePool,
    account_id: Option<i64>,
    mailbox: &str,
    limit: Option<i64>,
) -> Result<Vec<MessageHeader>, AppError> {
    let rows = sqlx::query_as(
        r#"WITH visible AS (
             SELECT t.account_id,
                    COALESCE(t.thread_key, 'row:' || t.id) AS tkey,
                    COALESCE(NULLIF(t.message_id_hdr, ''), 'row:' || t.id) AS mkey,
                    t.read, t.has_attachments,
                    (COALESCE(b.role, '') = 'drafts') AS is_draft
             FROM messages t
             LEFT JOIN mailboxes b ON b.account_id = t.account_id AND b.name = t.mailbox
             WHERE COALESCE(b.role, '') NOT IN ('trash', 'junk')
           ),
           stats AS (
             -- Drafts only feed the has-draft flag: an unsent reply is not
             -- a message of the exchange, so it counts into nothing else.
             SELECT account_id, tkey,
                    COUNT(DISTINCT CASE WHEN NOT is_draft THEN mkey END) AS thread_count,
                    MAX(CASE WHEN read = 0 AND NOT is_draft THEN 1 ELSE 0 END) AS thread_unread,
                    MAX(CASE WHEN is_draft THEN 0 ELSE has_attachments END) AS thread_attachments,
                    MAX(is_draft) AS thread_has_draft
             FROM visible
             GROUP BY account_id, tkey
           ),
           ranked AS (
             SELECT m.*, ROW_NUMBER() OVER (
                      PARTITION BY m.account_id, COALESCE(m.thread_key, 'row:' || m.id)
                      ORDER BY m.date DESC, m.id DESC
                    ) AS rn
             FROM messages m
             WHERE m.mailbox = ?2 AND (?1 IS NULL OR m.account_id = ?1)
           )
           SELECT r.id, r.account_id, r.mailbox, r.from_addr AS "from", r.to_addr AS "to",
                  r.cc_addr AS cc, r.reply_to_addr AS reply_to, r.bcc_addr AS bcc,
                  r.subject, r.snippet,
                  r.date, r.read, s.thread_attachments AS has_attachments,
                  COALESCE(r.message_id_hdr, '') AS message_id,
                  r.references_hdr AS "references",
                  s.thread_count, s.thread_unread, s.thread_has_draft
           FROM ranked r
           JOIN stats s ON s.account_id = r.account_id
                       AND s.tkey = COALESCE(r.thread_key, 'row:' || r.id)
           WHERE r.rn = 1 ORDER BY r.date DESC LIMIT ?3"#,
    )
    .bind(account_id)
    .bind(mailbox)
    .bind(limit.unwrap_or(-1))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// The whole conversation of one message, oldest first, across the folders
/// the thread view shows (everything but trash/junk; an unsent reply in
/// Drafts rides along flagged `is_draft`, so the view can badge it). The
/// anchor itself is always included — even keyless or sitting in Trash —
/// so the viewer never comes up empty for the message the user clicked.
///
/// why the copy_rank window: one RFC message can be cached from several
/// folders (Gmail's All Mail lists everything again, some servers alias
/// Sent under two names) — the conversation shows it once, preferring the
/// copy outside the all/archive containers.
pub async fn thread_of(pool: &SqlitePool, message_id: i64) -> Result<Vec<MessageHeader>, AppError> {
    let rows = sqlx::query_as(
        r#"SELECT id, account_id, mailbox, "from", "to", cc, reply_to, bcc, subject, snippet,
                  date, read, has_attachments, message_id, "references", is_draft
           FROM (
             SELECT m.id, m.account_id, m.mailbox, m.from_addr AS "from", m.to_addr AS "to",
                    m.cc_addr AS cc, m.reply_to_addr AS reply_to, m.bcc_addr AS bcc,
                    m.subject, m.snippet,
                    m.date, m.read, m.has_attachments,
                    COALESCE(m.message_id_hdr, '') AS message_id,
                    m.references_hdr AS "references",
                    (COALESCE(b.role, '') = 'drafts') AS is_draft,
                    ROW_NUMBER() OVER (
                      PARTITION BY COALESCE(NULLIF(m.message_id_hdr, ''), 'row:' || m.id)
                      ORDER BY (COALESCE(b.role, '') IN ('all', 'archive')), m.id
                    ) AS copy_rank
             FROM messages m
             JOIN messages a ON a.id = ?1 AND m.account_id = a.account_id
             LEFT JOIN mailboxes b ON b.account_id = m.account_id AND b.name = m.mailbox
             WHERE m.id = a.id
                OR (a.thread_key IS NOT NULL AND m.thread_key = a.thread_key
                    AND COALESCE(b.role, '') NOT IN ('trash', 'junk'))
           )
           WHERE copy_rank = 1
           ORDER BY date ASC, id ASC"#,
    )
    .bind(message_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// `(row id, uid)` of every thread member sharing the anchor's folder,
/// anchor included — the unit a thread-row action (archive/trash/move)
/// operates on. Keyless anchors act alone.
pub async fn thread_rows_in_mailbox(
    pool: &SqlitePool,
    message_id: i64,
) -> Result<Vec<(i64, i64)>, AppError> {
    Ok(sqlx::query_as(
        "SELECT m.id, m.uid FROM messages m
         JOIN messages a ON a.id = ?
         WHERE m.account_id = a.account_id AND m.mailbox = a.mailbox
           AND (m.id = a.id OR (a.thread_key IS NOT NULL AND m.thread_key = a.thread_key))
         ORDER BY m.uid",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await?)
}

/// Highest cached UID for incremental sync; `None` when nothing is cached.
pub async fn max_uid(
    pool: &SqlitePool,
    account_id: i64,
    mailbox: &str,
) -> Result<Option<i64>, AppError> {
    let uid =
        sqlx::query_scalar("SELECT MAX(uid) FROM messages WHERE account_id = ? AND mailbox = ?")
            .bind(account_id)
            .bind(mailbox)
            .fetch_one(pool)
            .await?;
    Ok(uid)
}

/// Counts for one list view; `account_id: None` spans all accounts (the
/// unified inbox). `threaded` must match how the view lists rows so
/// `list_rows` agrees with what an unlimited `list`/`list_threaded` returns.
///
/// Header totals (`list_rows`, `unread`) are scoped to the viewed folder;
/// the progress pair (`cached`, `server_total`) spans every folder of the
/// account (or of all accounts) — the user asked to see the backfill from
/// any view, not just the folder it happens to be working on.
pub async fn view_status(
    pool: &SqlitePool,
    account_id: Option<i64>,
    mailbox: &str,
    threaded: bool,
) -> Result<crate::models::ViewStatus, AppError> {
    let (view_count, unread): (i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(SUM(read = 0), 0) FROM messages
         WHERE mailbox = ?2 AND (?1 IS NULL OR account_id = ?1)",
    )
    .bind(account_id)
    .bind(mailbox)
    .fetch_one(pool)
    .await?;
    let list_rows = if threaded {
        // Mirrors list_threaded's grouping: one row per (account, thread),
        // keyless messages standing alone under a synthetic per-row key.
        sqlx::query_scalar(
            "SELECT count(DISTINCT account_id || '/' || COALESCE(thread_key, 'row:' || id))
             FROM messages WHERE mailbox = ?2 AND (?1 IS NULL OR account_id = ?1)",
        )
        .bind(account_id)
        .bind(mailbox)
        .fetch_one(pool)
        .await?
    } else {
        view_count
    };
    let cached: i64 =
        sqlx::query_scalar("SELECT count(*) FROM messages WHERE ?1 IS NULL OR account_id = ?1")
            .bind(account_id)
            .fetch_one(pool)
            .await?;
    // SUM of NULLs is NULL — folders never synced report no total rather
    // than a misleading zero.
    let server_total: Option<i64> = sqlx::query_scalar(
        "SELECT SUM(server_exists) FROM mailboxes WHERE ?1 IS NULL OR account_id = ?1",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await?;
    Ok(crate::models::ViewStatus {
        list_rows,
        unread,
        cached,
        server_total,
    })
}

/// Lowest cached UID — where the header backfill continues downwards from;
/// `None` when nothing is cached.
pub async fn min_uid(
    pool: &SqlitePool,
    account_id: i64,
    mailbox: &str,
) -> Result<Option<i64>, AppError> {
    let uid =
        sqlx::query_scalar("SELECT MIN(uid) FROM messages WHERE account_id = ? AND mailbox = ?")
            .bind(account_id)
            .bind(mailbox)
            .fetch_one(pool)
            .await?;
    Ok(uid)
}

/// UIDVALIDITY the cache was built against; `None` when nothing is cached.
pub async fn stored_uid_validity(
    pool: &SqlitePool,
    account_id: i64,
    mailbox: &str,
) -> Result<Option<i64>, AppError> {
    let validity = sqlx::query_scalar(
        "SELECT uid_validity FROM messages WHERE account_id = ? AND mailbox = ? LIMIT 1",
    )
    .bind(account_id)
    .bind(mailbox)
    .fetch_optional(pool)
    .await?;
    Ok(validity)
}

/// What a body request needs from the cache: the stored bodies plus the
/// coordinates (account, mailbox, uid) for a lazy server fetch on miss.
#[derive(Debug, sqlx::FromRow)]
pub struct BodyRow {
    pub account_id: i64,
    pub mailbox: String,
    pub uid: i64,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    /// False for bodies cached before attachment metadata existed — the
    /// next open refetches once to harvest it.
    pub attachments_scanned: bool,
    /// Cached AuthResults as JSON; None = unknown (no header, or the body
    /// was cached before verdicts were harvested).
    pub auth_results: Option<String>,
    /// The stored From line ("Name <addr>"), for the sender check.
    pub from_addr: String,
}

pub async fn get_body(pool: &SqlitePool, message_id: i64) -> Result<BodyRow, AppError> {
    let row = sqlx::query_as(
        "SELECT account_id, mailbox, uid, body_text, body_html, attachments_scanned, auth_results,
                from_addr
         FROM messages WHERE id = ?",
    )
    .bind(message_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Store a freshly fetched body; the snippet becomes real now that text exists.
///
/// why text coalesces to "": NULL in both body columns means "never fetched"
/// (see uids_missing_body). A message whose download parsed to nothing —
/// attachment-only mail, unparsable MIME — must still count as cached, or
/// the prefetcher would re-download it on every sync forever.
// why allow: the args mirror ParsedBody's fields one-to-one; tests set
// each independently, so bundling them into a param struct here would
// just add a second shape for the same data.
#[allow(clippy::too_many_arguments)]
pub async fn set_body(
    pool: &SqlitePool,
    message_id: i64,
    text: Option<&str>,
    html: Option<&str>,
    snippet: &str,
    images: &[InlineImage],
    attachments: &[AttachmentMeta],
    auth: Option<&AuthResults>,
) -> Result<(), AppError> {
    // why .ok() instead of ?: a verdict that somehow fails to serialize
    // must not lose the whole body write.
    let auth_json = auth.and_then(|a| serde_json::to_string(a).ok());
    // why attachments_scanned = 1: this body was parsed by a build that
    // harvests attachment metadata, so no rescan is ever needed for it.
    sqlx::query(
        "UPDATE messages SET body_text = ?, body_html = ?, snippet = ?,
                             has_attachments = ?, attachments_scanned = 1,
                             auth_results = ? WHERE id = ?",
    )
    .bind(text.unwrap_or(""))
    .bind(html)
    .bind(snippet)
    .bind(!attachments.is_empty())
    .bind(auth_json)
    .bind(message_id)
    .execute(pool)
    .await?;
    // why: images live inside set_body, not a separate call — one write path
    // means a cached body can never drift apart from its cid images.
    sqlx::query("DELETE FROM message_images WHERE message_id = ?")
        .bind(message_id)
        .execute(pool)
        .await?;
    for image in images {
        sqlx::query(
            "INSERT INTO message_images (message_id, content_id, content_type, data)
             VALUES (?, ?, ?, ?)",
        )
        .bind(message_id)
        .bind(&image.content_id)
        .bind(&image.content_type)
        .bind(&image.data)
        .execute(pool)
        .await?;
    }
    // Same reasoning: attachment metadata always mirrors the cached body.
    sqlx::query("DELETE FROM message_attachments WHERE message_id = ?")
        .bind(message_id)
        .execute(pool)
        .await?;
    for attachment in attachments {
        sqlx::query(
            "INSERT INTO message_attachments (message_id, part_index, filename, content_type, size)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(message_id)
        .bind(attachment.part_index)
        .bind(&attachment.filename)
        .bind(&attachment.content_type)
        .bind(attachment.size)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Attachment metadata of one cached message, in part order.
pub async fn attachments(
    pool: &SqlitePool,
    message_id: i64,
) -> Result<Vec<MessageAttachment>, AppError> {
    let rows = sqlx::query_as(
        "SELECT id, message_id, part_index, filename, content_type, size
         FROM message_attachments WHERE message_id = ? ORDER BY part_index",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// One attachment row by id — what the save commands start from.
pub async fn attachment(
    pool: &SqlitePool,
    attachment_id: i64,
) -> Result<MessageAttachment, AppError> {
    let row = sqlx::query_as(
        "SELECT id, message_id, part_index, filename, content_type, size
         FROM message_attachments WHERE id = ?",
    )
    .bind(attachment_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// The cid: images cached for one message, for resolving `src="cid:..."`
/// references at render time.
pub async fn images(pool: &SqlitePool, message_id: i64) -> Result<Vec<InlineImage>, AppError> {
    let rows: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT content_id, content_type, data FROM message_images WHERE message_id = ?",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(content_id, content_type, data)| InlineImage {
            content_id,
            content_type,
            data,
        })
        .collect())
}

/// The body-prefetch work-list: `(id, uid)` of messages with no cached body,
/// newest first so recent mail becomes searchable soonest.
pub async fn uids_missing_body(
    pool: &SqlitePool,
    account_id: i64,
    mailbox: &str,
    limit: i64,
) -> Result<Vec<(i64, i64)>, AppError> {
    let rows = sqlx::query_as(
        "SELECT id, uid FROM messages
         WHERE account_id = ? AND mailbox = ?
           AND body_text IS NULL AND body_html IS NULL
         ORDER BY date DESC
         LIMIT ?",
    )
    .bind(account_id)
    .bind(mailbox)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Whether any cached message of this account, in any folder, still lacks
/// a body — lets the prefetcher skip connecting when there is nothing to do.
pub async fn has_missing_bodies(pool: &SqlitePool, account_id: i64) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messages
         WHERE account_id = ? AND body_text IS NULL AND body_html IS NULL",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

/// `(id, uid, read)` of every cached row in one folder, uid order — the
/// local side of reconciliation.
pub async fn uid_flags(
    pool: &SqlitePool,
    account_id: i64,
    mailbox: &str,
) -> Result<Vec<(i64, i64, bool)>, AppError> {
    let rows = sqlx::query_as(
        "SELECT id, uid, read FROM messages
         WHERE account_id = ? AND mailbox = ? ORDER BY uid",
    )
    .bind(account_id)
    .bind(mailbox)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Row id of one folder's cached message by Message-ID header — how a
/// just-appended draft is found again after its folder syncs.
pub async fn find_by_message_id(
    pool: &SqlitePool,
    account_id: i64,
    mailbox: &str,
    message_id: &str,
) -> Result<Option<i64>, AppError> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM messages
         WHERE account_id = ? AND mailbox = ? AND message_id_hdr = ?",
    )
    .bind(account_id)
    .bind(mailbox)
    .bind(message_id)
    .fetch_optional(pool)
    .await?)
}

/// The compose fields of a cached draft — what reopening a draft needs,
/// mirroring `mail::parse::parse_draft`'s output shape (bare Message-IDs,
/// space-joined references, empty string = absent).
#[derive(Debug, sqlx::FromRow)]
pub struct CachedDraft {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    /// The stored From line ("Name <addr>"), for alias matching.
    pub from_addr: String,
    pub message_id: String,
    pub in_reply_to: String,
    pub references: String,
}

/// A draft the cache can serve whole: body cached and provably no
/// attachments. Attachment bytes are never cached, and an unscanned body
/// could hide some — both cases return None and reopen via the server.
pub async fn cached_draft(
    pool: &SqlitePool,
    message_id: i64,
) -> Result<Option<CachedDraft>, AppError> {
    Ok(sqlx::query_as(
        r#"SELECT to_addr AS "to", cc_addr AS cc, bcc_addr AS bcc, subject,
                  body_text AS body, from_addr,
                  COALESCE(message_id_hdr, '') AS message_id,
                  COALESCE(in_reply_to_hdr, '') AS in_reply_to,
                  COALESCE(references_hdr, '') AS "references"
           FROM messages m
           WHERE id = ? AND body_text IS NOT NULL AND attachments_scanned = 1
             AND NOT EXISTS (SELECT 1 FROM message_attachments a WHERE a.message_id = m.id)"#,
    )
    .bind(message_id)
    .fetch_optional(pool)
    .await?)
}

/// Remove one cached message (it vanished from the folder server-side).
pub async fn delete_by_id(pool: &SqlitePool, message_id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM messages WHERE id = ?")
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Where a cached message lives on the server — the coordinates a UID
/// command (STORE, MOVE) needs.
#[derive(Debug, sqlx::FromRow)]
pub struct MessageLocation {
    pub account_id: i64,
    pub mailbox: String,
    pub uid: i64,
}

/// Look up one message's server coordinates by row id.
pub async fn location(pool: &SqlitePool, message_id: i64) -> Result<MessageLocation, AppError> {
    let row = sqlx::query_as("SELECT account_id, mailbox, uid FROM messages WHERE id = ?")
        .bind(message_id)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

/// Adopt the server's read/unread state for one message.
pub async fn set_read(pool: &SqlitePool, message_id: i64, read: bool) -> Result<(), AppError> {
    sqlx::query("UPDATE messages SET read = ? WHERE id = ?")
        .bind(read)
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Drop the cache for one mailbox — required when UIDVALIDITY changes
/// (RFC 3501: old UIDs are meaningless after that).
pub async fn clear_mailbox(
    pool: &SqlitePool,
    account_id: i64,
    mailbox: &str,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM messages WHERE account_id = ? AND mailbox = ?")
        .bind(account_id)
        .bind(mailbox)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAccount;
    use crate::storage::{accounts, test_pool};

    async fn account(pool: &SqlitePool, name: &str) -> i64 {
        accounts::insert(
            pool,
            &NewAccount {
                name: name.to_string(),
                email: format!("{}@example.com", name.to_lowercase()),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                username: format!("{}@example.com", name.to_lowercase()),
            },
        )
        .await
        .unwrap()
        .id
    }

    fn header(uid: i64, subject: &str, date: &str, read: bool) -> FetchedHeader {
        FetchedHeader {
            uid,
            uid_validity: 7,
            from: "Alice <alice@example.com>".to_string(),
            to: "Bob <bob@example.com>".to_string(),
            cc: "Cara <cara@example.com>".to_string(),
            reply_to: "Alice Reply <reply@example.com>".to_string(),
            subject: subject.to_string(),
            date: date.to_string(),
            snippet: format!("snippet of {subject}"),
            read,
            ..Default::default()
        }
    }

    /// A header whose threading identity matters and nothing else does.
    fn threaded(
        uid: i64,
        message_id: &str,
        in_reply_to: &str,
        references: &[&str],
    ) -> FetchedHeader {
        FetchedHeader {
            uid,
            uid_validity: 7,
            date: format!("2026-07-{uid:02}T00:00:00Z"),
            message_id: message_id.to_string(),
            in_reply_to: in_reply_to.to_string(),
            references: references.iter().map(|r| r.to_string()).collect(),
            ..Default::default()
        }
    }

    async fn thread_keys(pool: &SqlitePool, account_id: i64) -> Vec<Option<String>> {
        sqlx::query_scalar(
            "SELECT thread_key FROM messages WHERE account_id = ? ORDER BY mailbox, uid",
        )
        .bind(account_id)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn replies_inherit_the_roots_thread_key() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(2, "b@x", "a@x", &["a@x"]),
                threaded(3, "c@x", "b@x", &["a@x", "b@x"]),
                threaded(4, "unrelated@x", "", &[]),
            ],
        )
        .await
        .unwrap();

        let keys = thread_keys(&pool, id).await;
        assert_eq!(keys[0], keys[1]);
        assert_eq!(keys[1], keys[2]);
        assert!(keys[0].is_some());
        assert_ne!(keys[3], keys[0]);
    }

    #[tokio::test]
    async fn threads_span_mailboxes_within_an_account() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(&pool, id, "INBOX", &[threaded(1, "a@x", "", &[])])
            .await
            .unwrap();
        upsert_headers(&pool, id, "Sent", &[threaded(1, "b@x", "a@x", &["a@x"])])
            .await
            .unwrap();

        let keys = thread_keys(&pool, id).await;
        assert_eq!(keys[0], keys[1]);
        assert!(keys[0].is_some());
    }

    #[tokio::test]
    async fn out_of_order_arrival_still_threads() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        // The reply lands in the cache before the message it answers.
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "c@x", "b@x", &["a@x", "b@x"]),
                threaded(2, "a@x", "", &[]),
            ],
        )
        .await
        .unwrap();

        let keys = thread_keys(&pool, id).await;
        assert_eq!(keys[0], keys[1]);
    }

    #[tokio::test]
    async fn bridge_message_merges_split_threads() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        // A and C arrive first with no visible link (C only knows its direct
        // parent B) — two separate threads until B arrives and bridges them.
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[threaded(1, "a@x", "", &[]), threaded(2, "c@x", "b@x", &[])],
        )
        .await
        .unwrap();
        let before = thread_keys(&pool, id).await;
        assert_ne!(before[0], before[1]);

        upsert_headers(&pool, id, "INBOX", &[threaded(3, "b@x", "a@x", &["a@x"])])
            .await
            .unwrap();

        let keys = thread_keys(&pool, id).await;
        assert_eq!(keys[0], keys[1]);
        assert_eq!(keys[1], keys[2]);
    }

    /// Simulate a row cached by a build without the threading columns.
    async fn null_out_threading(pool: &SqlitePool, account_id: i64, uid: i64) {
        sqlx::query(
            "UPDATE messages SET message_id_hdr = NULL, in_reply_to_hdr = '',
             references_hdr = '', thread_key = NULL WHERE account_id = ? AND uid = ?",
        )
        .bind(account_id)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn rows_missing_threading_lists_only_unparsed_rows() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[threaded(1, "a@x", "", &[]), threaded(2, "", "", &[])],
        )
        .await
        .unwrap();
        null_out_threading(&pool, id, 1).await;

        let missing = rows_missing_threading(&pool, id, "INBOX").await.unwrap();

        // uid 2 was parsed (its sender just set no Message-ID) — only the
        // NULLed pre-migration row needs a backfill.
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].1, 1);
    }

    #[tokio::test]
    async fn backfill_threading_joins_a_legacy_row_to_its_thread() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(2, "b@x", "a@x", &["a@x"]),
            ],
        )
        .await
        .unwrap();
        null_out_threading(&pool, id, 2).await;
        let (row_id, _) = rows_missing_threading(&pool, id, "INBOX").await.unwrap()[0];

        backfill_threading(&pool, id, row_id, &threaded(2, "b@x", "a@x", &["a@x"]))
            .await
            .unwrap();

        let keys = thread_keys(&pool, id).await;
        assert_eq!(keys[0], keys[1]);
        assert!(keys[0].is_some());
        assert!(rows_missing_threading(&pool, id, "INBOX")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn list_exposes_threading_identity_for_replies() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(2, "b@x", "a@x", &["a@x"]),
            ],
        )
        .await
        .unwrap();

        let all = list(&pool, Some(id), "INBOX", None).await.unwrap();

        let reply = all.iter().find(|m| m.message_id == "b@x").unwrap();
        assert_eq!(reply.references, "a@x");
        let root = all.iter().find(|m| m.message_id == "a@x").unwrap();
        assert_eq!(root.references, "");
    }

    /// Folder roles the thread queries depend on: which folders count into
    /// a conversation (not trash/junk/drafts) comes from the mailboxes table.
    async fn seed_roles(pool: &SqlitePool, account_id: i64) {
        use crate::storage::mailboxes::DiscoveredMailbox;
        let found: Vec<DiscoveredMailbox> = [
            ("INBOX", Some("inbox")),
            ("Sent", Some("sent")),
            ("Trash", Some("trash")),
            ("Drafts", Some("drafts")),
        ]
        .into_iter()
        .map(|(name, role)| DiscoveredMailbox {
            name: name.to_string(),
            role: role.map(str::to_string),
        })
        .collect();
        crate::storage::mailboxes::replace(pool, account_id, &found)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn threaded_list_shows_one_row_per_conversation() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(3, "b@x", "a@x", &["a@x"]),
                threaded(4, "solo@x", "", &[]),
            ],
        )
        .await
        .unwrap();

        let rows = list_threaded(&pool, Some(id), "INBOX", None).await.unwrap();

        assert_eq!(rows.len(), 2);
        // Newest first: the solo message (uid 4 → date 07-04) precedes the
        // thread, represented by its newest inbox message (uid 3).
        assert_eq!(rows[0].message_id, "solo@x");
        assert_eq!(rows[0].thread_count, 1);
        assert_eq!(rows[1].message_id, "b@x");
        assert_eq!(rows[1].thread_count, 2);
    }

    #[tokio::test]
    async fn thread_count_spans_folders_but_trash_stays_out() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        upsert_headers(&pool, id, "INBOX", &[threaded(1, "a@x", "", &[])])
            .await
            .unwrap();
        upsert_headers(&pool, id, "Sent", &[threaded(1, "b@x", "a@x", &["a@x"])])
            .await
            .unwrap();
        upsert_headers(&pool, id, "Trash", &[threaded(1, "c@x", "a@x", &["a@x"])])
            .await
            .unwrap();

        let rows = list_threaded(&pool, Some(id), "INBOX", None).await.unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].thread_count, 2);
    }

    #[tokio::test]
    async fn thread_unread_flags_any_unread_sibling() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        let mut root = threaded(1, "a@x", "", &[]);
        root.read = false;
        let mut reply = threaded(2, "b@x", "a@x", &["a@x"]);
        reply.read = true;
        upsert_headers(&pool, id, "INBOX", &[root, reply])
            .await
            .unwrap();

        let rows = list_threaded(&pool, Some(id), "INBOX", None).await.unwrap();

        assert_eq!(rows.len(), 1);
        // The representative (newest) is read, but the thread is not.
        assert!(rows[0].read);
        assert!(rows[0].thread_unread);
    }

    #[tokio::test]
    async fn keyless_messages_stay_single_rows() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[threaded(1, "", "", &[]), threaded(2, "", "", &[])],
        )
        .await
        .unwrap();

        let rows = list_threaded(&pool, Some(id), "INBOX", None).await.unwrap();

        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.thread_count == 1));
    }

    #[tokio::test]
    async fn unified_threaded_list_keeps_accounts_apart() {
        let pool = test_pool().await;
        let first = account(&pool, "Personal").await;
        let second = account(&pool, "Work").await;
        seed_roles(&pool, first).await;
        seed_roles(&pool, second).await;
        upsert_headers(&pool, first, "INBOX", &[threaded(1, "a@x", "", &[])])
            .await
            .unwrap();
        upsert_headers(&pool, second, "INBOX", &[threaded(1, "a@x", "", &[])])
            .await
            .unwrap();

        let rows = list_threaded(&pool, None, "INBOX", None).await.unwrap();

        // Identical thread keys in different accounts must not collapse.
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn thread_of_returns_the_conversation_oldest_first_across_folders() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(3, "c@x", "b@x", &["a@x", "b@x"]),
            ],
        )
        .await
        .unwrap();
        upsert_headers(&pool, id, "Sent", &[threaded(2, "b@x", "a@x", &["a@x"])])
            .await
            .unwrap();
        upsert_headers(
            &pool,
            id,
            "Trash",
            &[threaded(4, "d@x", "c@x", &["a@x", "b@x", "c@x"])],
        )
        .await
        .unwrap();
        let anchor = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        let thread = thread_of(&pool, anchor).await.unwrap();

        let mids: Vec<&str> = thread.iter().map(|m| m.message_id.as_str()).collect();
        // Oldest first, Sent included, Trash excluded.
        assert_eq!(mids, vec!["a@x", "b@x", "c@x"]);
    }

    #[tokio::test]
    async fn find_by_message_id_scopes_to_account_and_mailbox() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(&pool, id, "Drafts", &[threaded(1, "d@x", "", &[])])
            .await
            .unwrap();
        upsert_headers(&pool, id, "INBOX", &[threaded(1, "a@x", "", &[])])
            .await
            .unwrap();

        let found = find_by_message_id(&pool, id, "Drafts", "d@x")
            .await
            .unwrap();
        assert!(found.is_some());

        // Wrong folder, unknown id, wrong account: all misses.
        assert!(find_by_message_id(&pool, id, "INBOX", "d@x")
            .await
            .unwrap()
            .is_none());
        assert!(find_by_message_id(&pool, id, "Drafts", "nope@x")
            .await
            .unwrap()
            .is_none());
        assert!(find_by_message_id(&pool, 999, "Drafts", "d@x")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn cached_draft_serves_a_bodied_attachment_free_draft() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        let mut draft = threaded(1, "d@x", "root@x", &["root@x"]);
        draft.from = "Me <me@example.com>".to_string();
        draft.to = "Alice <alice@example.com>".to_string();
        draft.subject = "Re: plans".to_string();
        upsert_headers(&pool, id, "Drafts", &[draft]).await.unwrap();
        let row_id = list(&pool, Some(id), "Drafts", None).await.unwrap()[0].id;

        // Body not cached yet — the cache cannot serve the draft whole.
        assert!(cached_draft(&pool, row_id).await.unwrap().is_none());

        set_body(
            &pool,
            row_id,
            Some("hi there"),
            None,
            "hi there",
            &[],
            &[],
            None,
        )
        .await
        .unwrap();

        let cached = cached_draft(&pool, row_id).await.unwrap().unwrap();
        assert_eq!(cached.to, "Alice <alice@example.com>");
        assert_eq!(cached.subject, "Re: plans");
        assert_eq!(cached.body, "hi there");
        assert_eq!(cached.from_addr, "Me <me@example.com>");
        assert_eq!(cached.message_id, "d@x");
        assert_eq!(cached.in_reply_to, "root@x");
        assert_eq!(cached.references, "root@x");
    }

    #[tokio::test]
    async fn cached_draft_refuses_drafts_with_attachments() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(&pool, id, "Drafts", &[threaded(1, "d@x", "", &[])])
            .await
            .unwrap();
        let row_id = list(&pool, Some(id), "Drafts", None).await.unwrap()[0].id;
        let file = AttachmentMeta {
            part_index: 1,
            filename: "cv.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            size: 100,
        };
        set_body(
            &pool,
            row_id,
            Some("see cv"),
            None,
            "see cv",
            &[],
            &[file],
            None,
        )
        .await
        .unwrap();

        // Attachment bytes are never cached — the server path owns this one.
        assert!(cached_draft(&pool, row_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn thread_of_includes_reply_drafts_flagged_as_such() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        upsert_headers(&pool, id, "INBOX", &[threaded(1, "a@x", "", &[])])
            .await
            .unwrap();
        // An unsent reply saved to the server's Drafts folder — it carries
        // In-Reply-To, so it threads like any reply.
        upsert_headers(&pool, id, "Drafts", &[threaded(2, "d@x", "a@x", &["a@x"])])
            .await
            .unwrap();
        let anchor = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        let thread = thread_of(&pool, anchor).await.unwrap();

        let shown: Vec<(&str, bool)> = thread
            .iter()
            .map(|m| (m.message_id.as_str(), m.is_draft))
            .collect();
        assert_eq!(shown, vec![("a@x", false), ("d@x", true)]);
    }

    #[tokio::test]
    async fn thread_row_flags_a_conversation_with_a_draft() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[threaded(1, "a@x", "", &[]), threaded(3, "solo@x", "", &[])],
        )
        .await
        .unwrap();
        upsert_headers(&pool, id, "Drafts", &[threaded(2, "d@x", "a@x", &["a@x"])])
            .await
            .unwrap();

        let rows = list_threaded(&pool, Some(id), "INBOX", None).await.unwrap();

        // Newest first: solo (no draft), then the thread with the reply draft.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].message_id, "solo@x");
        assert!(!rows[0].thread_has_draft);
        assert_eq!(rows[1].message_id, "a@x");
        assert!(rows[1].thread_has_draft);
    }

    #[tokio::test]
    async fn thread_counts_leave_drafts_out() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        upsert_headers(&pool, id, "INBOX", &[threaded(1, "a@x", "", &[])])
            .await
            .unwrap();
        upsert_headers(&pool, id, "Drafts", &[threaded(2, "d@x", "a@x", &["a@x"])])
            .await
            .unwrap();

        let rows = list_threaded(&pool, Some(id), "INBOX", None).await.unwrap();

        // The draft rides along in the conversation view, but it is not a
        // message of the exchange yet — the list's count ignores it.
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].thread_count, 1);
    }

    #[tokio::test]
    async fn thread_rows_in_mailbox_covers_only_the_anchors_folder() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(2, "b@x", "a@x", &["a@x"]),
                threaded(3, "other@x", "", &[]),
            ],
        )
        .await
        .unwrap();
        upsert_headers(&pool, id, "Sent", &[threaded(1, "c@x", "a@x", &["a@x"])])
            .await
            .unwrap();
        let anchor = list(&pool, Some(id), "INBOX", None)
            .await
            .unwrap()
            .into_iter()
            .find(|m| m.message_id == "b@x")
            .unwrap()
            .id;

        let rows = thread_rows_in_mailbox(&pool, anchor).await.unwrap();

        // Both inbox members, not the Sent sibling, not the unrelated mail.
        let uids: Vec<i64> = rows.iter().map(|(_, uid)| *uid).collect();
        assert_eq!(uids, vec![1, 2]);
    }

    #[tokio::test]
    async fn thread_rows_for_a_keyless_message_is_just_itself() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[threaded(1, "", "", &[]), threaded(2, "", "", &[])],
        )
        .await
        .unwrap();
        let anchor = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        let rows = thread_rows_in_mailbox(&pool, anchor).await.unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, anchor);
    }

    #[tokio::test]
    async fn thread_of_shows_a_server_side_copy_only_once() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        use crate::storage::mailboxes::DiscoveredMailbox;
        crate::storage::mailboxes::replace(
            &pool,
            id,
            &[
                DiscoveredMailbox {
                    name: "INBOX".to_string(),
                    role: Some("inbox".to_string()),
                },
                DiscoveredMailbox {
                    name: "[Gmail]/All Mail".to_string(),
                    role: Some("all".to_string()),
                },
            ],
        )
        .await
        .unwrap();
        // Gmail lists every message twice: once in its folder, once in
        // All Mail. Same Message-ID = same RFC message.
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(2, "b@x", "a@x", &["a@x"]),
            ],
        )
        .await
        .unwrap();
        upsert_headers(
            &pool,
            id,
            "[Gmail]/All Mail",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(2, "b@x", "a@x", &["a@x"]),
            ],
        )
        .await
        .unwrap();
        let anchor = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        let thread = thread_of(&pool, anchor).await.unwrap();

        // Two messages, not four — and the INBOX copies are the ones shown.
        let shown: Vec<(&str, &str)> = thread
            .iter()
            .map(|m| (m.message_id.as_str(), m.mailbox.as_str()))
            .collect();
        assert_eq!(shown, vec![("a@x", "INBOX"), ("b@x", "INBOX")]);
    }

    #[tokio::test]
    async fn thread_row_surfaces_attachments_of_any_member() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        // The OLDER message carries the file; the newest (the row's
        // representative) does not — the paperclip must still show.
        let mut with_file = threaded(1, "a@x", "", &[]);
        with_file.has_attachments = true;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[with_file, threaded(2, "b@x", "a@x", &["a@x"])],
        )
        .await
        .unwrap();

        let rows = list_threaded(&pool, Some(id), "INBOX", None).await.unwrap();

        assert_eq!(rows.len(), 1);
        assert!(rows[0].has_attachments);
        // The conversation view reports per message, not per thread.
        let thread = thread_of(&pool, rows[0].id).await.unwrap();
        assert!(thread[0].has_attachments);
        assert!(!thread[1].has_attachments);
    }

    #[tokio::test]
    async fn thread_counts_ignore_server_side_copies() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        use crate::storage::mailboxes::DiscoveredMailbox;
        crate::storage::mailboxes::replace(
            &pool,
            id,
            &[
                DiscoveredMailbox {
                    name: "INBOX".to_string(),
                    role: Some("inbox".to_string()),
                },
                DiscoveredMailbox {
                    name: "[Gmail]/All Mail".to_string(),
                    role: Some("all".to_string()),
                },
            ],
        )
        .await
        .unwrap();
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(2, "b@x", "a@x", &["a@x"]),
            ],
        )
        .await
        .unwrap();
        upsert_headers(
            &pool,
            id,
            "[Gmail]/All Mail",
            &[threaded(1, "a@x", "", &[])],
        )
        .await
        .unwrap();

        let rows = list_threaded(&pool, Some(id), "INBOX", None).await.unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].thread_count, 2);
    }

    #[tokio::test]
    async fn thread_of_for_a_keyless_message_is_just_itself() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        seed_roles(&pool, id).await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[threaded(1, "", "", &[]), threaded(2, "", "", &[])],
        )
        .await
        .unwrap();
        let anchor = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        let thread = thread_of(&pool, anchor).await.unwrap();

        assert_eq!(thread.len(), 1);
        assert_eq!(thread[0].id, anchor);
    }

    #[tokio::test]
    async fn messages_without_ids_get_no_thread_key() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[threaded(1, "", "", &[]), threaded(2, "", "", &[])],
        )
        .await
        .unwrap();

        assert_eq!(thread_keys(&pool, id).await, vec![None, None]);
    }

    #[tokio::test]
    async fn threads_do_not_cross_accounts() {
        let pool = test_pool().await;
        let first = account(&pool, "Personal").await;
        let second = account(&pool, "Work").await;
        // In account one, message a@x already hangs under thread root1@x.
        upsert_headers(
            &pool,
            first,
            "INBOX",
            &[threaded(1, "a@x", "root1@x", &["root1@x"])],
        )
        .await
        .unwrap();
        // Account two's reply references a@x — matching across accounts
        // would wrongly adopt root1@x instead of starting at a@x.
        upsert_headers(
            &pool,
            second,
            "INBOX",
            &[threaded(1, "b@x", "a@x", &["a@x"])],
        )
        .await
        .unwrap();

        assert_eq!(
            thread_keys(&pool, second).await,
            vec![Some("a@x".to_string())]
        );
    }

    #[tokio::test]
    async fn upsert_persists_recipients() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "Hello", "2026-07-08T00:00:00Z", false)],
        )
        .await
        .unwrap();

        let (to, cc, reply_to): (String, String, String) =
            sqlx::query_as("SELECT to_addr, cc_addr, reply_to_addr FROM messages WHERE uid = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(to, "Bob <bob@example.com>");
        assert_eq!(cc, "Cara <cara@example.com>");
        assert_eq!(reply_to, "Alice Reply <reply@example.com>");
    }

    #[tokio::test]
    async fn unified_inbox_lists_all_accounts_newest_first() {
        let pool = test_pool().await;
        let first = account(&pool, "Personal").await;
        let second = account(&pool, "Work").await;
        upsert_headers(
            &pool,
            first,
            "INBOX",
            &[header(1, "Old", "2026-07-01T00:00:00Z", true)],
        )
        .await
        .unwrap();
        upsert_headers(
            &pool,
            second,
            "INBOX",
            &[header(1, "New", "2026-07-08T00:00:00Z", false)],
        )
        .await
        .unwrap();

        let all = list(&pool, None, "INBOX", None).await.unwrap();

        let subjects: Vec<&str> = all.iter().map(|m| m.subject.as_str()).collect();
        assert_eq!(subjects, vec!["New", "Old"]);
        assert_eq!(all[0].mailbox, "INBOX");
        assert_eq!(all[0].from, "Alice <alice@example.com>");
        assert_eq!(all[0].to, "Bob <bob@example.com>");
        assert_eq!(all[0].cc, "Cara <cara@example.com>");
    }

    #[tokio::test]
    async fn account_filter_returns_only_that_accounts_messages() {
        let pool = test_pool().await;
        let first = account(&pool, "Personal").await;
        let second = account(&pool, "Work").await;
        upsert_headers(
            &pool,
            first,
            "INBOX",
            &[header(1, "Mine", "2026-07-01T00:00:00Z", true)],
        )
        .await
        .unwrap();
        upsert_headers(
            &pool,
            second,
            "INBOX",
            &[header(1, "Other", "2026-07-08T00:00:00Z", false)],
        )
        .await
        .unwrap();

        let mine = list(&pool, Some(first), "INBOX", None).await.unwrap();

        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].subject, "Mine");
        assert!(list(&pool, Some(999), "INBOX", None)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn upsert_updates_read_flag_without_duplicating() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(5, "Hello", "2026-07-08T00:00:00Z", false)],
        )
        .await
        .unwrap();

        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(5, "Hello", "2026-07-08T00:00:00Z", true)],
        )
        .await
        .unwrap();

        let all = list(&pool, Some(id), "INBOX", None).await.unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].read);
    }

    #[tokio::test]
    async fn max_uid_and_validity_reflect_the_cache() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;

        assert_eq!(max_uid(&pool, id, "INBOX").await.unwrap(), None);
        assert_eq!(stored_uid_validity(&pool, id, "INBOX").await.unwrap(), None);

        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                header(3, "A", "2026-07-01T00:00:00Z", false),
                header(9, "B", "2026-07-02T00:00:00Z", false),
            ],
        )
        .await
        .unwrap();

        assert_eq!(max_uid(&pool, id, "INBOX").await.unwrap(), Some(9));
        assert_eq!(
            stored_uid_validity(&pool, id, "INBOX").await.unwrap(),
            Some(7)
        );
    }

    #[tokio::test]
    async fn list_honors_the_row_limit() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                header(1, "Oldest", "2026-07-01T00:00:00Z", false),
                header(2, "Middle", "2026-07-02T00:00:00Z", false),
                header(3, "Newest", "2026-07-03T00:00:00Z", false),
            ],
        )
        .await
        .unwrap();

        let limited = list(&pool, Some(id), "INBOX", Some(2)).await.unwrap();

        let subjects: Vec<String> = limited.into_iter().map(|m| m.subject).collect();
        assert_eq!(subjects, vec!["Newest", "Middle"]);
    }

    #[tokio::test]
    async fn list_threaded_honors_the_row_limit() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[threaded(1, "old@x", "", &[]), threaded(2, "new@x", "", &[])],
        )
        .await
        .unwrap();

        let limited = list_threaded(&pool, Some(id), "INBOX", Some(1))
            .await
            .unwrap();

        // The newest conversation wins the single row (threaded() dates
        // follow uid order).
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].message_id, "new@x");
    }

    #[tokio::test]
    async fn view_status_counts_the_flat_view() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                header(1, "A", "2026-07-01T00:00:00Z", true),
                header(2, "B", "2026-07-02T00:00:00Z", false),
                header(3, "C", "2026-07-03T00:00:00Z", false),
            ],
        )
        .await
        .unwrap();
        // Another folder must not leak into the view's counts.
        upsert_headers(
            &pool,
            id,
            "Archive",
            &[header(9, "Z", "2026-07-04T00:00:00Z", false)],
        )
        .await
        .unwrap();

        let status = view_status(&pool, Some(id), "INBOX", false).await.unwrap();

        assert_eq!(
            status,
            crate::models::ViewStatus {
                // Header totals stay scoped to the viewed folder…
                list_rows: 3,
                unread: 2,
                // …while sync progress spans the whole account, so the
                // strip shows in every view while any folder still syncs.
                cached: 4,
                // No mailboxes row yet — the server total is unknown.
                server_total: None,
            }
        );
    }

    #[tokio::test]
    async fn view_status_progress_spans_all_folders_of_the_account() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        crate::storage::mailboxes::replace(
            &pool,
            id,
            &[
                crate::storage::mailboxes::DiscoveredMailbox {
                    name: "INBOX".to_string(),
                    role: Some("inbox".to_string()),
                },
                crate::storage::mailboxes::DiscoveredMailbox {
                    name: "Archive".to_string(),
                    role: Some("archive".to_string()),
                },
            ],
        )
        .await
        .unwrap();
        crate::storage::mailboxes::set_server_exists(&pool, id, "INBOX", 2)
            .await
            .unwrap();
        crate::storage::mailboxes::set_server_exists(&pool, id, "Archive", 5000)
            .await
            .unwrap();
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                header(1, "A", "2026-07-01T00:00:00Z", true),
                header(2, "B", "2026-07-02T00:00:00Z", true),
            ],
        )
        .await
        .unwrap();
        upsert_headers(
            &pool,
            id,
            "Archive",
            &[header(1, "Old", "2026-07-03T00:00:00Z", true)],
        )
        .await
        .unwrap();

        // Viewing the fully-mirrored INBOX still reports the account-wide
        // backfill (Archive is far from done).
        let status = view_status(&pool, Some(id), "INBOX", false).await.unwrap();

        assert_eq!(status.list_rows, 2);
        assert_eq!(status.cached, 3);
        assert_eq!(status.server_total, Some(5002));
    }

    #[tokio::test]
    async fn view_status_reports_the_servers_total_once_synced() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        crate::storage::mailboxes::replace(
            &pool,
            id,
            &[crate::storage::mailboxes::DiscoveredMailbox {
                name: "INBOX".to_string(),
                role: Some("inbox".to_string()),
            }],
        )
        .await
        .unwrap();
        crate::storage::mailboxes::set_server_exists(&pool, id, "INBOX", 9999)
            .await
            .unwrap();
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "A", "2026-07-01T00:00:00Z", true)],
        )
        .await
        .unwrap();

        let status = view_status(&pool, Some(id), "INBOX", false).await.unwrap();

        assert_eq!(status.cached, 1);
        assert_eq!(status.server_total, Some(9999));
    }

    #[tokio::test]
    async fn view_status_counts_each_conversation_once_when_threaded() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                threaded(1, "a@x", "", &[]),
                threaded(2, "b@x", "a@x", &["a@x"]),
                threaded(3, "solo@x", "", &[]),
            ],
        )
        .await
        .unwrap();

        let threaded_status = view_status(&pool, Some(id), "INBOX", true).await.unwrap();
        assert_eq!(threaded_status.list_rows, 2);
        assert_eq!(threaded_status.cached, 3);

        let flat_status = view_status(&pool, Some(id), "INBOX", false).await.unwrap();
        assert_eq!(flat_status.list_rows, 3);
    }

    #[tokio::test]
    async fn view_status_spans_all_accounts_in_the_unified_view() {
        let pool = test_pool().await;
        let first = account(&pool, "Personal").await;
        let second = account(&pool, "Work").await;
        for id in [first, second] {
            crate::storage::mailboxes::replace(
                &pool,
                id,
                &[crate::storage::mailboxes::DiscoveredMailbox {
                    name: "INBOX".to_string(),
                    role: Some("inbox".to_string()),
                }],
            )
            .await
            .unwrap();
            upsert_headers(
                &pool,
                id,
                "INBOX",
                &[header(1, "Hi", "2026-07-01T00:00:00Z", false)],
            )
            .await
            .unwrap();
        }
        crate::storage::mailboxes::set_server_exists(&pool, first, "INBOX", 10)
            .await
            .unwrap();
        crate::storage::mailboxes::set_server_exists(&pool, second, "INBOX", 5)
            .await
            .unwrap();

        let status = view_status(&pool, None, "INBOX", false).await.unwrap();

        assert_eq!(status.cached, 2);
        assert_eq!(status.unread, 2);
        assert_eq!(status.server_total, Some(15));
    }

    #[tokio::test]
    async fn min_uid_reflects_the_cache() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;

        assert_eq!(min_uid(&pool, id, "INBOX").await.unwrap(), None);

        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                header(3, "A", "2026-07-01T00:00:00Z", false),
                header(9, "B", "2026-07-02T00:00:00Z", false),
            ],
        )
        .await
        .unwrap();

        assert_eq!(min_uid(&pool, id, "INBOX").await.unwrap(), Some(3));
        // Other folders don't leak into the minimum.
        assert_eq!(min_uid(&pool, id, "Archive").await.unwrap(), None);
    }

    #[tokio::test]
    async fn list_shows_only_the_requested_mailbox() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "In inbox", "2026-07-01T00:00:00Z", false)],
        )
        .await
        .unwrap();
        upsert_headers(
            &pool,
            id,
            "Archive",
            &[header(1, "Archived", "2026-07-02T00:00:00Z", false)],
        )
        .await
        .unwrap();

        let inbox: Vec<String> = list(&pool, Some(id), "INBOX", None)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.subject)
            .collect();
        assert_eq!(inbox, vec!["In inbox"]);

        let unified: Vec<String> = list(&pool, None, "Archive", None)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.subject)
            .collect();
        assert_eq!(unified, vec!["Archived"]);
    }

    #[tokio::test]
    async fn set_body_with_nothing_parseable_still_counts_as_cached() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "Weird", "2026-07-01T00:00:00Z", false)],
        )
        .await
        .unwrap();
        let row_id = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        // e.g. an attachment-only message: the parser yields no text or html.
        set_body(&pool, row_id, None, None, "", &[], &[], None)
            .await
            .unwrap();

        // Without this, the prefetcher would re-download it on every sync.
        assert!(!has_missing_bodies(&pool, id).await.unwrap());
        assert_eq!(
            uids_missing_body(&pool, id, "INBOX", 10).await.unwrap(),
            Vec::<(i64, i64)>::new()
        );
    }

    #[tokio::test]
    async fn has_missing_bodies_spans_all_mailboxes() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        assert!(!has_missing_bodies(&pool, id).await.unwrap());

        upsert_headers(
            &pool,
            id,
            "Archive",
            &[header(1, "Old", "2026-07-01T00:00:00Z", false)],
        )
        .await
        .unwrap();
        assert!(has_missing_bodies(&pool, id).await.unwrap());

        let row_id = list(&pool, Some(id), "Archive", None).await.unwrap()[0].id;
        set_body(&pool, row_id, Some("text"), None, "text", &[], &[], None)
            .await
            .unwrap();
        assert!(!has_missing_bodies(&pool, id).await.unwrap());
    }

    #[tokio::test]
    async fn missing_body_lists_newest_first_and_skips_cached() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                header(1, "Old", "2026-07-01T00:00:00Z", false),
                header(2, "Cached", "2026-07-02T00:00:00Z", false),
                header(3, "New", "2026-07-03T00:00:00Z", false),
            ],
        )
        .await
        .unwrap();
        let cached_id = list(&pool, Some(id), "INBOX", None)
            .await
            .unwrap()
            .iter()
            .find(|m| m.subject == "Cached")
            .unwrap()
            .id;
        set_body(&pool, cached_id, Some("text"), None, "text", &[], &[], None)
            .await
            .unwrap();

        let missing = uids_missing_body(&pool, id, "INBOX", 10).await.unwrap();
        let uids: Vec<i64> = missing.iter().map(|(_, uid)| *uid).collect();
        assert_eq!(uids, vec![3, 1]);

        let limited = uids_missing_body(&pool, id, "INBOX", 1).await.unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].1, 3);
    }

    #[tokio::test]
    async fn body_roundtrips_and_updates_the_snippet() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "Hello", "2026-07-08T00:00:00Z", false)],
        )
        .await
        .unwrap();
        let message_id = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        let before = get_body(&pool, message_id).await.unwrap();
        assert_eq!(before.body_text, None);
        assert_eq!(before.body_html, None);
        assert_eq!(before.mailbox, "INBOX");
        assert_eq!(before.uid, 1);

        set_body(
            &pool,
            message_id,
            Some("plain body"),
            Some("<p>html body</p>"),
            "plain body",
            &[],
            &[],
            None,
        )
        .await
        .unwrap();

        let after = get_body(&pool, message_id).await.unwrap();
        assert_eq!(after.body_text.as_deref(), Some("plain body"));
        assert_eq!(after.body_html.as_deref(), Some("<p>html body</p>"));
        // No auth verdicts came with this body — the cache says "unknown".
        assert_eq!(after.auth_results, None);
        let headers = list(&pool, Some(id), "INBOX", None).await.unwrap();
        assert_eq!(headers[0].snippet, "plain body");
    }

    #[tokio::test]
    async fn auth_verdicts_ride_the_body_cache_as_json() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "Hello", "2026-07-08T00:00:00Z", false)],
        )
        .await
        .unwrap();
        let message_id = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        let auth = AuthResults {
            spf: Some("pass".to_string()),
            dkim: None,
            dmarc: Some("fail".to_string()),
        };
        set_body(
            &pool,
            message_id,
            Some("t"),
            None,
            "t",
            &[],
            &[],
            Some(&auth),
        )
        .await
        .unwrap();

        let row = get_body(&pool, message_id).await.unwrap();
        let cached: AuthResults = serde_json::from_str(&row.auth_results.unwrap()).unwrap();
        assert_eq!(cached, auth);
    }

    #[tokio::test]
    async fn set_body_replaces_inline_images_and_delete_cascades() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "Pics", "2026-07-08T00:00:00Z", false)],
        )
        .await
        .unwrap();
        let message_id = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        let photo = InlineImage {
            content_id: "photo1".to_string(),
            content_type: "image/png".to_string(),
            data: b"\x89PNG".to_vec(),
        };
        set_body(
            &pool,
            message_id,
            None,
            Some("<img>"),
            "",
            &[photo],
            &[],
            None,
        )
        .await
        .unwrap();

        let stored = images(&pool, message_id).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].content_id, "photo1");
        assert_eq!(stored[0].content_type, "image/png");
        assert_eq!(stored[0].data, b"\x89PNG");

        // A re-fetched body replaces its images instead of stacking them.
        set_body(
            &pool,
            message_id,
            None,
            Some("<p>plain</p>"),
            "",
            &[],
            &[],
            None,
        )
        .await
        .unwrap();
        assert_eq!(images(&pool, message_id).await.unwrap(), Vec::new());

        // And deleting the message must not strand image blobs.
        set_body(
            &pool,
            message_id,
            None,
            Some("<img>"),
            "",
            &[InlineImage {
                content_id: "photo2".to_string(),
                content_type: "image/jpeg".to_string(),
                data: b"JJ".to_vec(),
            }],
            &[],
            None,
        )
        .await
        .unwrap();
        delete_by_id(&pool, message_id).await.unwrap();
        let orphans: i64 = sqlx::query_scalar("SELECT count(*) FROM message_images")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(orphans, 0);
    }

    #[tokio::test]
    async fn set_body_stores_attachment_metadata_and_marks_the_scan() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "Files", "2026-07-08T00:00:00Z", false)],
        )
        .await
        .unwrap();
        let message_id = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;

        // Cached before any body arrived: nothing scanned, nothing listed.
        assert!(
            !get_body(&pool, message_id)
                .await
                .unwrap()
                .attachments_scanned
        );
        assert_eq!(attachments(&pool, message_id).await.unwrap(), Vec::new());

        let pdf = AttachmentMeta {
            part_index: 2,
            filename: "report.pdf".to_string(),
            content_type: "application/pdf".to_string(),
            size: 1234,
        };
        set_body(
            &pool,
            message_id,
            Some("see file"),
            None,
            "see file",
            &[],
            &[pdf],
            None,
        )
        .await
        .unwrap();

        assert!(
            get_body(&pool, message_id)
                .await
                .unwrap()
                .attachments_scanned
        );
        let stored = attachments(&pool, message_id).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].message_id, message_id);
        assert_eq!(stored[0].part_index, 2);
        assert_eq!(stored[0].filename, "report.pdf");
        assert_eq!(stored[0].content_type, "application/pdf");
        assert_eq!(stored[0].size, 1234);

        // The row is addressable by id (what the save commands look up)…
        let by_id = attachment(&pool, stored[0].id).await.unwrap();
        assert_eq!(by_id.filename, "report.pdf");
        assert!(attachment(&pool, 9999).await.is_err());

        // …a re-fetched body replaces the list instead of stacking it…
        set_body(
            &pool,
            message_id,
            Some("see file"),
            None,
            "see file",
            &[],
            &[],
            None,
        )
        .await
        .unwrap();
        assert_eq!(attachments(&pool, message_id).await.unwrap(), Vec::new());

        // …and a body with none still counts as scanned (no rescan loop).
        assert!(
            get_body(&pool, message_id)
                .await
                .unwrap()
                .attachments_scanned
        );
    }

    #[tokio::test]
    async fn deleting_a_message_cascades_its_attachments() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "Files", "2026-07-08T00:00:00Z", false)],
        )
        .await
        .unwrap();
        let message_id = list(&pool, Some(id), "INBOX", None).await.unwrap()[0].id;
        let meta = AttachmentMeta {
            part_index: 0,
            filename: "a.zip".to_string(),
            content_type: "application/zip".to_string(),
            size: 10,
        };
        set_body(&pool, message_id, None, None, "", &[], &[meta], None)
            .await
            .unwrap();

        delete_by_id(&pool, message_id).await.unwrap();

        let orphans: i64 = sqlx::query_scalar("SELECT count(*) FROM message_attachments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(orphans, 0);
    }

    #[tokio::test]
    async fn uid_flags_delete_and_set_read_roundtrip() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                header(1, "First", "2026-07-01T00:00:00Z", false),
                header(2, "Second", "2026-07-02T00:00:00Z", true),
            ],
        )
        .await
        .unwrap();

        let rows = uid_flags(&pool, id, "INBOX").await.unwrap();
        let uids: Vec<i64> = rows.iter().map(|(_, uid, _)| *uid).collect();
        assert_eq!(uids, vec![1, 2]);

        let (first_id, _, first_read) = rows[0];
        assert!(!first_read);
        set_read(&pool, first_id, true).await.unwrap();
        assert!(uid_flags(&pool, id, "INBOX").await.unwrap()[0].2);

        delete_by_id(&pool, first_id).await.unwrap();
        let remaining = uid_flags(&pool, id, "INBOX").await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].1, 2);
    }

    #[tokio::test]
    async fn location_returns_server_coordinates() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "Archive",
            &[header(42, "Filed", "2026-07-01T00:00:00Z", false)],
        )
        .await
        .unwrap();
        let message_id = list(&pool, Some(id), "Archive", None).await.unwrap()[0].id;

        let loc = location(&pool, message_id).await.unwrap();
        assert_eq!(loc.account_id, id);
        assert_eq!(loc.mailbox, "Archive");
        assert_eq!(loc.uid, 42);

        assert!(location(&pool, 9999).await.is_err());
    }

    #[tokio::test]
    async fn backfill_rewrites_only_url_snippets_and_is_idempotent() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                header(1, "Freelo", "2026-07-01T00:00:00Z", false),
                header(2, "Clean", "2026-07-02T00:00:00Z", false),
                header(3, "NoBody", "2026-07-03T00:00:00Z", false),
            ],
        )
        .await
        .unwrap();
        let rows = list(&pool, Some(id), "INBOX", None).await.unwrap();
        let stale = rows.iter().find(|m| m.subject == "Freelo").unwrap().id;
        let clean = rows.iter().find(|m| m.subject == "Clean").unwrap().id;

        // Simulate a body cached by the OLD build: snippet still holds the URL.
        let body = "<https://app.freelo.io/dashboard/?utm_source=x> Assigned you to a task";
        sqlx::query("UPDATE messages SET body_text = ?, snippet = ? WHERE id = ?")
            .bind(body)
            .bind(body) // old snippet == raw body prefix, URL and all
            .bind(stale)
            .execute(&pool)
            .await
            .unwrap();
        // A row whose snippet has no URL must be left untouched.
        set_body(
            &pool,
            clean,
            Some("Just prose here"),
            None,
            "Just prose here",
            &[],
            &[],
            None,
        )
        .await
        .unwrap();

        let fixed = backfill_url_snippets(&pool).await.unwrap();
        assert_eq!(fixed, 1);

        let after = list(&pool, Some(id), "INBOX", None).await.unwrap();
        let snip = |subject: &str| {
            after
                .iter()
                .find(|m| m.subject == subject)
                .unwrap()
                .snippet
                .clone()
        };
        assert_eq!(snip("Freelo"), "Assigned you to a task");
        assert_eq!(snip("Clean"), "Just prose here");
        // The body-less row was never a candidate (body_text IS NULL).
        assert_eq!(snip("NoBody"), "snippet of NoBody");

        // Second run finds nothing left to fix.
        assert_eq!(backfill_url_snippets(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn backfill_recomputes_snippets_carrying_entity_padding() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[
                header(1, "Bistro", "2026-07-01T00:00:00Z", false),
                header(2, "Clean", "2026-07-02T00:00:00Z", false),
            ],
        )
        .await
        .unwrap();
        let rows = list(&pool, Some(id), "INBOX", None).await.unwrap();
        let stale = rows.iter().find(|m| m.subject == "Bistro").unwrap().id;
        let clean = rows.iter().find(|m| m.subject == "Clean").unwrap().id;

        // Simulate a body cached by the OLD build: snippet kept the literal
        // "&zwnj;" padding the parser now strips.
        let body = "Bistro.sk\n&zwnj; &zwnj; &zwnj;\nVeľké finále je tu";
        sqlx::query("UPDATE messages SET body_text = ?, snippet = ? WHERE id = ?")
            .bind(body)
            .bind("Bistro.sk &zwnj; &zwnj; &zwnj; Veľké finále je tu")
            .bind(stale)
            .execute(&pool)
            .await
            .unwrap();
        // A row without padding must be left untouched.
        set_body(
            &pool,
            clean,
            Some("Just prose here"),
            None,
            "Just prose here",
            &[],
            &[],
            None,
        )
        .await
        .unwrap();

        let fixed = backfill_entity_snippets(&pool).await.unwrap();
        assert_eq!(fixed, 1);

        let after = list(&pool, Some(id), "INBOX", None).await.unwrap();
        let snip = |subject: &str| {
            after
                .iter()
                .find(|m| m.subject == subject)
                .unwrap()
                .snippet
                .clone()
        };
        assert_eq!(snip("Bistro"), "Bistro.sk Veľké finále je tu");
        assert_eq!(snip("Clean"), "Just prose here");

        // Second run finds nothing left to fix.
        assert_eq!(backfill_entity_snippets(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn clear_mailbox_removes_only_that_mailbox() {
        let pool = test_pool().await;
        let id = account(&pool, "Personal").await;
        upsert_headers(
            &pool,
            id,
            "INBOX",
            &[header(1, "Inbox", "2026-07-01T00:00:00Z", false)],
        )
        .await
        .unwrap();
        upsert_headers(
            &pool,
            id,
            "Archive",
            &[header(1, "Archived", "2026-07-01T00:00:00Z", false)],
        )
        .await
        .unwrap();

        clear_mailbox(&pool, id, "INBOX").await.unwrap();

        assert!(list(&pool, Some(id), "INBOX", None)
            .await
            .unwrap()
            .is_empty());
        let remaining = list(&pool, Some(id), "Archive", None).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].subject, "Archived");
    }
}
