//! The local "address book" behind compose autocomplete: every address seen
//! on cached mail or typed into a sent message, ranked by how often and how
//! recently it appeared. Purely local — no external directory, ever.

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{Contact, SenderAnomaly};

/// One display-formatted address list ("Ann <a@x>, b@y") split into
/// `(name, email)` pairs.
///
/// why hand-rolled: the strings were formatted by mail::parse::format_addr
/// (names never quoted), so splitting on commas is right except when a NAME
/// contains a comma — then its leading fragment has no '@' and is dropped,
/// which only costs part of the display name, never the address.
pub fn split_address_list(list: &str) -> Vec<(String, String)> {
    list.split(',')
        .filter_map(|segment| {
            let segment = segment.trim();
            if let Some((name, rest)) = segment.split_once('<') {
                let email = rest.trim_end_matches('>').trim();
                is_plausible_email(email)
                    .then(|| (name.trim().trim_matches('"').to_string(), email.to_string()))
            } else {
                is_plausible_email(segment).then(|| (String::new(), segment.to_string()))
            }
        })
        .collect()
}

/// Loose sanity check — this guards a suggestion list, not delivery.
fn is_plausible_email(value: &str) -> bool {
    value.len() < 255
        && !value.contains(char::is_whitespace)
        && value.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && domain.contains('.') && !domain.ends_with('.')
        })
}

/// Record one sighting of every address in `lists` (each a comma-separated
/// display string) on a message dated `seen_at` (RFC3339, may be empty).
pub async fn harvest(pool: &SqlitePool, lists: &[&str], seen_at: &str) -> Result<(), AppError> {
    for list in lists {
        for (name, email) in split_address_list(list) {
            // why MAX(): sync order is not message order — an old folder
            // synced later must not roll last_seen backwards. Non-empty
            // names win so "a@x" never erases "Ann <a@x>".
            sqlx::query(
                "INSERT INTO contacts (email, name, seen_count, last_seen)
                 VALUES (?, ?, 1, ?)
                 ON CONFLICT (email) DO UPDATE SET
                   seen_count = seen_count + 1,
                   name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE name END,
                   last_seen = MAX(last_seen, excluded.last_seen)",
            )
            .bind(&email)
            .bind(&name)
            .bind(seen_at)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// Record the recipients of an outgoing message, stamped with the current
/// time — someone just written to should rank as the freshest contact.
///
/// why strftime: the timestamp must match the RFC3339 shape harvested from
/// message Date headers so MAX() string comparison stays meaningful, and
/// SQLite produces it without pulling in a date-time crate.
pub async fn harvest_sent(pool: &SqlitePool, lists: &[&str]) -> Result<(), AppError> {
    let now: String = sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now')")
        .fetch_one(pool)
        .await?;
    harvest(pool, lists, &now).await
}

/// One-time seed from the already-cached messages, run at startup while the
/// contacts table is still empty (rows cached before the table existed would
/// otherwise never be harvested — sync only touches new headers).
pub async fn backfill(pool: &SqlitePool) -> Result<u64, AppError> {
    let empty: i64 = sqlx::query_scalar("SELECT count(*) FROM contacts")
        .fetch_one(pool)
        .await?;
    if empty > 0 {
        return Ok(0);
    }
    let rows: Vec<(String, String, String, String, String)> =
        sqlx::query_as("SELECT from_addr, to_addr, cc_addr, reply_to_addr, date FROM messages")
            .fetch_all(pool)
            .await?;
    let mut harvested = 0;
    for (from, to, cc, reply_to, date) in rows {
        harvest(pool, &[&from, &to, &cc, &reply_to], &date).await?;
        harvested += 1;
    }
    Ok(harvested)
}

/// How many suggestions a compose field shows at most.
const SUGGEST_LIMIT: i64 = 8;

/// Contacts matching `query` as a prefix of the address or of any word of
/// the name, most-seen first, newest tiebreak. Empty query = empty list.
pub async fn suggest(pool: &SqlitePool, query: &str) -> Result<Vec<Contact>, AppError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // why escape: a literal % or _ typed by the user must not become a
    // wildcard that matches everything.
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let contacts = sqlx::query_as(
        r#"SELECT email, name FROM contacts
           WHERE email LIKE ?1 || '%' ESCAPE '\'
              OR name LIKE ?1 || '%' ESCAPE '\'
              OR name LIKE '% ' || ?1 || '%' ESCAPE '\'
           ORDER BY seen_count DESC, last_seen DESC, email
           LIMIT ?2"#,
    )
    .bind(escaped)
    .bind(SUGGEST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(contacts)
}

/// A name counts as "familiar" once seen this often on one address…
const FAMILIAR_MIN: i64 = 3;
/// …and an address still counts as "new" up to this many sightings.
/// why > 0: the message being viewed has usually been harvested already,
/// so a first-contact address arrives here with a count of at least 1.
const NEW_MAX: i64 = 2;

/// The Canary-style sender check: does this From line pair a familiar
/// display name with an address that name does not usually use?
///
/// Purely local — compares the message against the harvested contact
/// history, no external lookup. `own_emails` (the user's accounts and
/// aliases) never warn: two own accounts sharing one display name is
/// normal, not suspicious.
pub async fn sender_anomaly(
    pool: &SqlitePool,
    from: &str,
    own_emails: &[String],
) -> Result<Option<SenderAnomaly>, AppError> {
    let Some((name, email)) = split_address_list(from).into_iter().next() else {
        return Ok(None);
    };
    if name.is_empty()
        || own_emails
            .iter()
            .any(|own| own.eq_ignore_ascii_case(&email))
    {
        return Ok(None);
    }
    // Every address this display name has appeared with, busiest first.
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT email, seen_count FROM contacts
         WHERE name = ? COLLATE NOCASE
         ORDER BY seen_count DESC, email",
    )
    .bind(&name)
    .fetch_all(pool)
    .await?;
    let this_count = rows
        .iter()
        .find(|(e, _)| e.eq_ignore_ascii_case(&email))
        .map_or(0, |(_, count)| *count);
    match rows.iter().find(|(e, _)| !e.eq_ignore_ascii_case(&email)) {
        Some((usual_email, count)) if *count >= FAMILIAR_MIN && this_count <= NEW_MAX => {
            Ok(Some(SenderAnomaly {
                name,
                usual_email: usual_email.clone(),
            }))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    #[test]
    fn splits_formatted_address_lists() {
        assert_eq!(
            split_address_list("Ann Boe <ann@example.com>, bob@example.com"),
            vec![
                ("Ann Boe".to_string(), "ann@example.com".to_string()),
                (String::new(), "bob@example.com".to_string()),
            ]
        );
        // A comma inside the display name costs its leading fragment only.
        assert_eq!(
            split_address_list("Novák, Ján <jan@example.sk>"),
            vec![("Ján".to_string(), "jan@example.sk".to_string())]
        );
        assert_eq!(split_address_list(""), Vec::new());
        assert_eq!(split_address_list("not an address"), Vec::new());
        assert_eq!(split_address_list("half@"), Vec::new());
        assert_eq!(split_address_list("x@nodot"), Vec::new());
    }

    #[tokio::test]
    async fn harvest_counts_keeps_names_and_newest_date() {
        let pool = test_pool().await;

        harvest(&pool, &["Ann <ann@example.com>"], "2026-07-02T00:00:00Z")
            .await
            .unwrap();
        // Same address again: bare form, older date — count grows, the
        // name stays, the date does not move backwards.
        harvest(&pool, &["ann@example.com"], "2026-07-01T00:00:00Z")
            .await
            .unwrap();

        let (name, count, last): (String, i64, String) =
            sqlx::query_as("SELECT name, seen_count, last_seen FROM contacts WHERE email = ?")
                .bind("ann@example.com")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(name, "Ann");
        assert_eq!(count, 2);
        assert_eq!(last, "2026-07-02T00:00:00Z");
    }

    #[tokio::test]
    async fn harvest_dedupes_case_insensitively() {
        let pool = test_pool().await;
        harvest(&pool, &["Ann@Example.com"], "").await.unwrap();
        harvest(&pool, &["ann@example.com"], "").await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM contacts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn suggest_matches_email_and_name_prefixes_ranked_by_use() {
        let pool = test_pool().await;
        harvest(
            &pool,
            &["Ján Novák <jan@example.sk>"],
            "2026-07-01T00:00:00Z",
        )
        .await
        .unwrap();
        for _ in 0..3 {
            harvest(
                &pool,
                &["Jana Malá <jana@example.sk>"],
                "2026-07-02T00:00:00Z",
            )
            .await
            .unwrap();
        }
        harvest(&pool, &["other@example.com"], "2026-07-03T00:00:00Z")
            .await
            .unwrap();

        // Email prefix; the more-seen contact ranks first.
        let hits = suggest(&pool, "jan").await.unwrap();
        let emails: Vec<&str> = hits.iter().map(|c| c.email.as_str()).collect();
        assert_eq!(emails, vec!["jana@example.sk", "jan@example.sk"]);

        // Second word of a name matches too.
        let by_surname = suggest(&pool, "Novák").await.unwrap();
        assert_eq!(by_surname.len(), 1);
        assert_eq!(by_surname[0].email, "jan@example.sk");
        assert_eq!(by_surname[0].name, "Ján Novák");

        assert_eq!(suggest(&pool, "").await.unwrap(), Vec::new());
        assert_eq!(suggest(&pool, "zzz").await.unwrap(), Vec::new());
        // LIKE wildcards typed by the user stay literal.
        assert_eq!(suggest(&pool, "%").await.unwrap(), Vec::new());
    }

    #[tokio::test]
    async fn harvest_sent_stamps_recipients_with_now() {
        let pool = test_pool().await;
        harvest(&pool, &["old@example.com"], "2026-07-01T00:00:00Z")
            .await
            .unwrap();

        harvest_sent(&pool, &["new@example.com", ""]).await.unwrap();

        // The just-written address outranks the older, equally-seen one.
        let hits = suggest(&pool, "e").await.unwrap();
        assert_eq!(hits.len(), 0); // prefix "e" matches neither address

        let all = suggest(&pool, "new").await.unwrap();
        assert_eq!(all.len(), 1);
        let last: String =
            sqlx::query_scalar("SELECT last_seen FROM contacts WHERE email = 'new@example.com'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(last.as_str() > "2026-07-01T00:00:00Z");
    }

    #[tokio::test]
    async fn backfill_seeds_from_cached_messages_once() {
        let pool = test_pool().await;
        let account = crate::storage::accounts::insert(
            &pool,
            &crate::models::NewAccount {
                name: "P".to_string(),
                email: "me@example.com".to_string(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                username: "me@example.com".to_string(),
            },
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO messages (account_id, uid, uid_validity, date, from_addr, to_addr)
             VALUES (?, 1, 1, '2026-07-01T00:00:00Z', 'Ann <ann@example.com>', 'me@example.com')",
        )
        .bind(account.id)
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(backfill(&pool).await.unwrap(), 1);
        assert_eq!(suggest(&pool, "ann").await.unwrap().len(), 1);

        // Table no longer empty — the second run must not double-count.
        assert_eq!(backfill(&pool).await.unwrap(), 0);
        let count: i64 =
            sqlx::query_scalar("SELECT seen_count FROM contacts WHERE email = 'ann@example.com'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1);
    }

    /// Shorthand: record `n` sightings of one formatted address.
    async fn seen(pool: &SqlitePool, addr: &str, n: usize) {
        for _ in 0..n {
            harvest(pool, &[addr], "2026-07-01T00:00:00Z")
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn warns_when_a_familiar_name_writes_from_a_new_address() {
        let pool = test_pool().await;
        seen(&pool, "Jakub Šarvaic <jakub@satori.sk>", 3).await;
        seen(&pool, "Jakub Šarvaic <jakub@hellosatori.sk>", 1).await;

        let anomaly = sender_anomaly(&pool, "Jakub Šarvaic <jakub@hellosatori.sk>", &[])
            .await
            .unwrap();
        assert_eq!(
            anomaly,
            Some(SenderAnomaly {
                name: "Jakub Šarvaic".to_string(),
                usual_email: "jakub@satori.sk".to_string(),
            })
        );
    }

    #[tokio::test]
    async fn stays_quiet_for_the_established_address_itself() {
        let pool = test_pool().await;
        seen(&pool, "Ann <ann@example.com>", 5).await;

        let anomaly = sender_anomaly(&pool, "Ann <ann@example.com>", &[])
            .await
            .unwrap();
        assert_eq!(anomaly, None);
    }

    #[tokio::test]
    async fn stays_quiet_once_the_new_address_is_established_too() {
        let pool = test_pool().await;
        seen(&pool, "Ann <ann@example.com>", 5).await;
        seen(&pool, "Ann <ann@other.com>", 3).await;

        let anomaly = sender_anomaly(&pool, "Ann <ann@other.com>", &[])
            .await
            .unwrap();
        assert_eq!(anomaly, None);
    }

    #[tokio::test]
    async fn stays_quiet_when_the_name_is_barely_known_elsewhere() {
        let pool = test_pool().await;
        seen(&pool, "Ann <ann@example.com>", 2).await;

        let anomaly = sender_anomaly(&pool, "Ann <ann@other.com>", &[])
            .await
            .unwrap();
        assert_eq!(anomaly, None);
    }

    #[tokio::test]
    async fn stays_quiet_for_unknown_names_and_bare_addresses() {
        let pool = test_pool().await;
        seen(&pool, "Ann <ann@example.com>", 5).await;

        assert_eq!(
            sender_anomaly(&pool, "Bob <bob@example.com>", &[])
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            sender_anomaly(&pool, "ann@other.com", &[]).await.unwrap(),
            None
        );
        assert_eq!(sender_anomaly(&pool, "", &[]).await.unwrap(), None);
    }

    #[tokio::test]
    async fn matches_names_case_insensitively() {
        let pool = test_pool().await;
        seen(&pool, "ANN BOE <ann@example.com>", 3).await;

        let anomaly = sender_anomaly(&pool, "Ann Boe <ann@other.com>", &[])
            .await
            .unwrap();
        assert_eq!(anomaly.unwrap().usual_email, "ann@example.com");
    }

    #[tokio::test]
    async fn never_warns_about_the_users_own_addresses() {
        let pool = test_pool().await;
        seen(&pool, "Dominik Mery <hello@vocalio.sk>", 5).await;
        seen(&pool, "Dominik Mery <dominik@vocalio.sk>", 1).await;

        let anomaly = sender_anomaly(
            &pool,
            "Dominik Mery <dominik@vocalio.sk>",
            &[
                "hello@vocalio.sk".to_string(),
                "Dominik@Vocalio.sk".to_string(),
            ],
        )
        .await
        .unwrap();
        assert_eq!(anomaly, None);
    }
}
