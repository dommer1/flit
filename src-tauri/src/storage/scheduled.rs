//! "Send Later" storage. The invariant everything here protects: a scheduled
//! message is sent at most once. Every path that ends in a delivery goes
//! through a DELETE ... RETURNING — whoever gets the row back owns it, so the
//! scheduler tick, "send now" and "cancel" can never race into a double send.

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{OutgoingMessage, ScheduledMessage};

/// Park a composed message until `scheduled_at` (unix seconds, UTC).
pub async fn insert(
    pool: &SqlitePool,
    message: &OutgoingMessage,
    scheduled_at: i64,
) -> Result<ScheduledMessage, AppError> {
    let row = sqlx::query_as(
        "INSERT INTO scheduled_messages
             (account_id, alias_id, to_addr, cc_addr, bcc_addr, subject, body, body_html, attachments, scheduled_at,
              in_reply_to, references_hdr)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING *",
    )
    .bind(message.account_id)
    .bind(message.alias_id)
    .bind(&message.to)
    .bind(&message.cc)
    .bind(&message.bcc)
    .bind(&message.subject)
    .bind(&message.body)
    .bind(&message.body_html)
    // why: Json<&T> serializes on bind — same serde path #[sqlx(json)]
    // uses on read, so the column round-trips without manual serde_json.
    .bind(sqlx::types::Json(&message.attachments))
    .bind(scheduled_at)
    .bind(&message.in_reply_to)
    .bind(&message.references)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Every parked message, soonest first — pending and missed alike.
pub async fn list(pool: &SqlitePool) -> Result<Vec<ScheduledMessage>, AppError> {
    let rows = sqlx::query_as("SELECT * FROM scheduled_messages ORDER BY scheduled_at, id")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Remove one parked message and return it; `None` when it is already gone
/// (someone else claimed it first) — callers treat that as "nothing to do".
pub async fn take(pool: &SqlitePool, id: i64) -> Result<Option<ScheduledMessage>, AppError> {
    let row = sqlx::query_as("DELETE FROM scheduled_messages WHERE id = ? RETURNING *")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Claim every pending message whose time has come: delete and return them
/// for delivery. Missed rows are deliberately excluded — they only leave via
/// `take` after the user decides.
pub async fn claim_due(pool: &SqlitePool, now: i64) -> Result<Vec<ScheduledMessage>, AppError> {
    let rows = sqlx::query_as(
        "DELETE FROM scheduled_messages
         WHERE status = 'pending' AND scheduled_at <= ?
         RETURNING *",
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Flag pending messages due at or before `cutoff` as missed and return
/// them (empty = nothing new to report). Startup passes `now` (anything
/// overdue was missed while the app was closed); the scheduler tick passes
/// `now - grace` so a Mac waking from a long sleep asks instead of sending.
pub async fn mark_missed(
    pool: &SqlitePool,
    cutoff: i64,
) -> Result<Vec<ScheduledMessage>, AppError> {
    let rows = sqlx::query_as(
        "UPDATE scheduled_messages SET status = 'missed'
         WHERE status = 'pending' AND scheduled_at <= ?
         RETURNING *",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    async fn account_id(pool: &SqlitePool) -> i64 {
        let account = crate::storage::accounts::insert(
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
        .unwrap();
        account.id
    }

    fn outgoing(account_id: i64, subject: &str) -> OutgoingMessage {
        OutgoingMessage {
            account_id,
            alias_id: None,
            to: "b@example.com".to_string(),
            cc: "".to_string(),
            bcc: "".to_string(),
            subject: subject.to_string(),
            body: "hello".to_string(),
            body_html: None,
            attachments: vec![],
            draft_message_id: None,
            in_reply_to: None,
            references: None,
            quote: None,
        }
    }

    #[tokio::test]
    async fn insert_returns_pending_row_mirroring_the_draft() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;

        let row = insert(&pool, &outgoing(account, "Hi"), 1_000)
            .await
            .unwrap();

        assert!(row.id > 0);
        assert_eq!(row.account_id, account);
        assert_eq!(row.to, "b@example.com");
        assert_eq!(row.subject, "Hi");
        assert_eq!(row.scheduled_at, 1_000);
        assert_eq!(row.status, "pending");
        assert_eq!(row.outgoing().body, "hello");
    }

    #[tokio::test]
    async fn alias_survives_the_round_trip() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        let alias = crate::storage::aliases::add(&pool, account, "Igor", "igor@vocalio.sk")
            .await
            .unwrap();

        let mut message = outgoing(account, "Hi");
        message.alias_id = Some(alias.id);
        let row = insert(&pool, &message, 1_000).await.unwrap();

        assert_eq!(row.alias_id, Some(alias.id));
        assert_eq!(row.outgoing().alias_id, Some(alias.id));
    }

    #[tokio::test]
    async fn deleting_the_alias_downgrades_the_row_to_the_account_address() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        let alias = crate::storage::aliases::add(&pool, account, "Igor", "igor@vocalio.sk")
            .await
            .unwrap();
        let mut message = outgoing(account, "Hi");
        message.alias_id = Some(alias.id);
        let row = insert(&pool, &message, 1_000).await.unwrap();

        crate::storage::aliases::delete(&pool, alias.id)
            .await
            .unwrap();

        // ON DELETE SET NULL — the parked send falls back, it never fails.
        let rows = list(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, row.id);
        assert_eq!(rows[0].alias_id, None);
    }

    #[tokio::test]
    async fn threading_identity_survives_the_round_trip() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        let mut message = outgoing(account, "Re: Hi");
        message.in_reply_to = Some("parent@x".to_string());
        message.references = Some("root@x parent@x".to_string());

        let row = insert(&pool, &message, 1_000).await.unwrap();

        let out = row.outgoing();
        assert_eq!(out.in_reply_to.as_deref(), Some("parent@x"));
        assert_eq!(out.references.as_deref(), Some("root@x parent@x"));
    }

    #[tokio::test]
    async fn attachments_survive_the_round_trip() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        let mut message = outgoing(account, "With file");
        message.attachments = vec![crate::models::AttachmentRef {
            path: "/tmp/report.pdf".to_string(),
            name: "report.pdf".to_string(),
        }];

        let row = insert(&pool, &message, 1_000).await.unwrap();

        assert_eq!(row.attachments.len(), 1);
        assert_eq!(row.attachments[0].name, "report.pdf");
        assert_eq!(row.outgoing().attachments[0].path, "/tmp/report.pdf");
    }

    #[tokio::test]
    async fn list_orders_by_scheduled_time() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        insert(&pool, &outgoing(account, "Later"), 2_000)
            .await
            .unwrap();
        insert(&pool, &outgoing(account, "Sooner"), 1_000)
            .await
            .unwrap();

        let subjects: Vec<String> = list(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.subject)
            .collect();

        assert_eq!(subjects, vec!["Sooner", "Later"]);
    }

    #[tokio::test]
    async fn take_is_one_shot() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        let row = insert(&pool, &outgoing(account, "Hi"), 1_000)
            .await
            .unwrap();

        let taken = take(&pool, row.id).await.unwrap();

        assert_eq!(taken.unwrap().subject, "Hi");
        assert!(take(&pool, row.id).await.unwrap().is_none());
        assert!(list(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn claim_due_takes_due_rows_and_leaves_future_ones() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        insert(&pool, &outgoing(account, "Due"), 1_000)
            .await
            .unwrap();
        insert(&pool, &outgoing(account, "Also due"), 1_500)
            .await
            .unwrap();
        insert(&pool, &outgoing(account, "Future"), 2_000)
            .await
            .unwrap();

        let claimed = claim_due(&pool, 1_500).await.unwrap();

        let subjects: Vec<&str> = claimed.iter().map(|m| m.subject.as_str()).collect();
        assert_eq!(subjects, vec!["Due", "Also due"]);
        let remaining = list(&pool).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].subject, "Future");
        // a second claim at the same time finds nothing — the delete was the claim
        assert!(claim_due(&pool, 1_500).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn claim_due_never_touches_missed_rows() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        insert(&pool, &outgoing(account, "Stale"), 1_000)
            .await
            .unwrap();
        mark_missed(&pool, 1_000).await.unwrap();

        assert!(claim_due(&pool, 9_000).await.unwrap().is_empty());
        assert_eq!(list(&pool).await.unwrap()[0].status, "missed");
    }

    #[tokio::test]
    async fn mark_missed_flags_only_pending_rows_up_to_the_cutoff() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        insert(&pool, &outgoing(account, "Old"), 1_000)
            .await
            .unwrap();
        insert(&pool, &outgoing(account, "Fresh"), 2_000)
            .await
            .unwrap();

        let missed = mark_missed(&pool, 1_500).await.unwrap();

        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].subject, "Old");
        assert_eq!(missed[0].status, "missed");
        // already-missed rows are not reported again on the next pass
        assert!(mark_missed(&pool, 1_500).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_an_account_cascades_its_scheduled_messages() {
        let pool = test_pool().await;
        let account = account_id(&pool).await;
        insert(&pool, &outgoing(account, "Hi"), 1_000)
            .await
            .unwrap();

        crate::storage::accounts::delete(&pool, account)
            .await
            .unwrap();

        assert!(list(&pool).await.unwrap().is_empty());
    }
}
