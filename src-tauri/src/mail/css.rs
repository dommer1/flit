//! SECURITY-CRITICAL (CLAUDE.md hard rule): sanitizes the CSS in message
//! `<style>` blocks before it reaches the webview. Part of the body-rendering
//! path — read `mail/sanitize.rs` first; this module only handles stylesheet
//! text, `sanitize.rs` owns the surrounding document.
//!
//! Policy, matching the inline-`style` allowlist:
//! - Only property names in `allowed` survive — and none of those can carry a
//!   URL, so no declaration can reach the network.
//! - Only style rules and `@media`/`@supports` groups are kept; `@import`,
//!   `@font-face` and every other at-rule are dropped (the CSS network
//!   vector).
//! - The stylesheet is PARSED and RE-SERIALISED by lightningcss. The original
//!   `<style>` text never reaches the output, which closes the classic
//!   `</style>`-smuggling hole (a sanitizer/parser boundary mismatch).
//! - The re-serialised output is finally made safe to embed in a `<style>`
//!   element: lightningcss can still emit a literal `</style` (e.g. from an
//!   attribute selector `[x="\3c/style"]` it unescapes), so every `</` is
//!   escaped to `<\/` — identical CSS, but no byte sequence can end the host
//!   `<style>` element early.

use std::collections::HashSet;

use lightningcss::declaration::DeclarationBlock;
use lightningcss::properties::Property;
use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;

/// Extract every `<style>` block from `html`, sanitize each, and return the
/// concatenated safe CSS (empty when there is nothing usable). The caller
/// injects the result as its own trusted `<style>` and strips the originals.
pub fn sanitize_style_blocks(html: &str, allowed: &HashSet<&str>) -> String {
    let mut css = String::new();
    for block in style_block_contents(html) {
        if let Some(clean) = sanitize_stylesheet(block, allowed) {
            css.push_str(&clean);
        }
    }
    embed_safe(css)
}

/// Neutralise any `</` so the CSS cannot terminate the `<style>` element it
/// gets embedded in. `<\/` is an identical `/` inside a CSS string (the only
/// place `</` can occur in valid serialised CSS), so this changes bytes, not
/// meaning.
fn embed_safe(css: String) -> String {
    if css.contains("</") {
        css.replace("</", "<\\/")
    } else {
        css
    }
}

/// The raw CSS text inside each `<style>…</style>` element, in order.
///
/// why hand-rolled and why that is safe: this only *locates* blocks; every
/// byte it returns is handed to lightningcss before it can reach the webview,
/// so a mislocated boundary can at worst lose styling — it can never leak
/// unsanitized CSS. `<style>` is an HTML raw-text element (HTML5 §13.2), so
/// its content runs verbatim until the first `</style`, which makes the scan
/// well defined without a full HTML parse.
fn style_block_contents(html: &str) -> Vec<&str> {
    let lower = html.to_ascii_lowercase();
    let mut blocks = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find("<style") {
        let tag_start = cursor + rel;
        let after_name = tag_start + "<style".len();
        // Reject `<styles>`, `<style-x>` …: a real <style> tag ends its name
        // with whitespace, '>' or '/'. Anything else is a different element.
        let is_style_tag = lower[after_name..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_whitespace() || c == '>' || c == '/');
        if !is_style_tag {
            cursor = after_name;
            continue;
        }
        // Skip the rest of the opening tag to its '>'.
        let Some(gt_rel) = lower[after_name..].find('>') else {
            break;
        };
        let content_start = after_name + gt_rel + 1;
        let Some(close_rel) = lower[content_start..].find("</style") else {
            break;
        };
        let content_end = content_start + close_rel;
        blocks.push(&html[content_start..content_end]);
        // Advance past the closing tag's '>' (fall back to content_end so a
        // truncated closing tag still moves the cursor forward).
        cursor = lower[content_end..]
            .find('>')
            .map(|g| content_end + g + 1)
            .unwrap_or(content_end.max(content_start));
    }
    blocks
}

/// Sanitize one stylesheet's text; `None` when it cannot be parsed at all
/// (the caller then drops the block rather than emitting anything).
fn sanitize_stylesheet(css: &str, allowed: &HashSet<&str>) -> Option<String> {
    // error_recovery: one malformed rule must not discard the whole sheet.
    let options = ParserOptions {
        error_recovery: true,
        ..Default::default()
    };
    let mut sheet = StyleSheet::parse(css, options).ok()?;
    filter_rules(&mut sheet.rules.0, allowed);
    if sheet.rules.0.is_empty() {
        return None;
    }
    let printed = sheet.to_css(PrinterOptions::default()).ok()?;
    Some(printed.code)
}

/// Recursively keep only allowlisted declarations inside style rules and
/// `@media`/`@supports` groups, then drop rules left empty and every other
/// rule type (`@import`, `@font-face`, `@namespace`, `@charset`, …).
fn filter_rules(rules: &mut Vec<CssRule>, allowed: &HashSet<&str>) {
    for rule in rules.iter_mut() {
        match rule {
            CssRule::Style(style) => filter_declarations(&mut style.declarations, allowed),
            CssRule::Media(media) => filter_rules(&mut media.rules.0, allowed),
            CssRule::Supports(supports) => filter_rules(&mut supports.rules.0, allowed),
            _ => {}
        }
    }
    rules.retain(|rule| match rule {
        CssRule::Style(style) => !is_empty_block(&style.declarations),
        CssRule::Media(media) => !media.rules.0.is_empty(),
        CssRule::Supports(supports) => !supports.rules.0.is_empty(),
        // Import, FontFace, Namespace, Charset, Page, Keyframes, … all go.
        _ => false,
    });
}

fn filter_declarations(block: &mut DeclarationBlock, allowed: &HashSet<&str>) {
    let keep = |property: &Property| {
        property
            .property_id()
            .to_css_string(PrinterOptions::default())
            .map(|name| allowed.contains(name.as_str()))
            .unwrap_or(false)
    };
    block.declarations.retain(&keep);
    block.important_declarations.retain(&keep);
}

fn is_empty_block(block: &DeclarationBlock) -> bool {
    block.declarations.is_empty() && block.important_declarations.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow() -> HashSet<&'static str> {
        [
            "color",
            "display",
            "width",
            "font-size",
            "line-height",
            "max-height",
            "opacity",
            "overflow",
            "background-color",
            "text-align",
        ]
        .into_iter()
        .collect()
    }

    fn clean(html: &str) -> String {
        sanitize_style_blocks(html, &allow())
    }

    #[test]
    fn keeps_class_rules_so_preheaders_can_hide() {
        // The whole point: class-based display:none lives in a <style> block.
        let css = clean(r#"<style>.preheader{display:none;color:red;font-size:1px}</style>"#);

        assert!(css.contains("display: none"));
        assert!(css.contains(".preheader"));
    }

    #[test]
    fn drops_import_and_font_face_at_rules() {
        let css = clean(
            r#"<style>@import url(https://evil/x.css);
               @font-face{font-family:x;src:url(https://evil/f.woff)}
               h1{color:blue}</style>"#,
        );

        assert!(!css.contains("@import"));
        assert!(!css.contains("@font-face"));
        assert!(!css.contains("evil"));
        assert!(css.contains("color"));
    }

    #[test]
    fn drops_url_bearing_and_positioning_declarations() {
        let css = clean(
            r#"<style>.a{background-image:url(https://t/x.png);position:fixed;color:red}</style>"#,
        );

        assert!(!css.to_lowercase().contains("url("));
        assert!(!css.to_lowercase().contains("position"));
        assert!(!css.contains("t/x.png"));
        assert!(css.contains("color: red"));
    }

    #[test]
    fn keeps_media_queries_and_prunes_empty_groups() {
        let css = clean(
            r#"<style>@media (max-width:600px){.col{width:100%;position:absolute}}
               @media print{.x{position:fixed}}</style>"#,
        );

        // The responsive rule survives with only its safe declaration…
        assert!(css.contains("@media"));
        assert!(css.contains("width: 100%"));
        assert!(!css.to_lowercase().contains("position"));
        // …and the second @media, empty after filtering, is gone entirely.
        assert_eq!(css.matches("@media").count(), 1);
    }

    #[test]
    fn reserialises_rather_than_passing_style_text_through() {
        // A </style> smuggling attempt: the raw text must never survive, only
        // lightningcss's re-serialised tokens.
        let css = clean(r#"<style>.a{color:red}</style><style>.b{color:blue}</style>"#);

        assert!(css.contains(".a"));
        assert!(css.contains(".b"));
        assert!(!css.contains("<style"));
        assert!(!css.contains("</style"));
    }

    #[test]
    fn ignores_lookalike_tags_and_missing_blocks() {
        assert_eq!(clean("<p>no styles here</p>"), "");
        // <styles> (plural) is a different element, not a stylesheet.
        assert_eq!(clean(r#"<styles>.a{color:red}</styles>"#), "");
    }

    #[test]
    fn handles_unterminated_style_block_without_panicking() {
        // A truncated block: no closing tag — nothing to safely extract.
        assert_eq!(clean(r#"<style>.a{color:red}"#), "");
    }

    #[test]
    fn survives_a_malformed_rule_among_good_ones() {
        let css = clean(r#"<style>.a{color:red} .b{ !!broken } .c{color:blue}</style>"#);

        assert!(css.contains(".a"));
        assert!(css.contains(".c"));
    }

    #[test]
    fn escapes_style_close_sequence_that_the_parser_reintroduces() {
        // lightningcss unescapes \3c/style into a literal </style> in an
        // attribute selector — which would break out of the host <style>.
        // The output must never carry that byte sequence.
        let css = clean(r#"<style>[data-x="\3c/style\3e"]{color:red}</style>"#);

        assert!(!css.to_lowercase().contains("</style"));
        // The rule is still there, just embed-safe.
        assert!(css.contains("color: red"));
    }
}
