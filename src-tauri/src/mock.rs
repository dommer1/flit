use crate::models::MessageHeader;

/// Messages for one account, or the unified inbox when `account_id` is `None`.
///
/// why: the unified inbox is not a special case — it's the same list with the
/// account filter dropped, merged and sorted by date desc. This mirrors the
/// SQLite query that replaces this module in Phase 1.
pub fn messages(account_id: Option<i64>) -> Vec<MessageHeader> {
    let mut messages: Vec<MessageHeader> = all_messages()
        .into_iter()
        .filter(|m| account_id.is_none_or(|id| m.account_id == id))
        .collect();
    messages.sort_by(|a, b| b.date.cmp(&a.date));
    messages
}

fn all_messages() -> Vec<MessageHeader> {
    let raw: [(i64, i64, &str, &str, &str, &str, bool); 8] = [
        (
            1,
            1,
            "Alice Novak <alice@example.com>",
            "Weekend plans",
            "Hey, are we still on for Saturday? I was thinking we could…",
            "2026-07-07T09:15:00Z",
            false,
        ),
        (
            2,
            1,
            "GitHub <noreply@github.com>",
            "[dommer1/flit] PR #1 merged",
            "Your pull request was merged into main…",
            "2026-07-08T08:02:00Z",
            false,
        ),
        (
            3,
            1,
            "Newsletter <news@rustweekly.dev>",
            "Rust Weekly #204",
            "This week: async closures stabilized, cargo tips, and…",
            "2026-07-06T06:30:00Z",
            true,
        ),
        (
            4,
            2,
            "Peter Kováč <peter@client.sk>",
            "Re: Invoice 2026-041",
            "Thanks, payment went out this morning. Could you also…",
            "2026-07-08T07:45:00Z",
            false,
        ),
        (
            5,
            2,
            "Zuzana <zuzana@vocalio.sk>",
            "Standup notes",
            "Quick summary from today: the deploy is scheduled for…",
            "2026-07-07T16:20:00Z",
            true,
        ),
        (
            6,
            2,
            "AWS <no-reply@aws.amazon.com>",
            "Your monthly bill is available",
            "Your invoice for June 2026 is now available in the…",
            "2026-07-05T03:10:00Z",
            true,
        ),
        (
            7,
            1,
            "Mom",
            "Photos from the trip",
            "Finally uploaded the photos, take a look when you have…",
            "2026-07-04T19:05:00Z",
            true,
        ),
        (
            8,
            2,
            "Peter Kováč <peter@client.sk>",
            "Invoice 2026-041",
            "Sending over the invoice for June, due in 14 days as…",
            "2026-07-03T10:00:00Z",
            true,
        ),
    ];

    raw.into_iter()
        .map(
            |(id, account_id, from, subject, snippet, date, read)| MessageHeader {
                id,
                account_id,
                from: from.to_string(),
                subject: subject.to_string(),
                snippet: snippet.to_string(),
                date: date.to_string(),
                read,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_inbox_returns_all_messages_newest_first() {
        let msgs = messages(None);

        assert_eq!(msgs.len(), 8);
        let dates: Vec<&str> = msgs.iter().map(|m| m.date.as_str()).collect();
        let mut sorted = dates.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(dates, sorted);
    }

    #[test]
    fn account_filter_returns_only_that_accounts_messages() {
        let msgs = messages(Some(2));

        assert!(!msgs.is_empty());
        assert!(msgs.iter().all(|m| m.account_id == 2));
    }

    #[test]
    fn unknown_account_returns_empty_list() {
        assert!(messages(Some(999)).is_empty());
    }
}
