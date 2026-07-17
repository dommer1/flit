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

/// Mirror the server's folder list for one account: wholesale replace.
///
/// why: folder lists are tiny and rarely change — delete + insert inside one
/// transaction is simpler than diffing, and nothing references mailbox rows
/// by id (messages carry the mailbox *name*).
pub async fn replace(
    pool: &SqlitePool,
    account_id: i64,
    mailboxes: &[DiscoveredMailbox],
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM mailboxes WHERE account_id = ?")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
    for mailbox in mailboxes {
        sqlx::query("INSERT INTO mailboxes (account_id, name, role) VALUES (?, ?, ?)")
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
