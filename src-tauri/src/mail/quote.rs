//! Quoted-history detection. Replies carry the whole previous exchange
//! below an attribution line ("On … wrote:", "Dňa … napísal:") or a run of
//! "> " lines — in a conversation view that reads as every message repeating
//! the one before it. The viewer folds that tail away, Gmail-style.

/// Split plain text into (own content, quoted history). The text stays
/// whole when no boundary is found, when nothing precedes the quote (a
/// fully-quoted body is better shown than hidden), or when the quote is
/// empty.
pub fn split_text_quote(text: &str) -> (String, Option<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let Some(boundary) = quote_boundary(&lines) else {
        return (text.to_string(), None);
    };
    let main = lines[..boundary].join("\n").trim_end().to_string();
    let quoted = lines[boundary..].join("\n").trim().to_string();
    if main.is_empty() || quoted.is_empty() {
        return (text.to_string(), None);
    }
    (main, Some(quoted))
}

/// First line index where the quoted history starts, `None` when the text
/// has none.
fn quote_boundary(lines: &[&str]) -> Option<usize> {
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if is_attribution(trimmed) || is_outlook_divider(trimmed) {
            return Some(index);
        }
        // A ">" line counts only when the rest of the text is mostly quoted
        // too — an interleaved (inline) reply keeps its own text visible.
        if trimmed.starts_with('>') && mostly_quoted(&lines[index..]) {
            return Some(index);
        }
    }
    None
}

/// The "On …, X wrote:" / "Dňa … napísal(a):" line most clients put above
/// the quote. Localised forms vary endlessly; matched loosely on purpose.
fn is_attribution(line: &str) -> bool {
    if !line.ends_with(':') || line.len() > 200 {
        return false;
    }
    let lower = line.to_lowercase();
    (lower.starts_with("on ") && lower.contains(" wrote"))
        || lower.contains("napísal")
        || lower.contains("napisal")
        || lower.ends_with("píše:")
        || lower.ends_with("pise:")
}

/// Outlook-style reply dividers: a long underscore rule or the literal
/// "-----Original Message-----".
fn is_outlook_divider(line: &str) -> bool {
    (line.len() >= 20 && line.chars().all(|c| c == '_'))
        || line.starts_with("-----Original Message-----")
        || line.starts_with("-----Pôvodná správa-----")
}

/// At least three quarters of the non-empty lines start with ">".
fn mostly_quoted(lines: &[&str]) -> bool {
    let content: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if content.is_empty() {
        return false;
    }
    let quoted = content.iter().filter(|l| l.trim().starts_with('>')).count();
    quoted * 4 >= content.len() * 3
}

/// Fold the trailing quoted history of a *sanitized* HTML fragment into a
/// `<details>` block the reader expands with a click — plain HTML, no
/// scripts, so the render sandbox is untouched. The wrap starts at an
/// element boundary (a `<blockquote>` or the paragraph carrying the first
/// "&gt;" quote line), so the markup stays well-formed.
pub fn fold_html_quote(html: String) -> String {
    let Some(at) = html_quote_start(&html) else {
        return html;
    };
    // A body that IS the quote (nothing visible above) stays unfolded.
    if visible_text_len(&html[..at]) == 0 {
        return html;
    }
    format!(
        "{}<details class=\"flit-quote\">\
         <summary title=\"Show quoted text\">•••</summary>{}</details>",
        &html[..at],
        &html[at..]
    )
}

/// Earliest safe fold point: the first `<blockquote>`, or the opening tag
/// of the element whose text begins with the "&gt;" quote prefix.
fn html_quote_start(html: &str) -> Option<usize> {
    let blockquote = html.find("<blockquote");
    // ">&gt;" = an opening tag closing right before a quote marker
    // (`<p>&gt; …`, `<br>&gt; …`); back up to that element's "<".
    let quote_para = html.find(">&gt;").and_then(|gt| html[..gt].rfind('<'));
    match (blockquote, quote_para) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Cut the trailing quoted history out of a *sanitized* HTML fragment —
/// used instead of folding when the quote provably repeats an earlier
/// message of the same conversation (shown one card up).
pub fn strip_html_quote(html: String) -> String {
    match html_quote_start(&html) {
        Some(at) if visible_text_len(&html[..at]) > 0 => html[..at].to_string(),
        _ => html,
    }
}

/// Whether `quoted` (the split-off history of a reply) repeats one of the
/// `earlier` message bodies. Compared in a normalized form — quote markers
/// and attribution lines stripped, whitespace collapsed — so re-wrapped
/// lines and nesting levels still match; containment covers selective
/// quoting. An edited quote no longer matches and stays visible.
pub fn quote_matches_history(quoted: &str, earlier: &[String]) -> bool {
    let needle = normalize(quoted);
    if needle.is_empty() {
        return false;
    }
    earlier.iter().any(|body| normalize(body).contains(&needle))
}

/// The comparison form of a text: per line, ">" markers (any nesting) and
/// attribution/divider lines dropped; then all whitespace collapsed. Both
/// sides of the match go through this, so a body that itself quotes an
/// older mail compares equal to its re-quoted (deeper-nested) form.
fn normalize(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(|line| {
            let mut rest = line.trim_start();
            while let Some(stripped) = rest.strip_prefix('>') {
                rest = stripped.trim_start();
            }
            rest
        })
        .filter(|line| !is_attribution(line) && !is_outlook_divider(line))
        .collect();
    lines
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rough count of the characters a reader would see: everything outside
/// tags, entities counted as one.
fn visible_text_len(html: &str) -> usize {
    let mut inside_tag = false;
    let mut count = 0;
    for c in html.chars() {
        match c {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            c if !inside_tag && !c.is_whitespace() => count += 1,
            _ => {}
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_without_a_quote_stays_whole() {
        let text = "Ahoj,\nposielam ti odkaz.\n\nS pozdravom";
        assert_eq!(split_text_quote(text), (text.to_string(), None));
    }

    #[test]
    fn splits_at_the_attribution_line() {
        let text = "Uhradené. Ďakujem.\n\n\
                    On Monday, Nov 10, 2025 at 3:17 PM, Tomáš Gaal <t@x.sk> wrote:\n\
                    > Dobrý deň,\n> posielam platobné údaje.";

        let (main, quoted) = split_text_quote(text);

        assert_eq!(main, "Uhradené. Ďakujem.");
        assert!(quoted.unwrap().starts_with("On Monday"));
    }

    #[test]
    fn splits_at_slovak_gmail_attributions() {
        let text = "Áno, môžete.\n\n\
                    Dňa ut 14. 4. 2026, 11:21 používateľ Dominik Mery napísal:\n\
                    > pôvodný text";

        let (main, quoted) = split_text_quote(text);

        assert_eq!(main, "Áno, môžete.");
        assert!(quoted.is_some());
    }

    #[test]
    fn splits_at_a_bare_quote_run_without_attribution() {
        let text = "Ytm*-nsthV\n\n> https://share.example/s\n> druhá citovaná";

        let (main, quoted) = split_text_quote(text);

        assert_eq!(main, "Ytm*-nsthV");
        assert_eq!(
            quoted.unwrap(),
            "> https://share.example/s\n> druhá citovaná"
        );
    }

    #[test]
    fn splits_at_the_outlook_divider() {
        let text = "Dobrý deň, v poriadku.\n\n\
                    ________________________________\n\
                    From: X\nSent: Monday\nSubject: Re: Hi\n\npôvodný text";

        let (main, quoted) = split_text_quote(text);

        assert_eq!(main, "Dobrý deň, v poriadku.");
        assert!(quoted.unwrap().starts_with("____"));
    }

    #[test]
    fn an_inline_reply_keeps_its_interleaved_text() {
        // Half the lines are the author's own answers between quotes —
        // hiding from the first ">" would hide their text too.
        let text = "> otázka jedna?\nodpoveď jedna\n> otázka dva?\nodpoveď dva\n\
                    ani toto nie je citát\na ani toto\nnaozaj nie\nvôbec nie";
        assert_eq!(split_text_quote(text).1, None);
    }

    #[test]
    fn a_fully_quoted_body_stays_whole() {
        let text = "> všetko\n> je\n> citát";
        assert_eq!(split_text_quote(text), (text.to_string(), None));
    }

    #[test]
    fn quote_matching_survives_markers_wrapping_and_nesting() {
        let earlier = vec![
            "Ahoj, posielam ti projekt bridgeai v takom stave ako si ho prezentoval.\n\
             Sú tam aj exporty db z fmuk verzie."
                .to_string(),
        ];
        // The reply re-wraps lines differently and prefixes them with "> ".
        let quoted = "On Wednesday, Mar 18, 2026, Dominik wrote:\n\
                      > Ahoj, posielam ti projekt bridgeai v takom\n\
                      > stave ako si ho prezentoval. Sú tam aj exporty db z fmuk verzie.";
        assert!(quote_matches_history(quoted, &earlier));

        // Selective quoting (only part of the body) still matches…
        let partial = "> Sú tam aj exporty db z fmuk verzie.";
        assert!(quote_matches_history(partial, &earlier));

        // …an edited quote does not.
        let edited = "> Ahoj, posielam ti projekt TOTO SOM DOPISAL v takom stave.";
        assert!(!quote_matches_history(edited, &earlier));

        // A quote of a mail we never cached matches nothing.
        assert!(!quote_matches_history("> niečo úplne iné", &[]));
    }

    #[test]
    fn nested_quotes_match_the_body_that_already_carried_them() {
        // B's body itself ends with a quote of A; C quotes B, so every
        // line gains one more ">" level. Normalization equalizes both.
        let b_body = "Uhradené. Ďakujem.\n\n> Dobrý deň,\n> posielam údaje.".to_string();
        let c_quote =
            "On Monday, X wrote:\n> Uhradené. Ďakujem.\n>\n>> Dobrý deň,\n>> posielam údaje.";
        assert!(quote_matches_history(c_quote, &[b_body]));
    }

    #[test]
    fn strip_html_quote_cuts_the_tail_but_never_blanks_the_body() {
        let html = "<p>Moja odpoveď.</p><blockquote><p>stará správa</p></blockquote>".to_string();
        assert_eq!(strip_html_quote(html), "<p>Moja odpoveď.</p>");

        let all_quote = "<blockquote><p>iba citát</p></blockquote>".to_string();
        assert_eq!(strip_html_quote(all_quote.clone()), all_quote);
    }

    #[test]
    fn folds_html_from_the_first_blockquote() {
        let html = "<p>Ahoj, posielam odkaz.</p>\
                    <blockquote><p>pôvodná správa</p></blockquote>"
            .to_string();

        let folded = fold_html_quote(html);

        assert!(folded.starts_with("<p>Ahoj, posielam odkaz.</p><details class=\"flit-quote\">"));
        assert!(folded.contains("<summary title=\"Show quoted text\">•••</summary>"));
        assert!(folded.ends_with("</blockquote></details>"));
    }

    #[test]
    fn folds_html_from_the_paragraph_carrying_the_quote_prefix() {
        let html = "<p>Moja odpoveď.</p><p>&gt; citovaný riadok</p>".to_string();

        let folded = fold_html_quote(html);

        assert!(folded.starts_with("<p>Moja odpoveď.</p><details"));
        assert!(folded.contains("<p>&gt; citovaný riadok</p></details>"));
    }

    #[test]
    fn html_without_quotes_or_without_own_content_stays_whole() {
        let plain = "<p>len text</p>".to_string();
        assert_eq!(fold_html_quote(plain.clone()), plain);

        // The whole body is one big quote — folding would blank the card.
        let all_quote = "<blockquote><p>iba citát</p></blockquote>".to_string();
        assert_eq!(fold_html_quote(all_quote.clone()), all_quote);
    }
}
