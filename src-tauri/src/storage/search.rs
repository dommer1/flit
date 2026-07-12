//! Gmail-style search query parsing: `from:x is:unread faktúra`.

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
}
