//! Quoted-history detection. Replies carry the whole previous exchange
//! below an attribution line ("On … wrote:", "Dňa … napísal:") or a run of
//! "> " lines — in a conversation view that reads as every message repeating
//! the one before it. The viewer folds that tail away, Gmail-style.

use crate::mail::parse::{strip_invisible, InlineImage};
use crate::mail::sanitize::sanitize_fragment;
use crate::models::MessageQuote;

/// Everything a reply needs to quote a message: the sanitized HTML fragment
/// (plain-text mail upconverted to blockquote markup) plus the cleaned text
/// feeding the outgoing "> " fallback part. Invisible preheader padding is
/// stripped from both — it otherwise rides invisibly into the sent mail —
/// and the text's blank-line runs collapse to one (ESP text parts pad
/// with dozens, which became "> \n> \n> …" spam in the fallback).
pub fn quote_material(
    html: Option<&str>,
    text: Option<&str>,
    images: &[InlineImage],
) -> MessageQuote {
    let stripped = strip_invisible(text.unwrap_or_default());
    let text = collapse_blank_runs(&stripped);
    let html = match html {
        Some(h) => strip_invisible(&sanitize_fragment(h, images)),
        None => text_to_quote_html(&text),
    };
    MessageQuote { html, text }
}

/// Lines trimmed at the right edge, runs of blank lines squeezed to a
/// single one, blank edges dropped.
fn collapse_blank_runs(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut blank_pending = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blank_pending = true;
            continue;
        }
        if blank_pending && !lines.is_empty() {
            lines.push("");
        }
        blank_pending = false;
        lines.push(line);
    }
    lines.join("\n")
}

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
/// of the element whose text begins with the "&gt;" quote prefix — then
/// backed up over the attribution line ("On …, X wrote:") right above it,
/// which belongs to the quote the way the text path already treats it.
fn html_quote_start(html: &str) -> Option<usize> {
    let blockquote = html.find("<blockquote");
    // ">&gt;" = an opening tag closing right before a quote marker
    // (`<p>&gt; …`, `<br>&gt; …`); back up to that element's "<".
    let quote_para = html.find(">&gt;").and_then(|gt| html[..gt].rfind('<'));
    let at = match (blockquote, quote_para) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }?;
    let start = attribution_start(&html[..at]).unwrap_or(at);
    Some(wrapper_start(html, start, at).unwrap_or(start))
}

/// Start of the attribution element sitting directly above the quote (only
/// whitespace between the two), `None` when there is none. The returned
/// index is the "<" of its opening tag, so cutting there keeps the markup
/// well-formed.
fn attribution_start(before: &str) -> Option<usize> {
    let before = before.trim_end();
    // The element must be closed — `…</p>` / `…</div>` — for us to know
    // where it began.
    let close_lt = before.rfind('<')?;
    let name = before[close_lt..]
        .strip_prefix("</")?
        .strip_suffix('>')?
        .trim()
        .to_ascii_lowercase();
    let open_lt = last_opening_tag(&before[..close_lt], &name)?;
    let open_end = before[open_lt..].find('>')? + open_lt;
    if open_end >= close_lt {
        return None;
    }
    // why: attribution lines carry text and <br> only. A nested element of
    // the same name means `open_lt` is an inner tag, not the one this
    // closing tag belongs to — bail rather than cut mid-element.
    let inner = &before[open_end + 1..close_lt];
    if inner.contains(&format!("</{name}")) {
        return None;
    }
    // Gmail (and our own drafts) label the line explicitly; no need to
    // parse the class list for a token this specific.
    if before[open_lt..=open_end].contains("gmail_attr") {
        return Some(open_lt);
    }
    // `is_attribution` wants the bare line ending in ':', so drop tags
    // (a trailing <br> is common) and decode the few entities that can
    // show up in a name or address.
    if matches!(name.as_str(), "p" | "div") && is_attribution(&visible_text(inner)) {
        return Some(open_lt);
    }
    None
}

/// When the attribution and the quote are alone inside a wrapper `<div>`
/// (our own drafts and Gmail use `<div class="gmail_quote">…</div>`), the
/// fold takes the wrapper too — otherwise its `</div>` would be stranded
/// inside the `<details>` with its opening tag left outside.
fn wrapper_start(html: &str, attribution: usize, quote: usize) -> Option<usize> {
    let before = html[..attribution].trim_end();
    let open_lt = before.rfind('<')?;
    let tag = &before[open_lt..];
    // The wrapper's opening tag has to be the last thing before the
    // attribution, and it has to be a plain `<div …>`.
    let rest = tag.strip_suffix('>')?.strip_prefix("<div")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    // …and it has to close at the very end, with exactly one `</div>` in
    // the quote that nothing there opened — that one is the wrapper's.
    let quoted = html[quote..].trim_end();
    if !quoted.ends_with("</div>") {
        return None;
    }
    // "</div>" does not contain "<div", so these two counts don't overlap.
    let opened = quoted.matches("<div").count();
    let closed = quoted.matches("</div>").count();
    (closed == opened + 1).then_some(open_lt)
}

/// Last opening tag of `name` in `html` (`<p`, `<div …`), ignoring closing
/// tags and longer names that merely start the same way.
fn last_opening_tag(html: &str, name: &str) -> Option<usize> {
    // ASCII-lowercasing never changes byte lengths, so indexes still line
    // up with the original.
    let lower = html.to_ascii_lowercase();
    let needle = format!("<{name}");
    let mut found = None;
    let mut from = 0;
    while let Some(offset) = lower[from..].find(&needle) {
        let at = from + offset;
        let after = lower[at + needle.len()..].chars().next();
        if matches!(after, Some(c) if c == '>' || c == '/' || c.is_whitespace()) {
            found = Some(at);
        }
        from = at + 1;
    }
    found
}

/// The text a reader would see in a fragment: tags dropped, the handful of
/// entities that reach an attribution line decoded, edges trimmed.
fn visible_text(html: &str) -> String {
    let mut text = String::new();
    let mut inside_tag = false;
    for c in html.chars() {
        match c {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            c if !inside_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        // why: "&amp;" last, so "&amp;lt;" decodes to "&lt;", not "<".
        .replace("&amp;", "&")
        .trim()
        .to_string()
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

/// Plain text — possibly carrying its own "> " quoted history — rendered as
/// HTML for a reply's quoted block: one <p> per line, "> " runs nested as
/// real <blockquote>s, so a text mail gets the same bar-styled look as an
/// HTML one. Output is built from escaped text only, safe to embed.
pub fn text_to_quote_html(text: &str) -> String {
    let mut html = String::new();
    let mut open = 0usize;
    for line in text.lines() {
        let (depth, content) = quote_markers(line);
        // Blank lines add nothing between per-line <p>s — and skipping
        // BEFORE the depth adjustment keeps a blank from splitting one
        // quote run into two blockquotes.
        if content.is_empty() {
            continue;
        }
        while open < depth {
            html.push_str("<blockquote>");
            open += 1;
        }
        while open > depth {
            html.push_str("</blockquote>");
            open -= 1;
        }
        html.push_str("<p>");
        html.push_str(&escape_html(content));
        html.push_str("</p>");
    }
    html.push_str(&"</blockquote>".repeat(open));
    html
}

/// Leading ">" markers of a quoted line: nesting depth plus the content
/// after them ("> > x" and ">>x" both parse as depth 2, "x").
fn quote_markers(line: &str) -> (usize, &str) {
    let mut depth = 0;
    let mut rest = line;
    loop {
        let trimmed = rest.trim_start();
        match trimmed.strip_prefix('>') {
            Some(after) => {
                depth += 1;
                rest = after;
            }
            None => return (depth, trimmed),
        }
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
    fn quote_material_sanitizes_html_and_cleans_text() {
        let quote = quote_material(
            Some("<p>Novinky\u{200D}\u{034F}</p><script>alert(1)</script>"),
            Some("Novinky\u{200D}\u{034F}\n\u{034F} \u{200C}"),
            &[],
        );

        assert_eq!(quote.html, "<p>Novinky</p>");
        assert_eq!(quote.text, "Novinky");
    }

    #[test]
    fn quote_material_collapses_blank_line_runs() {
        // The shape of an ESP text part: content islands separated by
        // 10+ blank padding lines (some holding only spaces).
        let text = "The Final kicks off tonight.\n\n\n   \n\n\n\nHey Dominik,\n  \n\nWatch the game.\n\n\n";

        let quote = quote_material(None, Some(text), &[]);

        assert_eq!(
            quote.text,
            "The Final kicks off tonight.\n\nHey Dominik,\n\nWatch the game."
        );
    }

    #[test]
    fn quote_material_upconverts_text_only_mail() {
        let quote = quote_material(None, Some("Ahoj,\n> starý riadok"), &[]);

        assert_eq!(
            quote.html,
            "<p>Ahoj,</p><blockquote><p>starý riadok</p></blockquote>"
        );
        assert_eq!(quote.text, "Ahoj,\n> starý riadok");
    }

    #[test]
    fn text_becomes_paragraphs_without_quote_markers() {
        let html = text_to_quote_html("Ahoj,\n\nposielam <b>odkaz</b> & fakturu.");

        assert_eq!(
            html,
            "<p>Ahoj,</p><p>posielam &lt;b&gt;odkaz&lt;/b&gt; &amp; fakturu.</p>"
        );
    }

    #[test]
    fn quote_runs_become_nested_blockquotes() {
        let text = "odpoveď\n> prvý citát\n> > starší citát\n>>ešte starší\n> späť";

        assert_eq!(
            text_to_quote_html(text),
            "<p>odpoveď</p>\
             <blockquote><p>prvý citát</p>\
             <blockquote><p>starší citát</p><p>ešte starší</p></blockquote>\
             <p>späť</p></blockquote>"
        );
    }

    #[test]
    fn blank_lines_do_not_split_a_quote_run() {
        let text = "> jedna\n\n> dva";

        assert_eq!(
            text_to_quote_html(text),
            "<blockquote><p>jedna</p><p>dva</p></blockquote>"
        );
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

    #[test]
    fn folds_our_own_sent_quote_including_the_attribution() {
        // The exact shape `src/lib/draft.ts` builds for a reply.
        let html = "<p>Vďaka, pozriem sa na to.</p>\
                    <br><div class=\"gmail_quote flit-draft-quote\">\
                    <div class=\"gmail_attr\">On 13.09.2026 11:27, Jakub Šarvaic wrote:</div>\
                    <blockquote class=\"gmail_quote\" type=\"cite\"><p>pôvodná správa</p></blockquote>\
                    </div>"
            .to_string();

        let folded = fold_html_quote(html);

        // Own text stays above the fold…
        assert!(
            folded.starts_with("<p>Vďaka, pozriem sa na to.</p><br><details class=\"flit-quote\">")
        );
        // …the attribution goes under it, wrapper and all.
        assert!(folded.contains(
            "</summary><div class=\"gmail_quote flit-draft-quote\">\
             <div class=\"gmail_attr\">On 13.09.2026 11:27, Jakub Šarvaic wrote:</div>"
        ));
        assert!(folded.ends_with("</div></details>"));
    }

    #[test]
    fn folds_a_gmail_web_reply_including_the_attribution() {
        // Gmail's own reply markup: dir attribute, trailing <br>, wrapper div.
        let html = "<div dir=\"ltr\">Own text</div><br>\
                    <div class=\"gmail_quote gmail_quote_container\">\
                    <div dir=\"ltr\" class=\"gmail_attr\">On Sat, 13 Sept 2026 at 11:27, Jakub wrote:<br></div>\
                    <blockquote class=\"gmail_quote\"><div dir=\"ltr\">q</div></blockquote></div>"
            .to_string();

        let folded = fold_html_quote(html);

        assert!(
            folded.starts_with("<div dir=\"ltr\">Own text</div><br><details class=\"flit-quote\">")
        );
        assert!(folded.contains("</summary><div class=\"gmail_quote gmail_quote_container\">"));
        assert!(folded.ends_with("</blockquote></div></details>"));
    }

    #[test]
    fn folds_a_plain_paragraph_attribution_with_the_quote() {
        let html = "<p>Own text</p><p>On Jul 24, 2026, Peter wrote:</p>\
                    <blockquote><p>q</p></blockquote>"
            .to_string();

        let folded = fold_html_quote(html);

        assert!(folded.starts_with("<p>Own text</p><details class=\"flit-quote\">"));
        assert!(folded.contains("</summary><p>On Jul 24, 2026, Peter wrote:</p><blockquote>"));
    }

    #[test]
    fn folds_an_apple_mail_attribution_div_with_the_quote() {
        let html = "<p>Own text</p><div>On 13 Sep 2026, at 11:27, Jakub wrote:<br></div>\
                    <blockquote type=\"cite\"><p>q</p></blockquote>"
            .to_string();

        let folded = fold_html_quote(html);

        assert!(folded.starts_with("<p>Own text</p><details class=\"flit-quote\">"));
        assert!(folded.contains("</summary><div>On 13 Sep 2026, at 11:27, Jakub wrote:<br></div>"));
    }

    #[test]
    fn folds_a_slovak_attribution_with_the_quote() {
        let html = "<p>Dobre, súhlasím.</p><p>Dňa 13. 9. 2026 o 11:27 Jakub napísal(a):</p>\
                    <blockquote><p>q</p></blockquote>"
            .to_string();

        let folded = fold_html_quote(html);

        assert!(folded.starts_with("<p>Dobre, súhlasím.</p><details class=\"flit-quote\">"));
        assert!(folded.contains("</summary><p>Dňa 13. 9. 2026 o 11:27 Jakub napísal(a):</p>"));
    }

    #[test]
    fn a_colon_line_that_is_not_an_attribution_stays_visible() {
        let html = "<p>Own text</p><p>See below:</p><blockquote><p>q</p></blockquote>".to_string();

        let folded = fold_html_quote(html);

        assert!(
            folded.starts_with("<p>Own text</p><p>See below:</p><details class=\"flit-quote\">")
        );
    }

    #[test]
    fn an_attribution_not_touching_the_quote_stays_visible() {
        let html = "<p>On Jul 24, 2026, Peter wrote:</p><p>middle</p>\
                    <blockquote><p>q</p></blockquote>"
            .to_string();

        let folded = fold_html_quote(html);

        assert!(folded.starts_with(
            "<p>On Jul 24, 2026, Peter wrote:</p><p>middle</p><details class=\"flit-quote\">"
        ));
    }

    #[test]
    fn strip_html_quote_cuts_the_attribution_too() {
        let html = "<p>Vďaka, pozriem sa na to.</p>\
                    <br><div class=\"gmail_quote flit-draft-quote\">\
                    <div class=\"gmail_attr\">On 13.09.2026 11:27, Jakub Šarvaic wrote:</div>\
                    <blockquote class=\"gmail_quote\" type=\"cite\"><p>pôvodná správa</p></blockquote>\
                    </div>"
            .to_string();

        assert_eq!(
            strip_html_quote(html),
            "<p>Vďaka, pozriem sa na to.</p><br>"
        );
    }

    #[test]
    fn a_body_that_is_only_attribution_and_quote_stays_whole() {
        let html = "<div class=\"gmail_attr\">On 13.09.2026 11:27, Jakub wrote:</div>\
                    <blockquote><p>iba citát</p></blockquote>"
            .to_string();

        assert_eq!(fold_html_quote(html.clone()), html);
        assert_eq!(strip_html_quote(html.clone()), html);
    }
}
