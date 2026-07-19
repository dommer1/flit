use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::Alias;

/// Reject an address the send path could not build a From header from —
/// the same parse lettre applies at MIME build time, so a stored alias can
/// never fail later at send.
fn validate_email(email: &str) -> Result<(), AppError> {
    email
        .parse::<lettre::Address>()
        .map(|_| ())
        .map_err(|_| AppError::Invalid(format!("not a valid e-mail address: {email}")))
}

/// Every alias across all accounts — settings and the compose picker both
/// group client-side, so one query serves both.
pub async fn list(pool: &SqlitePool) -> Result<Vec<Alias>, AppError> {
    let aliases = sqlx::query_as("SELECT * FROM account_aliases ORDER BY account_id, id")
        .fetch_all(pool)
        .await?;
    Ok(aliases)
}

/// Insert a new alias and return it with the assigned id.
pub async fn add(
    pool: &SqlitePool,
    account_id: i64,
    name: &str,
    email: &str,
) -> Result<Alias, AppError> {
    let email = email.trim();
    validate_email(email)?;
    let alias = sqlx::query_as(
        "INSERT INTO account_aliases (account_id, name, email) VALUES (?, ?, ?) RETURNING *",
    )
    .bind(account_id)
    .bind(name.trim())
    .bind(email)
    .fetch_one(pool)
    .await?;
    Ok(alias)
}

pub async fn update(pool: &SqlitePool, id: i64, name: &str, email: &str) -> Result<(), AppError> {
    let email = email.trim();
    validate_email(email)?;
    sqlx::query("UPDATE account_aliases SET name = ?, email = ? WHERE id = ?")
        .bind(name.trim())
        .bind(email)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete an alias row. Idempotent — deleting a missing id is not an error.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM account_aliases WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Make `alias_id` the identity new mail from this account starts with;
/// `None` = back to the account's own address. An alias belonging to a
/// different account is refused, not silently accepted.
pub async fn set_default(
    pool: &SqlitePool,
    account_id: i64,
    alias_id: Option<i64>,
) -> Result<(), AppError> {
    if let Some(alias_id) = alias_id {
        let alias: Alias = sqlx::query_as("SELECT * FROM account_aliases WHERE id = ?")
            .bind(alias_id)
            .fetch_one(pool)
            .await?;
        if alias.account_id != account_id {
            return Err(AppError::Invalid(
                "alias belongs to a different account".to_string(),
            ));
        }
    }
    sqlx::query("UPDATE accounts SET default_alias_id = ? WHERE id = ?")
        .bind(alias_id)
        .bind(account_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The From identity for an outgoing message: its chosen alias, or the
/// account's own name and address. A foreign or unknown alias id is refused
/// — only addresses the user configured can ever leave as From.
pub async fn sender(
    pool: &SqlitePool,
    message: &crate::models::OutgoingMessage,
) -> Result<(String, String), AppError> {
    let account = crate::storage::accounts::get(pool, message.account_id).await?;
    let Some(alias_id) = message.alias_id else {
        return Ok((account.name, account.email));
    };
    let alias: Alias = sqlx::query_as("SELECT * FROM account_aliases WHERE id = ?")
        .bind(alias_id)
        .fetch_one(pool)
        .await?;
    if alias.account_id != account.id {
        return Err(AppError::Invalid(
            "alias belongs to a different account".to_string(),
        ));
    }
    Ok((alias.name, alias.email))
}

/// The account's alias with this address (case-insensitive), if any —
/// how a reopened draft's From header maps back to a send-as identity.
pub async fn find_by_email(
    pool: &SqlitePool,
    account_id: i64,
    email: &str,
) -> Result<Option<Alias>, AppError> {
    let alias = sqlx::query_as(
        "SELECT * FROM account_aliases WHERE account_id = ? AND email = ? COLLATE NOCASE",
    )
    .bind(account_id)
    .bind(email.trim())
    .fetch_optional(pool)
    .await?;
    Ok(alias)
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

    fn outgoing(account_id: i64, alias_id: Option<i64>) -> crate::models::OutgoingMessage {
        crate::models::OutgoingMessage {
            account_id,
            alias_id,
            to: "a@example.com".to_string(),
            cc: String::new(),
            bcc: String::new(),
            subject: String::new(),
            body: String::new(),
            body_html: None,
            attachments: Vec::new(),
            draft_message_id: None,
            in_reply_to: None,
            references: None,
            quote: None,
        }
    }

    #[tokio::test]
    async fn sender_is_the_account_without_an_alias() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;

        let (name, email) = sender(&pool, &outgoing(acc, None)).await.unwrap();
        assert_eq!(name, "Test");
        assert_eq!(email, "hello@vocalio.sk");
    }

    #[tokio::test]
    async fn sender_is_the_chosen_alias() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;
        let alias = add(&pool, acc, "Igor", "igor@vocalio.sk").await.unwrap();

        let (name, email) = sender(&pool, &outgoing(acc, Some(alias.id))).await.unwrap();
        assert_eq!(name, "Igor");
        assert_eq!(email, "igor@vocalio.sk");
    }

    #[tokio::test]
    async fn sender_refuses_a_foreign_alias() {
        let pool = test_pool().await;
        let mine = account(&pool, "hello@vocalio.sk").await;
        let other = account(&pool, "other@example.com").await;
        let foreign = add(&pool, other, "", "info@example.com").await.unwrap();

        assert!(sender(&pool, &outgoing(mine, Some(foreign.id)))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn sender_errors_on_an_unknown_alias() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;

        assert!(sender(&pool, &outgoing(acc, Some(999))).await.is_err());
    }

    #[tokio::test]
    async fn find_by_email_ignores_case_and_other_accounts() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;
        let other = account(&pool, "other@example.com").await;
        let alias = add(&pool, acc, "Igor", "igor@vocalio.sk").await.unwrap();

        let found = find_by_email(&pool, acc, "IGOR@vocalio.sk").await.unwrap();
        assert_eq!(found.map(|a| a.id), Some(alias.id));

        let foreign = find_by_email(&pool, other, "igor@vocalio.sk")
            .await
            .unwrap();
        assert!(foreign.is_none());
    }

    #[tokio::test]
    async fn add_update_list_delete_roundtrip() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;

        let alias = add(&pool, acc, "Igor", " igor@vocalio.sk ").await.unwrap();
        assert_eq!(alias.account_id, acc);
        assert_eq!(alias.name, "Igor");
        // Stray whitespace is trimmed before it can break the From header.
        assert_eq!(alias.email, "igor@vocalio.sk");

        update(&pool, alias.id, "Igor Novák", "igor@vocalio.sk")
            .await
            .unwrap();
        let all = list(&pool).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Igor Novák");

        delete(&pool, alias.id).await.unwrap();
        assert!(list(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_an_invalid_address() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;

        assert!(add(&pool, acc, "", "not-an-address").await.is_err());
        assert!(add(&pool, acc, "", "").await.is_err());
    }

    #[tokio::test]
    async fn rejects_a_duplicate_alias_on_the_same_account() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;

        add(&pool, acc, "", "igor@vocalio.sk").await.unwrap();
        assert!(add(&pool, acc, "", "igor@vocalio.sk").await.is_err());
    }

    #[tokio::test]
    async fn deleting_the_account_removes_its_aliases() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;
        add(&pool, acc, "", "igor@vocalio.sk").await.unwrap();

        crate::storage::accounts::delete(&pool, acc).await.unwrap();

        assert!(list(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn set_default_roundtrip_and_clear() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;
        let alias = add(&pool, acc, "Igor", "igor@vocalio.sk").await.unwrap();

        set_default(&pool, acc, Some(alias.id)).await.unwrap();
        let stored = crate::storage::accounts::get(&pool, acc).await.unwrap();
        assert_eq!(stored.default_alias_id, Some(alias.id));

        set_default(&pool, acc, None).await.unwrap();
        let stored = crate::storage::accounts::get(&pool, acc).await.unwrap();
        assert_eq!(stored.default_alias_id, None);
    }

    #[tokio::test]
    async fn set_default_refuses_a_foreign_alias() {
        let pool = test_pool().await;
        let mine = account(&pool, "hello@vocalio.sk").await;
        let other = account(&pool, "other@example.com").await;
        let alias = add(&pool, other, "", "info@example.com").await.unwrap();

        assert!(set_default(&pool, mine, Some(alias.id)).await.is_err());
    }

    #[tokio::test]
    async fn deleting_the_default_alias_clears_the_account_default() {
        let pool = test_pool().await;
        let acc = account(&pool, "hello@vocalio.sk").await;
        let alias = add(&pool, acc, "Igor", "igor@vocalio.sk").await.unwrap();
        set_default(&pool, acc, Some(alias.id)).await.unwrap();

        delete(&pool, alias.id).await.unwrap();

        // ON DELETE SET NULL — the account falls back to its own address.
        let stored = crate::storage::accounts::get(&pool, acc).await.unwrap();
        assert_eq!(stored.default_alias_id, None);
    }
}
