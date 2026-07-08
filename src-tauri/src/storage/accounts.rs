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

pub async fn list(pool: &SqlitePool) -> Result<Vec<Account>, AppError> {
    let accounts = sqlx::query_as("SELECT * FROM accounts ORDER BY id")
        .fetch_all(pool)
        .await?;
    Ok(accounts)
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
    async fn delete_of_missing_id_is_ok() {
        let pool = test_pool().await;

        assert!(delete(&pool, 999).await.is_ok());
    }
}
