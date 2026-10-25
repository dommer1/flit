//! Reply, reply-all and forward drafts: who a draft goes to, from which
//! identity, under what subject and threading headers. `draft_for` gathers
//! the inputs from the cache; the rules themselves are pure functions.
//!
//! `date_label` is the original's date as the reader writes dates: it goes
//! into the attribution line and the forwarded header block word for word.

use std::collections::HashSet;

use tauri::AppHandle;

use crate::error::AppError;
use crate::mail::recipients;
use crate::models::{Alias, DraftKind, DraftQuote, MessageHeader, MessageQuote, OutgoingMessage};
use crate::state::AppState;
use crate::{mail, storage};

/// The compose draft for answering or forwarding one message.
///
/// why a failed body load still yields a draft: the quote is a courtesy —
/// offline, say, the user can still write the reply, just without it.
pub async fn draft_for(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    kind: DraftKind,
    date_label: &str,
) -> Result<OutgoingMessage, AppError> {
    let message = storage::messages::header(&state.pool, message_id).await?;
    // why the receiving account's address: reply-all must not echo mail back
    // to the mailbox it landed in.
    let own_email = storage::accounts::get(&state.pool, message.account_id)
        .await?
        .email;
    let aliases = storage::aliases::list(&state.pool).await?;
    if kind == DraftKind::Forward {
        // The text the viewer shows: the message's own words, its quoted
        // history left out.
        let text = match mail::body::load_body(app, state, message_id).await {
            Ok(loaded) => loaded
                .text
                .map(|text| mail::quote::split_text_quote(&text).0),
            Err(err) => {
                eprintln!("forwarding message {message_id} without its text: {err}");
                None
            }
        };
        return Ok(forward_draft(
            &message,
            text.as_deref(),
            date_label,
            &aliases,
        ));
    }
    let quote = match mail::body::get_message_quote(app, state, message_id).await {
        Ok(quote) => Some(quote),
        Err(err) => {
            eprintln!("replying to message {message_id} without a quote: {err}");
            None
        }
    };
    Ok(if kind == DraftKind::ReplyAll {
        reply_all_draft(&message, quote.as_ref(), date_label, &own_email, &aliases)
    } else {
        reply_draft(&message, quote.as_ref(), date_label, &own_email, &aliases)
    })
}

/// Reply goes from the account the message arrived on — and from the alias
/// it was addressed to, when one matches — to its sender. Replying to my own
/// sent message is a follow-up: it goes to the original recipients (mailing
/// myself would be useless) and keeps the identity it originally left from.
pub fn reply_draft(
    message: &MessageHeader,
    quote: Option<&MessageQuote>,
    date_label: &str,
    own_email: &str,
    aliases: &[Alias],
) -> OutgoingMessage {
    let from_address = sender_address(&message.from).to_lowercase();
    let sent_from_alias = aliases
        .iter()
        .find(|a| a.account_id == message.account_id && a.email.to_lowercase() == from_address);
    let is_own = from_address == own_email.trim().to_lowercase() || sent_from_alias.is_some();
    let alias = if is_own {
        sent_from_alias
    } else {
        matching_alias(message, aliases)
    };
    let (in_reply_to, references) = threading(message);
    OutgoingMessage {
        account_id: message.account_id,
        alias_id: alias.map(|a| a.id),
        // why the fallback: an own message with an empty To line would leave
        // the draft unsendable — degrade to replying to myself, like reply-all.
        to: if is_own && !message.to.trim().is_empty() {
            message.to.clone()
        } else {
            sender_address(&message.from).to_string()
        },
        subject: reply_subject(&message.subject),
        // The editor opens empty; the quote rides separately and is merged
        // into body/body_html at save/send time.
        body: String::new(),
        quote: draft_quote(message, quote, date_label),
        in_reply_to,
        references,
        ..Default::default()
    }
}

/// Reply-all: sender + the original To line on To, the original Cc line on
/// Cc — minus the receiving account's own address and any duplicates.
pub fn reply_all_draft(
    message: &MessageHeader,
    quote: Option<&MessageQuote>,
    date_label: &str,
    own_email: &str,
    aliases: &[Alias],
) -> OutgoingMessage {
    // why all account aliases, not just the matched one: every alias is "me"
    // — none of them belongs on the recipient lines of my own reply.
    let mut seen: HashSet<String> = aliases
        .iter()
        .filter(|a| a.account_id == message.account_id)
        .map(|a| a.email.to_lowercase())
        .collect();
    seen.insert(own_email.trim().to_lowercase());
    let mut keep = |entry: &String| {
        let address = sender_address(entry).to_lowercase();
        !address.is_empty() && seen.insert(address)
    };
    let to: Vec<String> = std::iter::once(message.from.clone())
        .chain(recipients::split(&message.to))
        .filter(&mut keep)
        .collect();
    let cc: Vec<String> = recipients::split(&message.cc)
        .into_iter()
        .filter(&mut keep)
        .collect();
    let (in_reply_to, references) = threading(message);
    OutgoingMessage {
        account_id: message.account_id,
        alias_id: matching_alias(message, aliases).map(|a| a.id),
        // why: everyone else can be me (replying to my own mail) — degrade to
        // a plain reply-to-sender rather than an unsendable empty To.
        to: if to.is_empty() {
            sender_address(&message.from).to_string()
        } else {
            to.join(", ")
        },
        cc: cc.join(", "),
        subject: reply_subject(&message.subject),
        body: String::new(),
        quote: draft_quote(message, quote, date_label),
        in_reply_to,
        references,
        ..Default::default()
    }
}

/// Forward: no recipient yet, the original riding below a header block.
pub fn forward_draft(
    message: &MessageHeader,
    body_text: Option<&str>,
    date_label: &str,
    aliases: &[Alias],
) -> OutgoingMessage {
    let mut header = vec![
        "---------- Forwarded message ----------".to_string(),
        format!("From: {}", message.from),
        format!("Date: {date_label}"),
        format!("Subject: {}", message.subject),
        format!("To: {}", message.to),
    ];
    if !message.cc.is_empty() {
        header.push(format!("Cc: {}", message.cc));
    }
    let body = format!("\n\n{}\n\n{}", header.join("\n"), body_text.unwrap_or(""));
    OutgoingMessage {
        account_id: message.account_id,
        alias_id: matching_alias(message, aliases).map(|a| a.id),
        subject: forward_subject(&message.subject),
        body: format!("{}\n", body.trim_end()),
        ..Default::default()
    }
}

/// Bare address out of `Name <addr>`; a plain address passes through.
fn sender_address(from: &str) -> &str {
    angle_address(from)
        .filter(|(_, addr)| !addr.is_empty())
        .map_or(from, |(_, addr)| addr)
        .trim()
}

/// Display name out of `Name <addr>` (one pair of quotes stripped); the
/// whole field when there is no name.
fn sender_name(from: &str) -> &str {
    let name = angle_address(from)
        .map(|(before, _)| {
            let before = before.trim();
            let before = before.strip_prefix('"').unwrap_or(before);
            let before = before.trim_end();
            before.strip_suffix('"').unwrap_or(before).trim()
        })
        .unwrap_or("");
    if name.is_empty() {
        from.trim()
    } else {
        name
    }
}

/// `(text before, address)` when the field ends in `<address>`.
fn angle_address(field: &str) -> Option<(&str, &str)> {
    let inner = field.trim_end().strip_suffix('>')?;
    let open = inner.rfind('<')?;
    let address = &inner[open + 1..];
    (!address.contains('>')).then_some((&field[..open], address))
}

/// The account's alias the message was addressed to, if any — a reply
/// should leave from the same address the mail arrived on.
fn matching_alias<'a>(message: &MessageHeader, aliases: &'a [Alias]) -> Option<&'a Alias> {
    let recipients: HashSet<String> = recipients::split(&message.to)
        .into_iter()
        .chain(recipients::split(&message.cc))
        .map(|entry| sender_address(&entry).to_lowercase())
        .collect();
    aliases.iter().find(|alias| {
        alias.account_id == message.account_id && recipients.contains(&alias.email.to_lowercase())
    })
}

/// `subject` under `prefix` ("Re:", "Fwd:"), unless it already carries it.
fn prefixed(subject: &str, prefix: &str) -> String {
    let trimmed = subject.trim();
    let carries = trimmed
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix));
    if carries {
        trimmed.to_string()
    } else {
        format!("{prefix} {trimmed}")
    }
}

fn reply_subject(subject: &str) -> String {
    prefixed(subject, "Re:")
}

fn forward_subject(subject: &str) -> String {
    prefixed(subject, "Fwd:")
}

/// The quote of a reply draft: material + the attribution line built for
/// it. None when the original had nothing quotable.
fn draft_quote(
    message: &MessageHeader,
    quote: Option<&MessageQuote>,
    date_label: &str,
) -> Option<DraftQuote> {
    let quote = quote.filter(|q| !(q.html.is_empty() && q.text.is_empty()))?;
    Some(DraftQuote {
        attribution: format!("On {date_label}, {} wrote:", sender_name(&message.from)),
        html: quote.html.clone(),
        text: quote.text.clone(),
    })
}

/// Threading identity a reply carries so recipients (and our own Sent copy)
/// group it into the conversation: answer the original's Message-ID and
/// extend its References chain. None when the original has no Message-ID —
/// there is nothing to thread on.
fn threading(message: &MessageHeader) -> (Option<String>, Option<String>) {
    if message.message_id.is_empty() {
        return (None, None);
    }
    let chain: Vec<&str> = message
        .references
        .split_whitespace()
        .chain(std::iter::once(message.message_id.as_str()))
        .collect();
    (Some(message.message_id.clone()), Some(chain.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATE: &str = "July 7, 2026 at 9:15 AM";

    fn message() -> MessageHeader {
        MessageHeader {
            id: 7,
            account_id: 2,
            mailbox: "INBOX".to_string(),
            from: "Alice Doe <alice@example.com>".to_string(),
            to: "Me <me@example.com>".to_string(),
            cc: String::new(),
            reply_to: String::new(),
            bcc: String::new(),
            subject: "Weekend plans".to_string(),
            snippet: "Are we still on?".to_string(),
            date: "2026-07-07T09:15:00Z".to_string(),
            read: true,
            has_attachments: false,
            message_id: String::new(),
            references: String::new(),
            is_draft: false,
            thread_count: 1,
            thread_unread: false,
            thread_has_draft: false,
        }
    }

    fn with(edit: impl FnOnce(&mut MessageHeader)) -> MessageHeader {
        let mut m = message();
        edit(&mut m);
        m
    }

    fn aliases() -> Vec<Alias> {
        let alias = |id, account_id, name: &str, email: &str| Alias {
            id,
            account_id,
            name: name.to_string(),
            email: email.to_string(),
        };
        vec![
            alias(10, 2, "Igor", "igor@vocalio.sk"),
            alias(11, 2, "", "dominik@vocalio.sk"),
            alias(12, 1, "", "other@example.com"),
        ]
    }

    fn quote(html: &str, text: &str) -> MessageQuote {
        MessageQuote {
            html: html.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn draft_kinds_arrive_under_the_names_types_ts_sends() {
        let kind = |wire: &str| serde_json::from_str::<DraftKind>(wire).unwrap();
        assert_eq!(kind("\"reply\""), DraftKind::Reply);
        assert_eq!(kind("\"reply-all\""), DraftKind::ReplyAll);
        assert_eq!(kind("\"forward\""), DraftKind::Forward);
    }

    // --- send-as alias matching ---

    #[test]
    fn a_reply_leaves_from_the_alias_the_mail_was_addressed_to() {
        let to_alias = with(|m| m.to = "Igor <igor@vocalio.sk>".to_string());
        assert_eq!(
            reply_draft(&to_alias, None, DATE, "", &aliases()).alias_id,
            Some(10)
        );
    }

    #[test]
    fn matches_an_alias_case_insensitively_and_in_the_cc_line() {
        let cc_alias = with(|m| m.cc = "DOMINIK@vocalio.sk".to_string());
        assert_eq!(
            reply_draft(&cc_alias, None, DATE, "", &aliases()).alias_id,
            Some(11)
        );
    }

    #[test]
    fn ignores_another_accounts_alias_with_the_same_address() {
        let to_foreign = with(|m| m.to = "other@example.com".to_string());
        assert_eq!(
            reply_draft(&to_foreign, None, DATE, "", &aliases()).alias_id,
            None
        );
    }

    #[test]
    fn leaves_the_alias_unset_when_the_mail_came_to_the_account_itself() {
        assert_eq!(
            reply_draft(&message(), None, DATE, "", &aliases()).alias_id,
            None
        );
    }

    #[test]
    fn reply_all_carries_the_matched_alias_too() {
        let to_alias = with(|m| m.to = "igor@vocalio.sk".to_string());
        assert_eq!(
            reply_all_draft(&to_alias, None, DATE, "me@example.com", &aliases()).alias_id,
            Some(10)
        );
    }

    #[test]
    fn reply_all_never_mails_the_accounts_own_aliases_back() {
        let to_both = with(|m| {
            m.to = "igor@vocalio.sk, Bob <bob@example.com>".to_string();
            m.cc = "dominik@vocalio.sk".to_string();
        });
        let draft = reply_all_draft(&to_both, None, DATE, "me@example.com", &aliases());

        assert_eq!(
            draft.to,
            "Alice Doe <alice@example.com>, Bob <bob@example.com>"
        );
        assert_eq!(draft.cc, "");
    }

    #[test]
    fn a_forward_keeps_the_identity_the_mail_was_addressed_to() {
        let to_alias = with(|m| m.to = "igor@vocalio.sk".to_string());
        assert_eq!(
            forward_draft(&to_alias, None, DATE, &aliases()).alias_id,
            Some(10)
        );
    }

    // --- reply ---

    #[test]
    fn replies_from_the_account_the_message_arrived_on() {
        assert_eq!(reply_draft(&message(), None, DATE, "", &[]).account_id, 2);
    }

    #[test]
    fn addresses_the_senders_bare_address() {
        assert_eq!(
            reply_draft(&message(), None, DATE, "", &[]).to,
            "alice@example.com"
        );
    }

    #[test]
    fn keeps_a_plain_from_field_as_the_address() {
        let plain = with(|m| m.from = "alice@example.com".to_string());
        assert_eq!(
            reply_draft(&plain, None, DATE, "", &[]).to,
            "alice@example.com"
        );
    }

    #[test]
    fn prefixes_the_subject_with_re() {
        assert_eq!(
            reply_draft(&message(), None, DATE, "", &[]).subject,
            "Re: Weekend plans"
        );
    }

    #[test]
    fn does_not_stack_re_prefixes() {
        let re = with(|m| m.subject = "RE: Weekend plans".to_string());
        assert_eq!(
            reply_draft(&re, None, DATE, "", &[]).subject,
            "RE: Weekend plans"
        );
    }

    #[test]
    fn opens_with_an_empty_body_and_carries_the_quote_separately() {
        let original = quote("<p>First line</p>", "First line");
        let draft = reply_draft(&message(), Some(&original), DATE, "", &[]);

        assert_eq!(draft.body, "");
        let quote = draft.quote.expect("a quote");
        assert_eq!(
            quote.attribution,
            "On July 7, 2026 at 9:15 AM, Alice Doe wrote:"
        );
        assert_eq!(quote.html, "<p>First line</p>");
        assert_eq!(quote.text, "First line");
    }

    #[test]
    fn carries_no_quote_when_the_original_has_no_content() {
        assert!(reply_draft(&message(), None, DATE, "", &[]).quote.is_none());
        let empty = quote("", "");
        assert!(reply_draft(&message(), Some(&empty), DATE, "", &[])
            .quote
            .is_none());
    }

    #[test]
    fn the_attribution_names_a_quoted_display_name_without_its_quotes() {
        let quoted = with(|m| m.from = "\"Novák, Ján\" <jan@example.com>".to_string());
        let original = quote("<p>x</p>", "x");
        let draft = reply_draft(&quoted, Some(&original), DATE, "", &[]);
        assert_eq!(
            draft.quote.expect("a quote").attribution,
            "On July 7, 2026 at 9:15 AM, Novák, Ján wrote:"
        );
    }

    #[test]
    fn stamps_the_reply_with_the_originals_threading_identity() {
        let threaded = with(|m| {
            m.message_id = "mid@x".to_string();
            m.references = "root@x".to_string();
        });
        let draft = reply_draft(&threaded, None, DATE, "", &[]);

        assert_eq!(draft.in_reply_to.as_deref(), Some("mid@x"));
        assert_eq!(draft.references.as_deref(), Some("root@x mid@x"));
    }

    #[test]
    fn starts_the_chain_at_the_original_when_it_had_no_references() {
        let first = with(|m| m.message_id = "mid@x".to_string());
        assert_eq!(
            reply_draft(&first, None, DATE, "", &[])
                .references
                .as_deref(),
            Some("mid@x")
        );
    }

    #[test]
    fn omits_threading_when_the_original_has_no_message_id() {
        let draft = reply_draft(&message(), None, DATE, "", &[]);
        assert_eq!(draft.in_reply_to, None);
        assert_eq!(draft.references, None);
    }

    // --- reply to own message: account 2's address is hello@vocalio.sk ---

    fn own() -> MessageHeader {
        with(|m| {
            m.from = "Me <hello@vocalio.sk>".to_string();
            m.to = "Alice Doe <alice@example.com>, bob@example.com".to_string();
        })
    }

    #[test]
    fn addresses_the_original_recipients_instead_of_myself() {
        let draft = reply_draft(&own(), None, DATE, "hello@vocalio.sk", &[]);
        assert_eq!(draft.to, "Alice Doe <alice@example.com>, bob@example.com");
    }

    #[test]
    fn compares_my_address_case_insensitively() {
        let draft = reply_draft(&own(), None, DATE, "HELLO@vocalio.sk", &[]);
        assert_eq!(draft.to, "Alice Doe <alice@example.com>, bob@example.com");
    }

    #[test]
    fn treats_mail_sent_from_my_alias_as_my_own_and_keeps_that_identity() {
        let mut from_alias = own();
        from_alias.from = "Igor <igor@vocalio.sk>".to_string();
        let draft = reply_draft(&from_alias, None, DATE, "hello@vocalio.sk", &aliases());
        assert_eq!(draft.to, "Alice Doe <alice@example.com>, bob@example.com");
        assert_eq!(draft.alias_id, Some(10));
    }

    #[test]
    fn ignores_another_accounts_alias_when_judging_ownership() {
        let mut foreign = own();
        foreign.from = "other@example.com".to_string();
        let draft = reply_draft(&foreign, None, DATE, "hello@vocalio.sk", &aliases());
        assert_eq!(draft.to, "other@example.com");
    }

    #[test]
    fn falls_back_to_the_sender_when_my_message_had_no_recipients() {
        let mut no_to = own();
        no_to.to = String::new();
        let draft = reply_draft(&no_to, None, DATE, "hello@vocalio.sk", &[]);
        assert_eq!(draft.to, "hello@vocalio.sk");
    }

    // --- reply-all ---

    fn group() -> MessageHeader {
        with(|m| {
            m.to = "Me <me@example.com>, Bob <bob@example.com>".to_string();
            m.cc = "carol@example.com, Me <me@example.com>".to_string();
        })
    }

    #[test]
    fn addresses_the_sender_plus_the_other_to_recipients_minus_me() {
        let draft = reply_all_draft(&group(), None, DATE, "me@example.com", &[]);
        assert_eq!(
            draft.to,
            "Alice Doe <alice@example.com>, Bob <bob@example.com>"
        );
    }

    #[test]
    fn keeps_the_surviving_cc_recipients_on_the_cc_line() {
        let draft = reply_all_draft(&group(), None, DATE, "me@example.com", &[]);
        assert_eq!(draft.cc, "carol@example.com");
    }

    #[test]
    fn reply_all_compares_addresses_case_insensitively() {
        let draft = reply_all_draft(&group(), None, DATE, "ME@Example.com", &[]);
        assert!(!draft.to.contains("me@example.com"), "{}", draft.to);
        assert_eq!(draft.cc, "carol@example.com");
    }

    #[test]
    fn drops_recipients_that_repeat_across_to_and_cc() {
        let mut dup = group();
        dup.cc = "Bob <bob@example.com>".to_string();
        let draft = reply_all_draft(&dup, None, DATE, "me@example.com", &[]);

        assert_eq!(
            draft.to,
            "Alice Doe <alice@example.com>, Bob <bob@example.com>"
        );
        assert_eq!(draft.cc, "");
    }

    #[test]
    fn keeps_display_names_whose_comma_would_split_naively() {
        let mut comma = group();
        comma.to = "\"Novák, Ján\" <jan@example.com>".to_string();
        assert_eq!(
            reply_all_draft(&comma, None, DATE, "me@example.com", &[]).to,
            "Alice Doe <alice@example.com>, \"Novák, Ján\" <jan@example.com>"
        );
    }

    #[test]
    fn falls_back_to_the_sender_when_everyone_else_was_me() {
        let solo = with(|m| {
            m.from = "Me <me@example.com>".to_string();
            m.to = "me@example.com".to_string();
            m.cc = String::new();
        });
        assert_eq!(
            reply_all_draft(&solo, None, DATE, "me@example.com", &[]).to,
            "me@example.com"
        );
    }

    #[test]
    fn reply_all_prefixes_the_subject_with_re_and_quotes_like_a_plain_reply() {
        let original = quote("<p>Hello</p>", "Hello");
        let draft = reply_all_draft(&group(), Some(&original), DATE, "me@example.com", &[]);

        assert_eq!(draft.subject, "Re: Weekend plans");
        assert_eq!(draft.quote.expect("a quote").text, "Hello");
        assert_eq!(draft.account_id, 2);
    }

    #[test]
    fn stamps_reply_all_with_the_originals_threading_identity() {
        let mut threaded = group();
        threaded.message_id = "mid@x".to_string();
        threaded.references = "root@x".to_string();
        let draft = reply_all_draft(&threaded, None, DATE, "me@example.com", &[]);

        assert_eq!(draft.in_reply_to.as_deref(), Some("mid@x"));
        assert_eq!(draft.references.as_deref(), Some("root@x mid@x"));
    }

    // --- forward ---

    #[test]
    fn leaves_the_recipient_empty_and_prefixes_the_subject_with_fwd() {
        let draft = forward_draft(&message(), None, DATE, &[]);

        assert_eq!(draft.to, "");
        assert_eq!(draft.subject, "Fwd: Weekend plans");
        assert_eq!(draft.account_id, 2);
    }

    #[test]
    fn does_not_stack_fwd_prefixes() {
        let fwd = with(|m| m.subject = "FWD: Weekend plans".to_string());
        assert_eq!(
            forward_draft(&fwd, None, DATE, &[]).subject,
            "FWD: Weekend plans"
        );
    }

    #[test]
    fn embeds_the_original_headers_and_text_in_a_forwarded_block() {
        let body = forward_draft(&message(), Some("Original text"), DATE, &[]).body;

        assert_eq!(
            body,
            "\n\n---------- Forwarded message ----------\n\
             From: Alice Doe <alice@example.com>\n\
             Date: July 7, 2026 at 9:15 AM\n\
             Subject: Weekend plans\n\
             To: Me <me@example.com>\n\
             \n\
             Original text\n"
        );
    }

    #[test]
    fn names_the_cc_recipients_when_the_original_had_any() {
        let cc = with(|m| m.cc = "carol@example.com".to_string());
        assert!(forward_draft(&cc, Some("x"), DATE, &[])
            .body
            .contains("Cc: carol@example.com"));
    }

    #[test]
    fn still_produces_the_block_when_the_original_has_no_text() {
        let body = forward_draft(&message(), None, DATE, &[]).body;

        assert!(body.contains("---------- Forwarded message ----------"));
        assert!(body.ends_with("To: Me <me@example.com>\n"), "{body:?}");
    }

    #[test]
    fn starts_a_fresh_thread_no_threading_headers() {
        let threaded = with(|m| {
            m.message_id = "mid@x".to_string();
            m.references = "root@x".to_string();
        });
        let draft = forward_draft(&threaded, None, DATE, &[]);

        assert_eq!(draft.in_reply_to, None);
        assert_eq!(draft.references, None);
    }
}
