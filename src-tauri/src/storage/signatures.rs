use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::Signature;

pub async fn list(pool: &SqlitePool) -> Result<Vec<Signature>, AppError> {
    let signatures = sqlx::query_as("SELECT * FROM signatures ORDER BY id")
        .fetch_all(pool)
        .await?;
    Ok(signatures)
}

/// Insert a new (empty) signature and return it with the assigned id.
pub async fn create(pool: &SqlitePool, name: &str) -> Result<Signature, AppError> {
    let signature = sqlx::query_as("INSERT INTO signatures (name) VALUES (?) RETURNING *")
        .bind(name)
        .fetch_one(pool)
        .await?;
    Ok(signature)
}

pub async fn update(pool: &SqlitePool, id: i64, name: &str, body: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE signatures SET name = ?, body = ? WHERE id = ?")
        .bind(name)
        .bind(body)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM signatures WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Make `signature_id` the default for exactly `account_ids` — accounts that
/// currently point at it but are not in the list get cleared.
pub async fn set_default_for_accounts(
    pool: &SqlitePool,
    signature_id: i64,
    account_ids: &[i64],
) -> Result<(), AppError> {
    // why a transaction: the clear and the re-assign must land together, or
    // a crash in between silently drops defaults.
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE accounts SET signature_id = NULL WHERE signature_id = ?")
        .bind(signature_id)
        .execute(&mut *tx)
        .await?;
    for account_id in account_ids {
        sqlx::query("UPDATE accounts SET signature_id = ? WHERE id = ?")
            .bind(signature_id)
            .bind(account_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    async fn account(pool: &SqlitePool, email: &str) -> i64 {
        crate::storage::accounts::insert(
            pool,
            &crate::models::NewAccount {
                name: "Test".to_string(),
                email: email.to_string(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                username: email.to_string(),
            },
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn create_update_list_delete_roundtrip() {
        let pool = test_pool().await;

        let sig = create(&pool, "hello@vocalio.sk").await.unwrap();
        assert_eq!(sig.name, "hello@vocalio.sk");
        assert_eq!(sig.body, "");

        update(&pool, sig.id, "Vocalio", "<p>S pozdravom</p>")
            .await
            .unwrap();
        let all = list(&pool).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Vocalio");
        assert_eq!(all[0].body, "<p>S pozdravom</p>");

        delete(&pool, sig.id).await.unwrap();
        assert!(list(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn defaults_are_exactly_the_given_accounts() {
        let pool = test_pool().await;
        let a = account(&pool, "a@example.com").await;
        let b = account(&pool, "b@example.com").await;
        let sig = create(&pool, "Sig").await.unwrap();

        set_default_for_accounts(&pool, sig.id, &[a, b])
            .await
            .unwrap();
        let accounts = crate::storage::accounts::list(&pool).await.unwrap();
        assert!(accounts.iter().all(|acc| acc.signature_id == Some(sig.id)));

        // Narrowing the list clears the account that fell out of it.
        set_default_for_accounts(&pool, sig.id, &[b]).await.unwrap();
        let accounts = crate::storage::accounts::list(&pool).await.unwrap();
        assert_eq!(
            accounts
                .iter()
                .find(|acc| acc.id == a)
                .unwrap()
                .signature_id,
            None
        );
        assert_eq!(
            accounts
                .iter()
                .find(|acc| acc.id == b)
                .unwrap()
                .signature_id,
            Some(sig.id)
        );
    }

    #[tokio::test]
    async fn claiming_an_account_steals_it_from_the_other_signature() {
        let pool = test_pool().await;
        let a = account(&pool, "a@example.com").await;
        let first = create(&pool, "First").await.unwrap();
        let second = create(&pool, "Second").await.unwrap();

        set_default_for_accounts(&pool, first.id, &[a])
            .await
            .unwrap();
        set_default_for_accounts(&pool, second.id, &[a])
            .await
            .unwrap();

        let accounts = crate::storage::accounts::list(&pool).await.unwrap();
        assert_eq!(accounts[0].signature_id, Some(second.id));
    }

    #[tokio::test]
    async fn deleting_a_signature_clears_account_defaults() {
        let pool = test_pool().await;
        let a = account(&pool, "a@example.com").await;
        let sig = create(&pool, "Sig").await.unwrap();
        set_default_for_accounts(&pool, sig.id, &[a]).await.unwrap();

        delete(&pool, sig.id).await.unwrap();

        // ON DELETE SET NULL — the account survives, its default is gone.
        let accounts = crate::storage::accounts::list(&pool).await.unwrap();
        assert_eq!(accounts[0].signature_id, None);
    }
}
