use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{Account, NewAccount};

/// Insert an account and return it with the assigned id.
pub async fn insert(pool: &SqlitePool, account: &NewAccount) -> Result<Account, AppError> {
    // why: RETURNING gives back the full row (incl. the generated id) in one
    // round trip, instead of a separate last_insert_rowid + SELECT.
    let inserted = sqlx::query_as(
        "INSERT INTO accounts (name, email, imap_host, imap_port, smtp_host, smtp_port, username)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING *",
    )
    .bind(&account.name)
    .bind(&account.email)
    .bind(&account.imap_host)
    .bind(account.imap_port)
    .bind(&account.smtp_host)
    .bind(account.smtp_port)
    .bind(&account.username)
    .fetch_one(pool)
    .await?;
    Ok(inserted)
}

/// One account by id; errors (RowNotFound) when it doesn't exist.
pub async fn get(pool: &SqlitePool, id: i64) -> Result<Account, AppError> {
    let account = sqlx::query_as("SELECT * FROM accounts WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(account)
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<Account>, AppError> {
    let accounts = sqlx::query_as("SELECT * FROM accounts ORDER BY id")
        .fetch_all(pool)
        .await?;
    Ok(accounts)
}

/// Every address the user sends as — account addresses plus stored
/// aliases. Used to exempt the user's own identities from sender warnings.
pub async fn own_emails(pool: &SqlitePool) -> Result<Vec<String>, AppError> {
    let emails =
        sqlx::query_scalar("SELECT email FROM accounts UNION SELECT email FROM account_aliases")
            .fetch_all(pool)
            .await?;
    Ok(emails)
}

/// Record the outcome of a connection check (`None` error = healthy).
pub async fn set_status(
    pool: &SqlitePool,
    id: i64,
    error: Option<&str>,
    checked_at: i64,
) -> Result<(), AppError> {
    sqlx::query("UPDATE accounts SET last_error = ?, checked_at = ? WHERE id = ?")
        .bind(error)
        .bind(checked_at)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set (or clear, with `None`) an account's accent color.
pub async fn set_color(pool: &SqlitePool, id: i64, color: Option<&str>) -> Result<(), AppError> {
    sqlx::query("UPDATE accounts SET color = ? WHERE id = ?")
        .bind(color)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set (or clear, with `None`) an account's notification overrides. `None`
/// means "inherit the global default" — see storage::settings.
pub async fn set_notify(
    pool: &SqlitePool,
    id: i64,
    enabled: Option<bool>,
    sound: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE accounts SET notify_enabled = ?, notify_sound = ? WHERE id = ?")
        .bind(enabled)
        .bind(sound)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete an account row. Idempotent — deleting a missing id is not an error.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM accounts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    fn sample(name: &str) -> NewAccount {
        NewAccount {
            name: name.to_string(),
            email: format!("{}@example.com", name.to_lowercase()),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            username: format!("{}@example.com", name.to_lowercase()),
        }
    }

    #[tokio::test]
    async fn insert_returns_account_with_generated_id() {
        let pool = test_pool().await;

        let account = insert(&pool, &sample("Personal")).await.unwrap();

        assert!(account.id > 0);
        assert_eq!(account.name, "Personal");
        assert_eq!(account.imap_port, 993);
        assert_eq!(account.smtp_port, 587);
    }

    #[tokio::test]
    async fn list_returns_accounts_in_id_order() {
        let pool = test_pool().await;
        let first = insert(&pool, &sample("Personal")).await.unwrap();
        let second = insert(&pool, &sample("Work")).await.unwrap();

        let accounts = list(&pool).await.unwrap();

        let ids: Vec<i64> = accounts.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![first.id, second.id]);
        assert_eq!(accounts[1].email, "work@example.com");
    }

    #[tokio::test]
    async fn delete_removes_the_account() {
        let pool = test_pool().await;
        let account = insert(&pool, &sample("Personal")).await.unwrap();

        delete(&pool, account.id).await.unwrap();

        assert!(list(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn set_status_records_and_clears_the_error() {
        let pool = test_pool().await;
        let account = insert(&pool, &sample("Personal")).await.unwrap();
        assert_eq!(account.last_error, None);
        assert_eq!(account.checked_at, None);

        set_status(&pool, account.id, Some("imap error: login"), 100)
            .await
            .unwrap();
        let broken = get(&pool, account.id).await.unwrap();
        assert_eq!(broken.last_error.as_deref(), Some("imap error: login"));
        assert_eq!(broken.checked_at, Some(100));

        set_status(&pool, account.id, None, 200).await.unwrap();
        let healthy = get(&pool, account.id).await.unwrap();
        assert_eq!(healthy.last_error, None);
        assert_eq!(healthy.checked_at, Some(200));
    }

    #[tokio::test]
    async fn set_color_sets_and_clears_the_accent() {
        let pool = test_pool().await;
        let account = insert(&pool, &sample("Personal")).await.unwrap();
        assert_eq!(account.color, None);

        set_color(&pool, account.id, Some("#ff9f0a")).await.unwrap();
        assert_eq!(
            get(&pool, account.id).await.unwrap().color.as_deref(),
            Some("#ff9f0a")
        );

        set_color(&pool, account.id, None).await.unwrap();
        assert_eq!(get(&pool, account.id).await.unwrap().color, None);
    }

    #[tokio::test]
    async fn set_notify_sets_and_clears_the_overrides() {
        let pool = test_pool().await;
        let account = insert(&pool, &sample("Personal")).await.unwrap();
        // Fresh accounts inherit the global defaults.
        assert_eq!(account.notify_enabled, None);
        assert_eq!(account.notify_sound, None);

        set_notify(&pool, account.id, Some(false), Some("Ping"))
            .await
            .unwrap();
        let overridden = get(&pool, account.id).await.unwrap();
        assert_eq!(overridden.notify_enabled, Some(false));
        assert_eq!(overridden.notify_sound.as_deref(), Some("Ping"));

        set_notify(&pool, account.id, None, None).await.unwrap();
        let inherited = get(&pool, account.id).await.unwrap();
        assert_eq!(inherited.notify_enabled, None);
        assert_eq!(inherited.notify_sound, None);
    }

    #[tokio::test]
    async fn get_returns_the_account_or_errors() {
        let pool = test_pool().await;
        let inserted = insert(&pool, &sample("Personal")).await.unwrap();

        let found = get(&pool, inserted.id).await.unwrap();

        assert_eq!(found.email, "personal@example.com");
        assert!(get(&pool, 999).await.is_err());
    }

    #[tokio::test]
    async fn delete_of_missing_id_is_ok() {
        let pool = test_pool().await;

        assert!(delete(&pool, 999).await.is_ok());
    }

    #[tokio::test]
    async fn own_emails_covers_accounts_and_aliases() {
        let pool = test_pool().await;
        let account = insert(&pool, &sample("Personal")).await.unwrap();
        crate::storage::aliases::add(&pool, account.id, "P", "alias@example.com")
            .await
            .unwrap();

        let mut emails = own_emails(&pool).await.unwrap();
        emails.sort();
        assert_eq!(emails, vec!["alias@example.com", "personal@example.com"]);
    }
}
