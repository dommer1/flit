use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::Mailbox;

/// A folder as discovered on the server, before it has a row id.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredMailbox {
    /// Full IMAP name, e.g. "Archive" or "Work.Clients".
    pub name: String,
    /// Special-use role: "inbox" | "drafts" | "sent" | "archive" | "junk"
    /// | "trash"; `None` for ordinary folders.
    pub role: Option<String>,
}

/// Mirror the server's folder list for one account: folders the server no
/// longer lists are dropped, the rest are inserted or updated in place.
///
/// why not delete-all + insert: this runs at the top of every sync pass, and
/// the row carries per-folder sync state (how far the cache has got). Wiping
/// and re-inserting would reset that state sixty times an hour. Upserting on
/// the (account_id, name) unique key keeps surviving folders' rows — and
/// their state — intact, while still mirroring renames and role changes.
pub async fn replace(
    pool: &SqlitePool,
    account_id: i64,
    mailboxes: &[DiscoveredMailbox],
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;

    // Prune first: a folder gone from the server takes its row with it.
    //
    // why a built string: the placeholder count follows the folder count, so
    // it cannot be a literal. Only `?`s are formatted in — every value is
    // still bound, never interpolated.
    let prune_sql = if mailboxes.is_empty() {
        "DELETE FROM mailboxes WHERE account_id = ?".to_string()
    } else {
        let placeholders = vec!["?"; mailboxes.len()].join(",");
        format!("DELETE FROM mailboxes WHERE account_id = ? AND name NOT IN ({placeholders})")
    };
    let mut prune = sqlx::query(sqlx::AssertSqlSafe(prune_sql)).bind(account_id);
    for mailbox in mailboxes {
        prune = prune.bind(&mailbox.name);
    }
    prune.execute(&mut *tx).await?;

    for mailbox in mailboxes {
        sqlx::query(
            "INSERT INTO mailboxes (account_id, name, role) VALUES (?, ?, ?)
             ON CONFLICT (account_id, name) DO UPDATE SET role = excluded.role",
        )
        .bind(account_id)
        .bind(&mailbox.name)
        .bind(&mailbox.role)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Human-readable folder name: IMAP wire names carry non-ASCII characters
/// in modified UTF-7 (RFC 3501 §5.1.3), so "Odoslan&AOk-" decodes to
/// "Odoslané". Gmail's "[Gmail]/" container prefix is display noise and is
/// dropped too.
fn display_name(wire_name: &str) -> String {
    let decoded = utf7_imap::decode_utf7_imap(wire_name.to_string());
    decoded
        .strip_prefix("[Gmail]/")
        .unwrap_or(&decoded)
        .to_string()
}

/// Folders of one account in sidebar order: special-use roles first
/// (inbox, drafts, sent, archive, junk, trash), then customs alphabetically.
pub async fn list(pool: &SqlitePool, account_id: i64) -> Result<Vec<Mailbox>, AppError> {
    let rows: Vec<Mailbox> = sqlx::query_as(
        // why a correlated subquery: messages reference folders by name, not
        // by mailbox id, so a JOIN + GROUP BY buys nothing here.
        "SELECT id, account_id, name, role,
                (SELECT count(*) FROM messages m
                  WHERE m.account_id = mailboxes.account_id
                    AND m.mailbox = mailboxes.name
                    AND m.read = 0) AS unread_count
         FROM mailboxes
         WHERE account_id = ?
         ORDER BY CASE role
                    WHEN 'inbox' THEN 0
                    WHEN 'drafts' THEN 1
                    WHEN 'sent' THEN 2
                    WHEN 'archive' THEN 3
                    WHEN 'junk' THEN 4
                    WHEN 'trash' THEN 5
                    ELSE 6
                  END,
                  name COLLATE NOCASE",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|mut m| {
            m.display_name = display_name(&m.name);
            m
        })
        .collect())
}

/// Record how many messages the server reported in this folder (the EXISTS
/// count of a SELECT). Written on every sync pass — `replace` wipes folder
/// rows wholesale, so the value must be re-recorded each time anyway.
pub async fn set_server_exists(
    pool: &SqlitePool,
    account_id: i64,
    name: &str,
    exists: i64,
) -> Result<(), AppError> {
    sqlx::query("UPDATE mailboxes SET server_exists = ? WHERE account_id = ? AND name = ?")
        .bind(exists)
        .bind(account_id)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

/// When this folder last had a full flag reconciliation, as unix epoch
/// seconds; `None` when it never did (or the folder is unknown).
pub async fn last_full_sweep(
    pool: &SqlitePool,
    account_id: i64,
    name: &str,
) -> Result<Option<i64>, AppError> {
    let at: Option<Option<i64>> = sqlx::query_scalar(
        "SELECT last_full_sweep FROM mailboxes WHERE account_id = ? AND name = ?",
    )
    .bind(account_id)
    .bind(name)
    .fetch_optional(pool)
    .await?;
    Ok(at.flatten())
}

/// Record that this folder just had a full flag reconciliation.
pub async fn mark_full_sweep(
    pool: &SqlitePool,
    account_id: i64,
    name: &str,
    at: i64,
) -> Result<(), AppError> {
    sqlx::query("UPDATE mailboxes SET last_full_sweep = ? WHERE account_id = ? AND name = ?")
        .bind(at)
        .bind(account_id)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

/// Folders whose server count exceeds what the cache holds — the header
/// backfill's work list, in sidebar order so INBOX completes first. Folders
/// that never synced (server_exists NULL) are skipped: the regular sync owns
/// their first pass.
pub async fn incomplete_mailboxes(
    pool: &SqlitePool,
    account_id: i64,
) -> Result<Vec<String>, AppError> {
    let names = sqlx::query_scalar(
        "SELECT name FROM mailboxes b
         WHERE account_id = ?
           AND server_exists > (SELECT count(*) FROM messages m
                                 WHERE m.account_id = b.account_id
                                   AND m.mailbox = b.name)
         ORDER BY CASE role
                    WHEN 'inbox' THEN 0
                    WHEN 'drafts' THEN 1
                    WHEN 'sent' THEN 2
                    WHEN 'archive' THEN 3
                    WHEN 'junk' THEN 4
                    WHEN 'trash' THEN 5
                    ELSE 6
                  END,
                  name COLLATE NOCASE",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(names)
}

/// Full IMAP name of the account's folder for a special-use `role`
/// ("sent" | "trash" | "archive" | …), if discovery found one.
pub async fn name_for_role(
    pool: &SqlitePool,
    account_id: i64,
    role: &str,
) -> Result<Option<String>, AppError> {
    let name = sqlx::query_scalar("SELECT name FROM mailboxes WHERE account_id = ? AND role = ?")
        .bind(account_id)
        .bind(role)
        .fetch_optional(pool)
        .await?;
    Ok(name)
}

/// Full IMAP name of the account's Sent folder, if discovery found one.
pub async fn sent_name(pool: &SqlitePool, account_id: i64) -> Result<Option<String>, AppError> {
    name_for_role(pool, account_id, "sent").await
}

/// Special-use role of one folder; `None` for custom folders and for names
/// discovery never saw.
pub async fn role_of(
    pool: &SqlitePool,
    account_id: i64,
    name: &str,
) -> Result<Option<String>, AppError> {
    let role: Option<Option<String>> =
        sqlx::query_scalar("SELECT role FROM mailboxes WHERE account_id = ? AND name = ?")
            .bind(account_id)
            .bind(name)
            .fetch_optional(pool)
            .await?;
    Ok(role.flatten())
}

/// Whether `name` is a discovered folder of this account — the guard a
/// user-supplied move destination must pass before any IMAP command runs.
pub async fn exists(pool: &SqlitePool, account_id: i64, name: &str) -> Result<bool, AppError> {
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mailboxes WHERE account_id = ? AND name = ?")
            .bind(account_id)
            .bind(name)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    async fn account(pool: &SqlitePool) -> i64 {
        crate::storage::accounts::insert(
            pool,
            &crate::models::NewAccount {
                name: "Personal".to_string(),
                email: "a@example.com".to_string(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                username: "a@example.com".to_string(),
            },
        )
        .await
        .unwrap()
        .id
    }

    fn found(name: &str, role: Option<&str>) -> DiscoveredMailbox {
        DiscoveredMailbox {
            name: name.to_string(),
            role: role.map(str::to_string),
        }
    }

    async fn insert_message(
        pool: &SqlitePool,
        account_id: i64,
        mailbox: &str,
        uid: i64,
        read: bool,
    ) {
        crate::storage::messages::upsert_headers(
            pool,
            account_id,
            mailbox,
            &[crate::storage::messages::FetchedHeader {
                uid,
                uid_validity: 1,
                from: "s@example.com".to_string(),
                to: "a@example.com".to_string(),
                subject: "hi".to_string(),
                date: "2026-07-17T10:00:00Z".to_string(),
                read,
                ..Default::default()
            }],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn list_counts_unread_messages_per_mailbox() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[found("INBOX", Some("inbox")), found("Work", None)],
        )
        .await
        .unwrap();

        insert_message(&pool, id, "INBOX", 1, false).await;
        insert_message(&pool, id, "INBOX", 2, false).await;
        insert_message(&pool, id, "INBOX", 3, true).await;
        insert_message(&pool, id, "Work", 1, true).await;

        let folders = list(&pool, id).await.unwrap();
        let unread: Vec<(String, i64)> = folders
            .into_iter()
            .map(|m| (m.name, m.unread_count))
            .collect();
        assert_eq!(
            unread,
            vec![("INBOX".to_string(), 2), ("Work".to_string(), 0)]
        );
    }

    #[tokio::test]
    async fn list_counts_only_that_accounts_messages() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        let other = crate::storage::accounts::insert(
            &pool,
            &crate::models::NewAccount {
                name: "Work".to_string(),
                email: "b@example.com".to_string(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                username: "b@example.com".to_string(),
            },
        )
        .await
        .unwrap()
        .id;
        replace(&pool, id, &[found("INBOX", Some("inbox"))])
            .await
            .unwrap();
        // Same folder name on the other account must not leak into the count.
        insert_message(&pool, other, "INBOX", 1, false).await;

        let folders = list(&pool, id).await.unwrap();
        assert_eq!(folders[0].unread_count, 0);
    }

    #[tokio::test]
    async fn replace_mirrors_the_server_list() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[found("INBOX", Some("inbox")), found("Old", None)],
        )
        .await
        .unwrap();

        replace(
            &pool,
            id,
            &[found("INBOX", Some("inbox")), found("New", None)],
        )
        .await
        .unwrap();

        let names: Vec<String> = list(&pool, id)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(names, vec!["INBOX", "New"]);
    }

    #[tokio::test]
    async fn replace_keeps_per_folder_state_of_surviving_folders() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[found("INBOX", Some("inbox")), found("Old", None)],
        )
        .await
        .unwrap();
        set_server_exists(&pool, id, "INBOX", 9876).await.unwrap();

        replace(
            &pool,
            id,
            &[found("INBOX", Some("inbox")), found("New", None)],
        )
        .await
        .unwrap();

        // The folder survived the refresh, so its accumulated state must too —
        // sync progress is per folder and must not reset on every pass.
        assert_eq!(server_exists_of(&pool, id, "INBOX").await, Some(9876));
        assert_eq!(server_exists_of(&pool, id, "New").await, None);
    }

    #[tokio::test]
    async fn replace_adopts_a_changed_role() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(&pool, id, &[found("Archive", None)]).await.unwrap();

        replace(&pool, id, &[found("Archive", Some("archive"))])
            .await
            .unwrap();

        assert_eq!(
            name_for_role(&pool, id, "archive").await.unwrap(),
            Some("Archive".to_string())
        );
    }

    #[tokio::test]
    async fn list_orders_special_roles_before_custom_folders() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[
                found("Zzz", None),
                found("Trash", Some("trash")),
                found("Archive", Some("archive")),
                found("Alpha", None),
                found("Sent", Some("sent")),
                found("INBOX", Some("inbox")),
            ],
        )
        .await
        .unwrap();

        let names: Vec<String> = list(&pool, id)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(
            names,
            vec!["INBOX", "Sent", "Archive", "Trash", "Alpha", "Zzz"]
        );
    }

    #[test]
    fn display_name_decodes_modified_utf7() {
        assert_eq!(display_name("Odoslan&AOk-"), "Odoslané");
        assert_eq!(display_name("K&APQBYQ-"), "Kôš");
        assert_eq!(display_name("INBOX"), "INBOX");
    }

    #[test]
    fn display_name_strips_the_gmail_container_prefix() {
        assert_eq!(
            display_name("[Gmail]/V&AWE-etky spr&AOE-vy"),
            "Všetky správy"
        );
        assert_eq!(display_name("[Gmail]/Spam"), "Spam");
        // Only the prefix goes — a folder merely named like it stays intact.
        assert_eq!(display_name("Gmail stuff"), "Gmail stuff");
    }

    #[tokio::test]
    async fn list_fills_display_names() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[
                found("INBOX", Some("inbox")),
                found("[Gmail]/Odoslan&AOk-", Some("sent")),
            ],
        )
        .await
        .unwrap();

        let folders = list(&pool, id).await.unwrap();
        let sent = folders
            .iter()
            .find(|m| m.role.as_deref() == Some("sent"))
            .unwrap();
        assert_eq!(sent.name, "[Gmail]/Odoslan&AOk-");
        assert_eq!(sent.display_name, "Odoslané");
    }

    #[tokio::test]
    async fn sent_name_returns_the_sent_role_folder() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[
                found("INBOX", Some("inbox")),
                found("Odoslané", Some("sent")),
            ],
        )
        .await
        .unwrap();

        assert_eq!(
            sent_name(&pool, id).await.unwrap(),
            Some("Odoslané".to_string())
        );
    }

    #[tokio::test]
    async fn sent_name_is_none_when_no_sent_folder_was_discovered() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(&pool, id, &[found("INBOX", Some("inbox"))])
            .await
            .unwrap();

        assert_eq!(sent_name(&pool, id).await.unwrap(), None);
    }

    #[tokio::test]
    async fn name_for_role_returns_the_matching_folder_or_none() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(&pool, id, &[found("INBOX", Some("inbox"))])
            .await
            .unwrap();
        assert_eq!(name_for_role(&pool, id, "trash").await.unwrap(), None);

        replace(
            &pool,
            id,
            &[
                found("INBOX", Some("inbox")),
                found("Kôš", Some("trash")),
                found("Archív", Some("archive")),
            ],
        )
        .await
        .unwrap();
        assert_eq!(
            name_for_role(&pool, id, "trash").await.unwrap(),
            Some("Kôš".to_string())
        );
        assert_eq!(
            name_for_role(&pool, id, "archive").await.unwrap(),
            Some("Archív".to_string())
        );
        assert_eq!(name_for_role(&pool, id, "junk").await.unwrap(), None);
    }

    #[tokio::test]
    async fn exists_matches_only_that_accounts_folders() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[found("INBOX", Some("inbox")), found("Work", None)],
        )
        .await
        .unwrap();

        assert!(exists(&pool, id, "Work").await.unwrap());
        assert!(exists(&pool, id, "INBOX").await.unwrap());
        assert!(!exists(&pool, id, "Missing").await.unwrap());
        assert!(!exists(&pool, id + 1, "Work").await.unwrap());
    }

    async fn server_exists_of(pool: &SqlitePool, account_id: i64, name: &str) -> Option<i64> {
        sqlx::query_scalar("SELECT server_exists FROM mailboxes WHERE account_id = ? AND name = ?")
            .bind(account_id)
            .bind(name)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn set_server_exists_records_the_servers_count() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[found("INBOX", Some("inbox")), found("Work", None)],
        )
        .await
        .unwrap();
        assert_eq!(server_exists_of(&pool, id, "INBOX").await, None);

        set_server_exists(&pool, id, "INBOX", 9876).await.unwrap();

        assert_eq!(server_exists_of(&pool, id, "INBOX").await, Some(9876));
        // Only the selected folder is touched.
        assert_eq!(server_exists_of(&pool, id, "Work").await, None);
    }

    #[tokio::test]
    async fn full_sweep_marker_round_trips_and_survives_a_refresh() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[found("INBOX", Some("inbox")), found("Work", None)],
        )
        .await
        .unwrap();
        assert_eq!(last_full_sweep(&pool, id, "INBOX").await.unwrap(), None);

        mark_full_sweep(&pool, id, "INBOX", 1_753_600_000)
            .await
            .unwrap();

        assert_eq!(
            last_full_sweep(&pool, id, "INBOX").await.unwrap(),
            Some(1_753_600_000)
        );
        // Only the swept folder is touched.
        assert_eq!(last_full_sweep(&pool, id, "Work").await.unwrap(), None);

        // The marker is worthless if the next pass's folder refresh clears it.
        replace(
            &pool,
            id,
            &[found("INBOX", Some("inbox")), found("Work", None)],
        )
        .await
        .unwrap();
        assert_eq!(
            last_full_sweep(&pool, id, "INBOX").await.unwrap(),
            Some(1_753_600_000)
        );
    }

    #[tokio::test]
    async fn last_full_sweep_is_none_for_an_unknown_folder() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(&pool, id, &[found("INBOX", Some("inbox"))])
            .await
            .unwrap();

        assert_eq!(last_full_sweep(&pool, id, "Nope").await.unwrap(), None);
    }

    #[tokio::test]
    async fn incomplete_mailboxes_lists_folders_with_uncached_mail() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[
                found("Work", None),
                found("INBOX", Some("inbox")),
                found("Empty", None),
            ],
        )
        .await
        .unwrap();

        // INBOX: server holds 3, cache holds 1 → incomplete.
        set_server_exists(&pool, id, "INBOX", 3).await.unwrap();
        insert_message(&pool, id, "INBOX", 10, true).await;
        // Work: server holds 1, cache holds 1 → complete.
        set_server_exists(&pool, id, "Work", 1).await.unwrap();
        insert_message(&pool, id, "Work", 5, true).await;
        // Empty: never synced (server_exists NULL) → not listed; the
        // regular sync owns the first pass.

        assert_eq!(
            incomplete_mailboxes(&pool, id).await.unwrap(),
            vec!["INBOX".to_string()]
        );
    }

    #[tokio::test]
    async fn incomplete_mailboxes_orders_the_inbox_first() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(
            &pool,
            id,
            &[
                found("Archive", Some("archive")),
                found("INBOX", Some("inbox")),
            ],
        )
        .await
        .unwrap();
        set_server_exists(&pool, id, "Archive", 5).await.unwrap();
        set_server_exists(&pool, id, "INBOX", 5).await.unwrap();
        insert_message(&pool, id, "Archive", 1, true).await;
        insert_message(&pool, id, "INBOX", 1, true).await;

        assert_eq!(
            incomplete_mailboxes(&pool, id).await.unwrap(),
            vec!["INBOX".to_string(), "Archive".to_string()]
        );
    }

    #[tokio::test]
    async fn deleting_an_account_cascades_its_mailboxes() {
        let pool = test_pool().await;
        let id = account(&pool).await;
        replace(&pool, id, &[found("INBOX", Some("inbox"))])
            .await
            .unwrap();

        crate::storage::accounts::delete(&pool, id).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mailboxes")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
