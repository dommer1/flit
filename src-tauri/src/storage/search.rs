//! Gmail-style search: query parsing (`from:x is:unread faktúra`) and the
//! SQL that runs it against the message cache + FTS5 index.

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::MessageHeader;
use crate::storage::messages::header_columns;

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
    /// YYYY-MM-DD, inclusive lower bound: from that local day's midnight on.
    pub after: Option<String>,
    /// YYYY-MM-DD, exclusive upper bound: everything before that local day.
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

/// How many matched rows the dedup pass looks at, before it collapses them
/// to at most `RESULT_LIMIT` messages.
///
/// why bounded at all: the copies of one message can only be found by
/// comparing rows, and comparing every row a broad query matches is what
/// made this slow — ranking all matches of `from:a` on a 97k-message cache
/// took 2.5 s against 8 ms for the plain query. Ranking a fixed window of
/// the newest matches instead costs 56 ms there, and 32 ms on a real
/// `from:` search.
///
/// why 5×: copies of a message carry the same date, so they land in the
/// same window; five folders per message is already more than Gmail's
/// inbox + All Mail + Important + label. A message duplicated beyond that
/// only costs the search a few rows of its 200, never a wrong result.
const CANDIDATE_LIMIT: i64 = RESULT_LIMIT * 5;

/// Run a parsed query against the cache. `account_id = None` searches all
/// accounts (unified inbox); operators filter columns, free text goes to the
/// FTS5 index. Newest first, one row per message.
pub async fn search(
    pool: &SqlitePool,
    account_id: Option<i64>,
    query: &SearchQuery,
) -> Result<Vec<MessageHeader>, AppError> {
    // why: one static SQL with `(? IS NULL OR …)` per filter instead of
    // building the string at runtime — sqlx 0.9 rejects runtime-built SQL
    // (SqlSafeStr), and a single shape keeps the query plan cached.
    //
    // why strftime(…, 'utc') around the date bounds: the column stores UTC,
    // but "after:2026-07-10" means the user's own day. SQLite reads the bare
    // date as local time and shifts it to the UTC instant that day begins at,
    // using the OS zone rules for THAT date — so the boundary follows DST.
    //
    // why the dedup pass: a search spans every folder, and an IMAP server
    // files one RFC message in several — Gmail's labels are folders, so a
    // mail in the inbox is also in All Mail, Important and each of its
    // labels. Measured on a real cache: 67,533 of 97,782 rows were copies,
    // which is why one hit showed up four times in the result list. The
    // folder lists never saw this: they are scoped to one folder.
    //
    // why the dedup only looks at the matched rows, not the whole table:
    // an `in:` search must still show that folder's copy — if the ranking
    // could reach outside the match set it would drop the archived copy in
    // favour of an inbox copy the user did not ask for, and return nothing.
    let rows = sqlx::query_as(concat!(
        r#"WITH candidates AS (
             SELECT * FROM messages
             WHERE (?1 IS NULL OR account_id = ?1)
               AND (?2 IS NULL OR from_addr LIKE '%' || ?2 || '%' ESCAPE '\')
               AND (?3 IS NULL OR to_addr LIKE '%' || ?3 || '%' ESCAPE '\'
                              OR cc_addr LIKE '%' || ?3 || '%' ESCAPE '\')
               AND (?4 IS NULL OR subject LIKE '%' || ?4 || '%' ESCAPE '\')
               AND (?5 IS NULL OR read = ?5)
               AND (?6 IS NULL OR date >= strftime('%Y-%m-%dT%H:%M:%SZ', ?6, 'utc'))
               AND (?7 IS NULL OR date < strftime('%Y-%m-%dT%H:%M:%SZ', ?7, 'utc'))
               AND (?8 IS NULL OR mailbox = ?8 COLLATE NOCASE)
               AND (?9 IS NULL OR id IN
                    (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?9))
             ORDER BY date DESC
             LIMIT ?10
           ),
           -- One row per message: copies share a Message-ID within an
           -- account, and the copy in the most meaningful folder speaks for
           -- them — the folder the user thinks of the mail as living in, and
           -- the one where opening it and acting on it does what they mean.
           -- A row whose sender set no Message-ID can never be matched to
           -- another row, so it stands alone under a key only it can have.
           deduped AS (
             SELECT c.*, ROW_NUMBER() OVER (
                      PARTITION BY c.account_id,
                                   COALESCE(NULLIF(c.message_id_hdr, ''), 'row:' || c.id)
                      ORDER BY CASE COALESCE(b.role, '')
                                 WHEN 'inbox' THEN 0
                                 WHEN 'sent' THEN 1
                                 WHEN 'drafts' THEN 2
                                 WHEN 'archive' THEN 3
                                 WHEN 'all' THEN 4
                                 WHEN 'junk' THEN 6
                                 WHEN 'trash' THEN 7
                                 ELSE 5
                               END, c.id
                    ) AS rn
             FROM candidates c
             LEFT JOIN mailboxes b
               ON b.account_id = c.account_id AND b.name = c.mailbox
           )
           SELECT "#,
        header_columns!("m"),
        r#" FROM deduped m
           WHERE m.rn = 1
           ORDER BY m.date DESC
           LIMIT ?11"#
    ))
    .bind(account_id)
    .bind(query.from.as_deref().map(escape_like))
    .bind(query.to.as_deref().map(escape_like))
    .bind(query.subject.as_deref().map(escape_like))
    .bind(query.read)
    .bind(query.after.as_deref())
    .bind(query.before.as_deref())
    .bind(query.mailbox.as_deref())
    .bind(fts_match_expr(&query.text))
    .bind(CANDIDATE_LIMIT)
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

/// Strictly YYYY-MM-DD — the shape SQLite turns into that local day's
/// midnight when the query converts the bound to UTC.
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

    /// One folder's copy of a message: the same RFC message filed under two
    /// Gmail labels is two rows sharing a Message-ID.
    async fn insert_copy(
        pool: &SqlitePool,
        account_id: i64,
        uid: i64,
        mailbox: &str,
        message_id: &str,
        subject: &str,
        date: &str,
    ) {
        sqlx::query(
            "INSERT INTO messages
               (account_id, mailbox, uid, uid_validity, from_addr, subject, date, message_id_hdr)
             VALUES (?, ?, ?, 1, 'm.gasparek@example.com', ?, ?, ?)",
        )
        .bind(account_id)
        .bind(mailbox)
        .bind(uid)
        .bind(subject)
        .bind(date)
        .bind(message_id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn folders(pool: &SqlitePool, account_id: i64, folders: &[(&str, Option<&str>)]) {
        let discovered: Vec<_> = folders
            .iter()
            .map(
                |(name, role)| crate::storage::mailboxes::DiscoveredMailbox {
                    name: name.to_string(),
                    role: role.map(str::to_string),
                },
            )
            .collect();
        crate::storage::mailboxes::replace(pool, account_id, &discovered)
            .await
            .unwrap();
    }

    async fn mailboxes_for(pool: &SqlitePool, input: &str) -> Vec<String> {
        search(pool, None, &parse_query(input))
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.mailbox)
            .collect()
    }

    /// Gmail files one message under every label it carries, so a flat
    /// search across folders saw the same mail four times.
    #[tokio::test]
    async fn folder_copies_of_one_message_collapse_to_a_single_hit() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Work").await;
        folders(
            &pool,
            id,
            &[
                ("INBOX", Some("inbox")),
                ("[Gmail]/All Mail", Some("all")),
                ("[Gmail]/Important", None),
            ],
        )
        .await;
        for (uid, mailbox) in [
            (1, "[Gmail]/All Mail"),
            (2, "[Gmail]/Important"),
            (3, "INBOX"),
        ] {
            insert_copy(
                &pool,
                id,
                uid,
                mailbox,
                "abc@example.com",
                "kontrola",
                "2026-09-03T05:27:45Z",
            )
            .await;
        }

        // One row per real message, and the inbox copy represents it — that
        // is the folder the user thinks of the mail as living in.
        assert_eq!(mailboxes_for(&pool, "gasparek").await, vec!["INBOX"]);
        assert_eq!(mailboxes_for(&pool, "from:gasparek").await, vec!["INBOX"]);
    }

    /// Ranking, not row order: the archived copy wins over a bare label even
    /// when the label's row was cached first.
    #[tokio::test]
    async fn the_most_meaningful_folder_represents_the_message() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Work").await;
        folders(
            &pool,
            id,
            &[
                ("[Gmail]/Important", None),
                ("[Gmail]/All Mail", Some("all")),
                ("[Gmail]/Trash", Some("trash")),
            ],
        )
        .await;
        insert_copy(
            &pool,
            id,
            1,
            "[Gmail]/Trash",
            "abc@x.sk",
            "kontrola",
            "2026-09-03T05:27:45Z",
        )
        .await;
        insert_copy(
            &pool,
            id,
            2,
            "[Gmail]/Important",
            "abc@x.sk",
            "kontrola",
            "2026-09-03T05:27:45Z",
        )
        .await;
        insert_copy(
            &pool,
            id,
            3,
            "[Gmail]/All Mail",
            "abc@x.sk",
            "kontrola",
            "2026-09-03T05:27:45Z",
        )
        .await;

        assert_eq!(
            mailboxes_for(&pool, "gasparek").await,
            vec!["[Gmail]/All Mail"]
        );
    }

    /// The dedup looks only at the rows a search actually matched, so a
    /// folder-scoped search still shows that folder's copy.
    #[tokio::test]
    async fn a_folder_scoped_search_keeps_that_folders_copy() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Work").await;
        folders(
            &pool,
            id,
            &[("INBOX", Some("inbox")), ("[Gmail]/All Mail", Some("all"))],
        )
        .await;
        insert_copy(
            &pool,
            id,
            1,
            "INBOX",
            "abc@x.sk",
            "kontrola",
            "2026-09-03T05:27:45Z",
        )
        .await;
        insert_copy(
            &pool,
            id,
            2,
            "[Gmail]/All Mail",
            "abc@x.sk",
            "kontrola",
            "2026-09-03T05:27:45Z",
        )
        .await;

        assert_eq!(
            mailboxes_for(&pool, "in:\"[Gmail]/All Mail\" gasparek").await,
            vec!["[Gmail]/All Mail"]
        );
    }

    /// Same subject, different messages — a thread is not a duplicate.
    #[tokio::test]
    async fn distinct_messages_are_never_merged() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Work").await;
        folders(&pool, id, &[("INBOX", Some("inbox"))]).await;
        insert_copy(
            &pool,
            id,
            1,
            "INBOX",
            "one@x.sk",
            "kontrola",
            "2026-09-03T05:00:00Z",
        )
        .await;
        insert_copy(
            &pool,
            id,
            2,
            "INBOX",
            "two@x.sk",
            "kontrola",
            "2026-09-03T06:00:00Z",
        )
        .await;
        // A message whose sender set no Message-ID can never be matched to
        // another row, so both such rows stand on their own.
        sqlx::query("UPDATE messages SET message_id_hdr = '' WHERE uid IN (1, 2)")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(mailboxes_for(&pool, "gasparek").await.len(), 2);
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
    async fn date_bounds_are_the_users_own_midnight_not_utcs() {
        let pool = test_pool().await;
        let id = test_account(&pool, "Personal").await;
        // The instant the local day starts, as the cache stores it: in a
        // +02:00 summer zone that is 2026-07-09T22:00:00Z. Asked from SQLite
        // so the test states the rule instead of hardcoding one zone.
        let midnight: String =
            sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', '2026-07-10', 'utc')")
                .fetch_one(&pool)
                .await
                .unwrap();
        let a_second_earlier: String = sqlx::query_scalar(
            "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', '2026-07-10', 'utc', '-1 second')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        insert_message(
            &pool,
            id,
            1,
            "a@x.sk",
            "",
            "Včera",
            &a_second_earlier,
            true,
            None,
        )
        .await;
        insert_message(&pool, id, 2, "a@x.sk", "", "Dnes", &midnight, true, None).await;

        // Both mails sit on the same UTC day, so a naive text bound would
        // put them on the same side of it — the local day parts them.
        assert_eq!(
            subjects_for(&pool, None, "after:2026-07-10").await,
            vec!["Dnes"]
        );
        assert_eq!(
            subjects_for(&pool, None, "before:2026-07-10").await,
            vec!["Včera"]
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
