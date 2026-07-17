//! Gmail-style search: query parsing (`from:x is:unread faktúra`) and the
//! SQL that runs it against the message cache + FTS5 index.

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::MessageHeader;

/// A search query broken into structured filters plus free text.
///
/// Operator tokens (`from:`, `to:`, `subject:`, `is:read`, `is:unread`,
/// `before:`, `after:`) become filters matched against columns; everything
/// else is free text for the full-text index. Values with spaces are quoted:
/// `from:"Ján Novák"`. Unknown `word:value` tokens stay free text, so
/// subjects like "Re: faktúra" remain searchable.
#[derive(Debug, Default, PartialEq)]
pub struct SearchQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub subject: Option<String>,
    /// `is:read` → `Some(true)`, `is:unread` → `Some(false)`.
    pub read: Option<bool>,
    /// YYYY-MM-DD, inclusive lower bound on the message date.
    pub after: Option<String>,
    /// YYYY-MM-DD, exclusive upper bound on the message date.
    pub before: Option<String>,
    /// `in:archive` — folder name, matched case-insensitively. Without it a
    /// search spans every folder (like Gmail's All Mail).
    pub mailbox: Option<String>,
    pub text: String,
}

pub fn parse_query(input: &str) -> SearchQuery {
    let mut query = SearchQuery::default();
    let mut text = Vec::new();

    for token in tokenize(input) {
        // why: split_once borrows `token`, but the fallback arms need to move
        // it into `text` — owning the pieces up front sidesteps the conflict.
        let operator = token
            .split_once(':')
            .map(|(op, value)| (op.to_ascii_lowercase(), value.to_string()));
        match operator {
            Some((op, value)) if !value.is_empty() => match op.as_str() {
                "from" => query.from = Some(value),
                "to" => query.to = Some(value),
                "subject" => query.subject = Some(value),
                "is" if value.eq_ignore_ascii_case("read") => query.read = Some(true),
                "is" if value.eq_ignore_ascii_case("unread") => query.read = Some(false),
                // Unsupported is:/date values are dropped: searching for them
                // literally would silently return nothing useful.
                "is" => {}
                "after" if is_iso_date(&value) => query.after = Some(value),
                "before" if is_iso_date(&value) => query.before = Some(value),
                "after" | "before" => {}
                "in" => query.mailbox = Some(value),
                _ => text.push(token),
            },
            _ => text.push(token),
        }
    }

    query.text = text.join(" ");
    query
}

/// Split on whitespace, except inside double quotes; the quotes themselves
/// are dropped (`from:"Ján Novák"` → one token `from:Ján Novák`).
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in input.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// why: enough for any result list a human scrolls, and it caps the IPC
/// payload — the full match set stays in SQLite.
const RESULT_LIMIT: i64 = 200;

/// Run a parsed query against the cache. `account_id = None` searches all
/// accounts (unified inbox); operators filter columns, free text goes to the
/// FTS5 index. Newest first.
pub async fn search(
    pool: &SqlitePool,
    account_id: Option<i64>,
    query: &SearchQuery,
) -> Result<Vec<MessageHeader>, AppError> {
    // why: one static SQL with `(? IS NULL OR …)` per filter instead of
    // building the string at runtime — sqlx 0.9 rejects runtime-built SQL
    // (SqlSafeStr), and a single shape keeps the query plan cached.
    let rows = sqlx::query_as(
        r#"SELECT id, account_id, mailbox, from_addr AS "from", to_addr AS "to", cc_addr AS cc,
                  reply_to_addr AS reply_to, bcc_addr AS bcc, subject, snippet, date, read, has_attachments,
                  COALESCE(message_id_hdr, '') AS message_id,
                  references_hdr AS "references"
           FROM messages
           WHERE (?1 IS NULL OR account_id = ?1)
             AND (?2 IS NULL OR from_addr LIKE '%' || ?2 || '%' ESCAPE '\')
             AND (?3 IS NULL OR to_addr LIKE '%' || ?3 || '%' ESCAPE '\'
                            OR cc_addr LIKE '%' || ?3 || '%' ESCAPE '\')
             AND (?4 IS NULL OR subject LIKE '%' || ?4 || '%' ESCAPE '\')
             AND (?5 IS NULL OR read = ?5)
             AND (?6 IS NULL OR date >= ?6)
             AND (?7 IS NULL OR date < ?7)
             AND (?8 IS NULL OR mailbox = ?8 COLLATE NOCASE)
             AND (?9 IS NULL OR id IN
                  (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?9))
           ORDER BY date DESC
           LIMIT ?10"#,
    )
    .bind(account_id)
    .bind(query.from.as_deref().map(escape_like))
    .bind(query.to.as_deref().map(escape_like))
    .bind(query.subject.as_deref().map(escape_like))
    .bind(query.read)
    .bind(query.after.as_deref())
    .bind(query.before.as_deref())
    .bind(query.mailbox.as_deref())
    .bind(fts_match_expr(&query.text))
    .bind(RESULT_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Free text → an FTS5 MATCH expression: every word quoted (so user input
/// can't inject MATCH syntax like NOT or ^), joined by implicit AND. The
/// last word matches as a prefix, so typing "výro" already finds "výrobu".
fn fts_match_expr(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let last = words.len().checked_sub(1)?;
    let expr = words
        .iter()
        .enumerate()
        .map(|(i, word)| {
            let quoted = format!("\"{}\"", word.replace('"', "\"\""));
            if i == last {
                format!("{quoted}*")
            } else {
                quoted
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some(expr)
}

/// Make a LIKE pattern fragment literal: escape the wildcards and the escape
/// character itself (the query uses ESCAPE '\').
fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Strictly YYYY-MM-DD. The date column is RFC3339 text, so a well-formed
/// prefix compares correctly as a plain string — no date parsing needed.
fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;
    use sqlx::SqlitePool;

    #[allow(clippy::too_many_arguments)]
    async fn insert_message(
        pool: &SqlitePool,
        account_id: i64,
        uid: i64,
        from: &str,
        to: &str,
        subject: &str,
        date: &str,
        read: bool,
        body_text: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO messages
               (account_id, uid, uid_validity, from_addr, to_addr, subject, date, read, body_text)
             VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?)",
        )
        .bind(account_id)
        .bind(uid)
        .bind(from)
        .bind(to)
        .bind(subject)
        .bind(date)
        .bind(read)
        .bind(body_text)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn test_account(pool: &SqlitePool, name: &str) -> i64 {
        crate::storage::accounts::insert(
            pool,
            &crate::models::NewAccount {
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

    async fn subjects_for(pool: &SqlitePool, account_id: Option<i64>, input: &str) -> Vec<String> {
        search(pool, account_id, &parse_query(input))
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.subject)
            .collect()
    }

    #[tokio::test]
    async fn to_operator_and_free_text_also_match_cc_recipients() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Personal").await;
        sqlx::query(
            "INSERT INTO messages
               (account_id, uid, uid_validity, from_addr, to_addr, cc_addr, subject, date, read)
             VALUES (?, 1, 1, 'a@example.com', 'bob@example.com',
                     'Dana Malá <dana@example.com>', 'Zápisnica', '2026-07-01T00:00:00Z', 0)",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

        // Structured operator reaches the new column…
        assert_eq!(
            subjects_for(&pool, Some(id), "to:dana").await,
            vec!["Zápisnica"]
        );
        // …and so does the rebuilt FTS index (with diacritics folded).
        assert_eq!(
            subjects_for(&pool, Some(id), "mala").await,
            vec!["Zápisnica"]
        );
    }

    #[tokio::test]
    async fn free_text_searches_bodies_and_folds_diacritics() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Personal").await;
        insert_message(
            &pool,
            id,
            1,
            "a@example.com",
            "",
            "Ponuka",
            "2026-07-01T00:00:00Z",
            false,
            Some("cenová ponuka na výrobu"),
        )
        .await;
        insert_message(
            &pool,
            id,
            2,
            "b@example.com",
            "",
            "Iné",
            "2026-07-02T00:00:00Z",
            false,
            Some("úplne iný obsah"),
        )
        .await;

        // Diacritics fold ("vyrobu" finds "výrobu"), but FTS does no Slovak
        // stemming — a different declension ("výroba") would not match.
        assert_eq!(subjects_for(&pool, None, "vyrobu").await, vec!["Ponuka"]);
        // The last word acts as a prefix — search-as-you-type finds partials.
        assert_eq!(subjects_for(&pool, None, "výro").await, vec!["Ponuka"]);
        assert!(subjects_for(&pool, None, "neexistuje").await.is_empty());
    }

    #[tokio::test]
    async fn from_operator_narrows_and_combines_with_text() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Personal").await;
        insert_message(
            &pool,
            id,
            1,
            "Dominik <dominik@vocalio.sk>",
            "",
            "Faktúra jún",
            "2026-07-01T00:00:00Z",
            false,
            Some("faktúra v prílohe"),
        )
        .await;
        insert_message(
            &pool,
            id,
            2,
            "Iný <iny@example.com>",
            "",
            "Faktúra máj",
            "2026-07-02T00:00:00Z",
            false,
            Some("faktúra v prílohe"),
        )
        .await;

        assert_eq!(
            subjects_for(&pool, None, "from:dominik@vocalio.sk faktura").await,
            vec!["Faktúra jún"]
        );
        assert_eq!(
            subjects_for(&pool, None, "from:dominik@vocalio.sk neexistuje").await,
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn read_flag_and_date_range_filter() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Personal").await;
        insert_message(
            &pool,
            id,
            1,
            "a@example.com",
            "",
            "Stará neprečítaná",
            "2026-05-01T10:00:00Z",
            false,
            None,
        )
        .await;
        insert_message(
            &pool,
            id,
            2,
            "a@example.com",
            "",
            "Nová neprečítaná",
            "2026-07-05T10:00:00Z",
            false,
            None,
        )
        .await;
        insert_message(
            &pool,
            id,
            3,
            "a@example.com",
            "",
            "Nová prečítaná",
            "2026-07-06T10:00:00Z",
            true,
            None,
        )
        .await;

        assert_eq!(
            subjects_for(&pool, None, "is:unread after:2026-07-01").await,
            vec!["Nová neprečítaná"]
        );
        assert_eq!(
            subjects_for(&pool, None, "before:2026-07-01").await,
            vec!["Stará neprečítaná"]
        );
    }

    #[tokio::test]
    async fn results_are_scoped_to_the_account_and_sorted_newest_first() {
        let pool = test_pool().await;
        let personal = test_account(&pool, "Personal").await;
        let work = test_account(&pool, "Work").await;
        insert_message(
            &pool,
            personal,
            1,
            "a@example.com",
            "",
            "Osobná zmluva",
            "2026-07-01T00:00:00Z",
            false,
            None,
        )
        .await;
        insert_message(
            &pool,
            work,
            1,
            "a@example.com",
            "",
            "Pracovná zmluva",
            "2026-07-02T00:00:00Z",
            false,
            None,
        )
        .await;

        assert_eq!(
            subjects_for(&pool, None, "zmluva").await,
            vec!["Pracovná zmluva", "Osobná zmluva"]
        );
        assert_eq!(
            subjects_for(&pool, Some(personal), "zmluva").await,
            vec!["Osobná zmluva"]
        );
    }

    #[tokio::test]
    async fn like_wildcards_in_operator_values_are_literal() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Personal").await;
        insert_message(
            &pool,
            id,
            1,
            "sale100x@example.com",
            "",
            "Wildcard",
            "2026-07-01T00:00:00Z",
            false,
            None,
        )
        .await;

        // A "%" in the value must not act as a LIKE wildcard.
        assert!(subjects_for(&pool, None, "from:100%").await.is_empty());
        assert_eq!(
            subjects_for(&pool, None, "from:100x").await,
            vec!["Wildcard"]
        );
    }

    #[test]
    fn plain_text_has_no_filters() {
        let q = parse_query("faktúra za jún");

        assert_eq!(
            q,
            SearchQuery {
                text: "faktúra za jún".to_string(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn empty_input_parses_to_default() {
        assert_eq!(parse_query("   "), SearchQuery::default());
    }

    #[test]
    fn from_operator_splits_off_free_text() {
        let q = parse_query("from:dominik@vocalio.sk faktúra za jún");

        assert_eq!(q.from.as_deref(), Some("dominik@vocalio.sk"));
        assert_eq!(q.text, "faktúra za jún");
    }

    #[test]
    fn operators_are_case_insensitive_and_last_wins() {
        let q = parse_query("FROM:a@example.com from:b@example.com");

        assert_eq!(q.from.as_deref(), Some("b@example.com"));
        assert_eq!(q.text, "");
    }

    #[test]
    fn quoted_values_keep_their_spaces() {
        let q = parse_query("from:\"Ján Novák\" subject:\"za jún\" zmluva");

        assert_eq!(q.from.as_deref(), Some("Ján Novák"));
        assert_eq!(q.subject.as_deref(), Some("za jún"));
        assert_eq!(q.text, "zmluva");
    }

    #[test]
    fn is_read_and_unread_set_the_flag() {
        assert_eq!(parse_query("is:read").read, Some(true));
        assert_eq!(parse_query("is:unread").read, Some(false));
        // Unsupported is: values are dropped, not searched literally.
        let starred = parse_query("is:starred zmluva");
        assert_eq!(starred.read, None);
        assert_eq!(starred.text, "zmluva");
    }

    #[test]
    fn date_operators_require_iso_dates() {
        let q = parse_query("after:2026-07-01 before:2026-08-01");
        assert_eq!(q.after.as_deref(), Some("2026-07-01"));
        assert_eq!(q.before.as_deref(), Some("2026-08-01"));

        let bad = parse_query("after:vcera zmluva");
        assert_eq!(bad.after, None);
        assert_eq!(bad.text, "zmluva");
    }

    #[test]
    fn unknown_operators_and_bare_colons_stay_free_text() {
        let q = parse_query("Re: has:attachment faktúra");

        assert_eq!(
            q,
            SearchQuery {
                text: "Re: has:attachment faktúra".to_string(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn to_operator_matches_recipients() {
        let q = parse_query("to:jan@example.com");

        assert_eq!(q.to.as_deref(), Some("jan@example.com"));
    }

    #[test]
    fn in_operator_captures_the_mailbox() {
        let q = parse_query("in:archive faktúra");

        assert_eq!(q.mailbox.as_deref(), Some("archive"));
        assert_eq!(q.text, "faktúra");
    }

    #[tokio::test]
    async fn in_operator_scopes_results_to_one_folder() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Personal").await;
        insert_message(
            &pool,
            id,
            1,
            "a@example.com",
            "",
            "V inboxe",
            "2026-07-01T00:00:00Z",
            false,
            Some("zmluva o dielo"),
        )
        .await;
        sqlx::query("UPDATE messages SET mailbox = 'Archive' WHERE uid = 1")
            .execute(&pool)
            .await
            .unwrap();
        insert_message(
            &pool,
            id,
            2,
            "a@example.com",
            "",
            "Tiež zmluva",
            "2026-07-02T00:00:00Z",
            false,
            Some("zmluva o dielo"),
        )
        .await;

        // Search spans all folders by default; in: narrows, case-insensitively.
        assert_eq!(
            subjects_for(&pool, None, "zmluva").await,
            vec!["Tiež zmluva", "V inboxe"]
        );
        assert_eq!(
            subjects_for(&pool, None, "in:archive zmluva").await,
            vec!["V inboxe"]
        );
    }
}
