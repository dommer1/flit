use sqlx::SqlitePool;

use crate::error::AppError;
use crate::mail::parse::InlineImage;
use crate::models::MessageHeader;

/// Header data as it arrives from an IMAP fetch, before it has a row id.
#[derive(Debug, Clone)]
pub struct FetchedHeader {
    pub uid: i64,
    pub uid_validity: i64,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub read: bool,
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
        sqlx::query(
            "INSERT INTO messages
               (account_id, mailbox, uid, uid_validity, from_addr, to_addr, cc_addr, subject, date, snippet, read)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (account_id, mailbox, uid) DO UPDATE SET read = excluded.read",
        )
        .bind(account_id)
        .bind(mailbox)
        .bind(header.uid)
        .bind(header.uid_validity)
        .bind(&header.from)
        .bind(&header.to)
        .bind(&header.cc)
        .bind(&header.subject)
        .bind(&header.date)
        .bind(&header.snippet)
        .bind(header.read)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Headers of one mailbox for one account — or across all accounts when
/// `account_id` is `None` (the "All Inboxes" view), newest first.
pub async fn list(
    pool: &SqlitePool,
    account_id: Option<i64>,
    mailbox: &str,
) -> Result<Vec<MessageHeader>, AppError> {
    // why: sqlx 0.9 rejects runtime-built SQL strings (SqlSafeStr), so the
    // column list is spelled out twice instead of shared via format!().
    let headers = match account_id {
        Some(id) => {
            sqlx::query_as(
                r#"SELECT id, account_id, from_addr AS "from", subject, snippet, date, read
                   FROM messages WHERE account_id = ? AND mailbox = ? ORDER BY date DESC"#,
            )
            .bind(id)
            .bind(mailbox)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                r#"SELECT id, account_id, from_addr AS "from", subject, snippet, date, read
                   FROM messages WHERE mailbox = ? ORDER BY date DESC"#,
            )
            .bind(mailbox)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(headers)
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
}

pub async fn get_body(pool: &SqlitePool, message_id: i64) -> Result<BodyRow, AppError> {
    let row = sqlx::query_as(
        "SELECT account_id, mailbox, uid, body_text, body_html FROM messages WHERE id = ?",
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
pub async fn set_body(
    pool: &SqlitePool,
    message_id: i64,
    text: Option<&str>,
    html: Option<&str>,
    snippet: &str,
    images: &[InlineImage],
) -> Result<(), AppError> {
    sqlx::query("UPDATE messages SET body_text = ?, body_html = ?, snippet = ? WHERE id = ?")
        .bind(text.unwrap_or(""))
        .bind(html)
        .bind(snippet)
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
    Ok(())
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

/// Remove one cached message (it vanished from the folder server-side).
pub async fn delete_by_id(pool: &SqlitePool, message_id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM messages WHERE id = ?")
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
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
            subject: subject.to_string(),
            date: date.to_string(),
            snippet: format!("snippet of {subject}"),
            read,
        }
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

        let (to, cc): (String, String) =
            sqlx::query_as("SELECT to_addr, cc_addr FROM messages WHERE uid = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(to, "Bob <bob@example.com>");
        assert_eq!(cc, "Cara <cara@example.com>");
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

        let all = list(&pool, None, "INBOX").await.unwrap();

        let subjects: Vec<&str> = all.iter().map(|m| m.subject.as_str()).collect();
        assert_eq!(subjects, vec!["New", "Old"]);
        assert_eq!(all[0].from, "Alice <alice@example.com>");
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

        let mine = list(&pool, Some(first), "INBOX").await.unwrap();

        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].subject, "Mine");
        assert!(list(&pool, Some(999), "INBOX").await.unwrap().is_empty());
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

        let all = list(&pool, Some(id), "INBOX").await.unwrap();
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

        let inbox: Vec<String> = list(&pool, Some(id), "INBOX")
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.subject)
            .collect();
        assert_eq!(inbox, vec!["In inbox"]);

        let unified: Vec<String> = list(&pool, None, "Archive")
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
        let row_id = list(&pool, Some(id), "INBOX").await.unwrap()[0].id;

        // e.g. an attachment-only message: the parser yields no text or html.
        set_body(&pool, row_id, None, None, "", &[]).await.unwrap();

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

        let row_id = list(&pool, Some(id), "Archive").await.unwrap()[0].id;
        set_body(&pool, row_id, Some("text"), None, "text", &[])
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
        let cached_id = list(&pool, Some(id), "INBOX")
            .await
            .unwrap()
            .iter()
            .find(|m| m.subject == "Cached")
            .unwrap()
            .id;
        set_body(&pool, cached_id, Some("text"), None, "text", &[])
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
        let message_id = list(&pool, Some(id), "INBOX").await.unwrap()[0].id;

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
        )
        .await
        .unwrap();

        let after = get_body(&pool, message_id).await.unwrap();
        assert_eq!(after.body_text.as_deref(), Some("plain body"));
        assert_eq!(after.body_html.as_deref(), Some("<p>html body</p>"));
        let headers = list(&pool, Some(id), "INBOX").await.unwrap();
        assert_eq!(headers[0].snippet, "plain body");
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
        let message_id = list(&pool, Some(id), "INBOX").await.unwrap()[0].id;

        let photo = InlineImage {
            content_id: "photo1".to_string(),
            content_type: "image/png".to_string(),
            data: b"\x89PNG".to_vec(),
        };
        set_body(&pool, message_id, None, Some("<img>"), "", &[photo])
            .await
            .unwrap();

        let stored = images(&pool, message_id).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].content_id, "photo1");
        assert_eq!(stored[0].content_type, "image/png");
        assert_eq!(stored[0].data, b"\x89PNG");

        // A re-fetched body replaces its images instead of stacking them.
        set_body(&pool, message_id, None, Some("<p>plain</p>"), "", &[])
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

        assert!(list(&pool, Some(id), "INBOX").await.unwrap().is_empty());
        let remaining = list(&pool, Some(id), "Archive").await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].subject, "Archived");
    }
}
