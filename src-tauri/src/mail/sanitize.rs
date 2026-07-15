//! SECURITY-CRITICAL (CLAUDE.md hard rule): every change here alters how
//! untrusted email HTML is rendered. The output of `build_srcdoc` is the ONLY
//! HTML form a message body may take on its way to the webview, and it is
//! rendered exclusively inside a fully sandboxed iframe (MessageView.svelte).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;

use crate::mail::parse::InlineImage;

/// CSP embedded in every rendered message document. `default-src 'none'`
/// blocks ALL remote loads — tracking pixels, fonts, media, CSS url().
/// `img-src data:` is the one carve-out: data: URIs carry no network
/// request, so they can't track; they render the message's own cid images.
const BODY_CSP: &str = "default-src 'none'; img-src data:; style-src 'unsafe-inline'";

/// Base styling for the message document — our own trusted CSS, the reason
/// style-src 'unsafe-inline' is allowed above. Untrusted CSS is admitted only
/// through the STYLE_PROPERTIES allowlist: inline `style` attributes via
/// ammonia's filter, and `<style>` blocks via mail::css (parsed, filtered and
/// re-serialised, then injected right after this base).
const BODY_STYLE: &str = "body{font-family:system-ui,sans-serif;font-size:0.875rem;color:#1a1a1a;\
     margin:0.5rem;word-wrap:break-word}img{max-width:100%}";

/// CSS properties permitted inside `style` attributes. ammonia's
/// `filter_style_properties` normalises every value and drops invalid
/// declarations and @rules, so only these property names — with a valid
/// value — reach the webview.
///
/// Two whole classes are deliberately absent:
/// - **URL-bearing** (`background`, `background-image`, `list-style-image`,
///   `cursor`, `content`, `border-image`, `mask`): the CSS
///   network/tracking vector. CSP (`img-src data:`) would block the load,
///   but the sanitizer must hold on its own — so they never survive here.
///   `background-color` (colour only) stands in for solid backgrounds.
/// - **Positioning** (`position`, `z-index`, `top`/`left`/…): fixed/absolute
///   overlays let a message paint fake UI over the app. The sandbox already
///   contains the message, but overlay spoofing isn't a network problem, so
///   CSP doesn't help — the allowlist is the only guard.
const STYLE_PROPERTIES: &[&str] = &[
    // Text & fonts
    "color",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "font-variant",
    "font-stretch",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "text-align",
    "text-align-last",
    "text-decoration",
    "text-decoration-color",
    "text-decoration-line",
    "text-decoration-style",
    "text-transform",
    "text-indent",
    "text-overflow",
    "text-shadow",
    "white-space",
    "vertical-align",
    "direction",
    "unicode-bidi",
    "word-break",
    "word-wrap",
    "overflow-wrap",
    "writing-mode",
    // Colour (no url)
    "background-color",
    "opacity",
    // Box model
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border",
    "border-width",
    "border-style",
    "border-color",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-top-width",
    "border-top-style",
    "border-top-color",
    "border-right-width",
    "border-right-style",
    "border-right-color",
    "border-bottom-width",
    "border-bottom-style",
    "border-bottom-color",
    "border-left-width",
    "border-left-style",
    "border-left-color",
    "border-radius",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-collapse",
    "border-spacing",
    "box-sizing",
    "box-shadow",
    "outline",
    "outline-color",
    "outline-style",
    "outline-width",
    // Sizing
    "width",
    "height",
    "max-width",
    "max-height",
    "min-width",
    "min-height",
    // Flow & display (no positioning)
    "display",
    "visibility",
    "overflow",
    "overflow-x",
    "overflow-y",
    "float",
    "clear",
    // Tables
    "table-layout",
    "caption-side",
    "empty-cells",
    // Lists (type/position only — list-style-image is url-bearing)
    "list-style-type",
    "list-style-position",
    // Flexbox
    "flex",
    "flex-direction",
    "flex-wrap",
    "flex-flow",
    "flex-grow",
    "flex-shrink",
    "flex-basis",
    "justify-content",
    "align-items",
    "align-content",
    "align-self",
    "gap",
    "row-gap",
    "column-gap",
    "order",
];

/// The allowlist as a set — shared by the inline-`style` filter and the
/// `<style>`-block sanitizer (mail::css) so both honour the same policy.
fn style_property_set() -> HashSet<&'static str> {
    STYLE_PROPERTIES.iter().copied().collect()
}

/// What `build_srcdoc` hands back: the locked-down document plus how many
/// loadable remote images stayed blocked (drives the "Load images" banner).
#[derive(Debug)]
pub struct SanitizedBody {
    pub html: String,
    /// https img references NOT resolved by `remote` — each one would load
    /// if the user asked for it. http: refs are excluded: they are blocked
    /// too, but never loadable (hard rule: all network I/O over TLS).
    pub blocked_remote: usize,
}

/// Sanitize untrusted email HTML and wrap it in a locked-down standalone
/// document for iframe `srcdoc` rendering. `images` are the message's own
/// cid attachments; `remote` maps already-fetched https URLs to data: URIs
/// (empty = remote images stay blocked). Every image the webview shows is a
/// data: URI — the document itself never triggers a network request.
///
/// why: sanitization happens on every read, never at store time — the DB is
/// treated as untrusted and future sanitizer improvements apply retroactively
/// to already-cached mail.
pub fn build_srcdoc(
    untrusted_html: &str,
    images: &[InlineImage],
    remote: &HashMap<String, String>,
) -> SanitizedBody {
    let (clean, blocked_remote) = sanitize(untrusted_html, images, remote);
    // Message <style> blocks: sanitized separately (mail::css parses and
    // re-serialises them under the same property allowlist) and injected as
    // our own trusted <style>, AFTER BODY_STYLE so the message overrides our
    // base. The css module returns text already safe to embed here.
    let message_css =
        crate::mail::css::sanitize_style_blocks(untrusted_html, &style_property_set());
    SanitizedBody {
        html: format!(
            "<!doctype html><html><head>\
             <meta charset=\"utf-8\">\
             <meta http-equiv=\"Content-Security-Policy\" content=\"{BODY_CSP}\">\
             <style>{BODY_STYLE}{message_css}</style>\
             </head><body>{clean}</body></html>"
        ),
        blocked_remote,
    }
}

fn sanitize(
    untrusted_html: &str,
    images: &[InlineImage],
    remote: &HashMap<String, String>,
) -> (String, usize) {
    // why: owned/cloned data moved into the closure — ammonia's
    // attribute_filter demands 'static, so it can't borrow the arguments.
    let data_uris: HashMap<String, String> = images
        .iter()
        .map(|image| (image.content_id.clone(), data_uri(image)))
        .collect();
    let remote = remote.clone();
    let blocked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&blocked);

    let html = ammonia::Builder::default()
        // Both schemes must be whitelisted or ammonia's scheme pass strips
        // them before attribute_filter ever sees the value. Neither leaks
        // through raw: the filter below resolves or removes every cid:, and
        // confines data: to img src.
        .add_url_schemes(&["cid", "data"])
        // Inline styles carry newsletter layout. `style` is allowed on every
        // element, but filter_style_properties keeps only STYLE_PROPERTIES
        // (no url-bearing, no positioning) with a valid value — the rest,
        // and any @rule, is dropped.
        .add_generic_attributes(&["style"])
        .filter_style_properties(style_property_set())
        // `class`/`id` are the hooks the injected <style> selectors match on
        // (mail::css). Neither can reference a URL or run script.
        .add_generic_attributes(&["class", "id"])
        // Legacy presentational HTML that older mail (and many ESP templates)
        // still relies on. `<font>` plus per-tag layout attributes — none can
        // reference a URL. The url-bearing `background` attribute is pointedly
        // NOT here: it is the attribute twin of CSS background-image.
        .add_tags(&["font", "center"])
        .add_tag_attributes("font", &["color", "face", "size"])
        .add_tag_attributes(
            "table",
            &[
                "bgcolor",
                "width",
                "height",
                "cellpadding",
                "cellspacing",
                "border",
                "align",
                "valign",
            ],
        )
        .add_tag_attributes("tr", &["bgcolor", "align", "valign"])
        .add_tag_attributes(
            "td",
            &[
                "bgcolor", "width", "height", "align", "valign", "colspan", "rowspan", "nowrap",
            ],
        )
        .add_tag_attributes(
            "th",
            &[
                "bgcolor", "width", "height", "align", "valign", "colspan", "rowspan", "nowrap",
            ],
        )
        .add_tag_attributes(
            "img",
            &["width", "height", "align", "border", "hspace", "vspace"],
        )
        .attribute_filter(move |element, attribute, value| {
            if element == "img" && attribute == "src" {
                return img_src(value, &data_uris, &remote, &counter);
            }
            // Everywhere else (a href, blockquote cite, …) data: and cid:
            // are removed — a data: link in the sandbox is still a webview
            // navigation and has no legitimate use in mail. `style` is left
            // for filter_style_properties (it runs after this filter).
            if attribute != "style" && (scheme_is(value, "data") || scheme_is(value, "cid")) {
                return None;
            }
            Some(value.into())
        })
        .clean(untrusted_html)
        .to_string();

    let blocked = blocked.load(Ordering::Relaxed);
    (html, blocked)
}

/// The https image URLs a message references, in document order, deduped —
/// the fetch work-list for "load remote images". Collected through the same
/// ammonia pass as rendering, so only references that would actually appear
/// in the sanitized document are ever fetched.
pub fn remote_image_urls(untrusted_html: &str) -> Vec<String> {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let collector = Arc::clone(&seen);
    let _ = ammonia::Builder::default()
        .add_url_schemes(&["cid", "data"])
        .attribute_filter(move |element, attribute, value| {
            if element == "img" && attribute == "src" && scheme_is(value, "https") {
                let url = effective_url(value);
                // why: no unwrap — a poisoned lock just yields fewer URLs.
                if let Ok(mut urls) = collector.lock() {
                    if !urls.contains(&url) {
                        urls.push(url);
                    }
                }
            }
            Some(value.into())
        })
        .clean(untrusted_html);
    seen.lock().map(|urls| urls.clone()).unwrap_or_default()
}

/// Policy for `<img src>`: cid: resolves to the matching inline image (or
/// nothing), data:image/* passes through (inert, can't track), fetched
/// https refs resolve to their data: URI, everything else survives for
/// layout but the CSP prevents it from ever loading.
fn img_src<'v>(
    value: &'v str,
    data_uris: &HashMap<String, String>,
    remote: &HashMap<String, String>,
    blocked: &AtomicUsize,
) -> Option<Cow<'v, str>> {
    let effective = effective_url(value);
    if scheme_is(value, "cid") {
        let cid = &effective["cid:".len()..];
        return data_uris.get(cid).map(|uri| uri.clone().into());
    }
    if scheme_is(value, "data") {
        // Only images may inline data — data:text/html and friends have no
        // business in mail markup.
        if starts_with_ignore_ascii_case(&effective, "data:image/") {
            return Some(value.into());
        }
        return None;
    }
    if scheme_is(value, "https") {
        if let Some(uri) = remote.get(&effective) {
            return Some(uri.clone().into());
        }
        blocked.fetch_add(1, Ordering::Relaxed);
    }
    Some(value.into())
}

/// The URL as a browser's parser would see it: WHATWG strips leading and
/// trailing C0 controls/spaces and removes tabs and newlines anywhere.
///
/// why: a naive starts_with("data:") is bypassable with "\tdata:…" — this
/// check must agree with the browser, not with the raw bytes.
fn effective_url(value: &str) -> String {
    value
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

fn scheme_is(value: &str, scheme: &str) -> bool {
    starts_with_ignore_ascii_case(&effective_url(value), &format!("{scheme}:"))
}

/// why: bytes, not &str[..n] slicing — a multibyte char at the cut point
/// would panic, and this runs on attacker-controlled input.
fn starts_with_ignore_ascii_case(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn data_uri(image: &InlineImage) -> String {
    format!(
        "data:{};base64,{}",
        image.content_type,
        base64::engine::general_purpose::STANDARD.encode(&image.data)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(content_id: &str) -> InlineImage {
        InlineImage {
            content_id: content_id.to_string(),
            content_type: "image/png".to_string(),
            data: b"\x89PNG\r\n\x1a\n".to_vec(),
        }
    }

    /// Most tests care only about the document, not the blocked count.
    fn srcdoc(untrusted_html: &str, images: &[InlineImage]) -> String {
        build_srcdoc(untrusted_html, images, &HashMap::new()).html
    }

    #[test]
    fn strips_script_tags() {
        let doc = srcdoc("<p>hi</p><script>alert(1)</script>", &[]);

        assert!(!doc.to_lowercase().contains("<script"));
        assert!(doc.contains("<p>hi</p>"));
    }

    #[test]
    fn strips_event_handlers() {
        let doc = srcdoc(r#"<img src="cid:x" onerror="alert(1)">"#, &[]);

        assert!(!doc.contains("onerror"));
    }

    #[test]
    fn strips_javascript_urls() {
        let doc = srcdoc(r#"<a href="javascript:alert(1)">click</a>"#, &[]);

        assert!(!doc.contains("javascript:"));
    }

    #[test]
    fn keeps_img_src_for_structure() {
        // The tag survives so the layout is intact, but the embedded CSP
        // (default-src 'none') prevents it from ever loading.
        let doc = srcdoc(r#"<img src="https://example.com/pixel.png" alt="x">"#, &[]);

        assert!(doc.contains("<img"));
        assert!(doc.contains("pixel.png"));
    }

    #[test]
    fn embeds_the_lockdown_csp() {
        let doc = srcdoc("<p>hi</p>", &[]);

        assert!(doc.contains(
            r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'">"#
        ));
        assert!(doc.starts_with("<!doctype html>"));
    }

    #[test]
    fn strips_iframes_and_forms() {
        let doc = srcdoc(
            r#"<iframe src="https://evil"></iframe><form action="https://evil"><input></form>"#,
            &[],
        );

        assert!(!doc.contains("<iframe"));
        assert!(!doc.contains("<form"));
    }

    #[test]
    fn counts_loadable_remote_images_left_blocked() {
        let body = build_srcdoc(
            r#"<img src="https://a.example/x.png"><img src="https://a.example/x.png">
               <img src="http://plain.example/y.png"><img src="data:image/gif;base64,AA==">"#,
            &[],
            &HashMap::new(),
        );

        // Both https refs count (each render blocks each occurrence); the
        // http: one is blocked too but never loadable (TLS-only rule), and
        // data: needs no loading.
        assert_eq!(body.blocked_remote, 2);
    }

    #[test]
    fn resolves_remote_references_from_the_fetched_map() {
        let mut remote = HashMap::new();
        remote.insert(
            "https://a.example/x.png".to_string(),
            "data:image/png;base64,iVBORw0KGgo=".to_string(),
        );

        let body = build_srcdoc(
            r#"<img src="https://a.example/x.png"><img src="https://a.example/missing.png">"#,
            &[],
            &remote,
        );

        assert!(body
            .html
            .contains(r#"src="data:image/png;base64,iVBORw0KGgo=""#));
        assert!(body.html.contains("missing.png"));
        assert_eq!(body.blocked_remote, 1);
    }

    #[test]
    fn collects_https_image_urls_deduped() {
        let urls = remote_image_urls(
            r#"<img src="https://a.example/x.png"><img src="https://a.example/x.png">
               <img src="&#9;https://tab.example/y.png"><img src="http://plain.example/z.png">
               <img src="cid:photo"><img src="data:image/gif;base64,AA==">
               <iframe><img src="https://stripped.example/never.png"></iframe>"#,
        );

        assert_eq!(
            urls,
            vec![
                "https://a.example/x.png".to_string(),
                "https://tab.example/y.png".to_string(),
            ]
        );
    }

    #[test]
    fn resolves_cid_references_to_data_uris() {
        let doc = srcdoc(r#"<p>pic:</p><img src="cid:photo1">"#, &[png("photo1")]);

        assert!(doc.contains(r#"src="data:image/png;base64,iVBORw0KGgo=""#));
        assert!(!doc.contains("cid:"));
    }

    #[test]
    fn unresolvable_cid_loses_its_src() {
        let doc = srcdoc(r#"<img src="cid:ghost" alt="x">"#, &[png("photo1")]);

        assert!(!doc.contains("cid:"));
        assert!(!doc.contains("src="));
        // The tag itself survives for layout.
        assert!(doc.contains("<img"));
    }

    #[test]
    fn keeps_data_image_uris_on_img_only() {
        let doc = srcdoc(
            r#"<img src="data:image/gif;base64,R0lGOD"><a href="data:text/html,<script>x</script>">l</a>"#,
            &[],
        );

        assert!(doc.contains(r#"src="data:image/gif;base64,R0lGOD""#));
        assert!(!doc.contains("href=\"data:"));
    }

    #[test]
    fn strips_non_image_data_uris_from_img_src() {
        let doc = srcdoc(r#"<img src="data:text/html,<b>x</b>" alt="x">"#, &[]);

        assert!(!doc.contains("data:text"));
    }

    #[test]
    fn data_scheme_detection_survives_whatwg_url_cleanup() {
        // A tab inside the scheme fools naive prefix checks but not a
        // browser's URL parser — both sides must agree it's data:.
        let doc = srcdoc(
            "<a href=\"\td\nata:text/html,x\">l</a><img src=\"\tdata:image/png;base64,AA==\">",
            &[],
        );

        assert!(!doc.contains("href="));
        assert!(doc.contains("img"));
    }

    #[test]
    fn keeps_safe_inline_style_properties() {
        // The everyday newsletter vocabulary: colors, fonts, spacing, borders,
        // table sizing — the reason bodies looked broken without it. Wrapped
        // in a real table so ammonia's tree normalisation keeps the <td>.
        let doc = srcdoc(
            r#"<table><tr><td style="background-color:#f4f4f4;color:#333;padding:16px;font-family:Arial;font-size:14px;border:1px solid #ccc;width:600px">hi</td></tr></table>"#,
            &[],
        );

        assert!(doc.contains("background-color:#f4f4f4"));
        assert!(doc.contains("color:#333"));
        assert!(doc.contains("padding:16px"));
        assert!(doc.contains("font-family:Arial"));
        assert!(doc.contains("width:600px"));
    }

    #[test]
    fn strips_positioning_that_enables_overlay_spoofing() {
        // position:fixed/absolute over the whole viewport is how a message
        // could paint fake UI on top of the app — never allowed, even though
        // the sandbox already contains it.
        let doc = srcdoc(
            r#"<div style="position:fixed;top:0;left:0;color:red">x</div>"#,
            &[],
        );

        assert!(doc.contains("color:red"));
        assert!(!doc.to_lowercase().contains("position"));
        assert!(!doc.contains("fixed"));
    }

    #[test]
    fn strips_url_bearing_style_properties() {
        // Every CSS property that can reach the network is dropped by the
        // allowlist — CSP would block the load too, but the sanitizer must
        // hold on its own. background-color (no url) still survives.
        let doc = srcdoc(
            r#"<div style="background-image:url(https://t.example/p.png);
               background:url(https://t.example/q.png);
               list-style-image:url(https://t.example/r.png);
               cursor:url(https://t.example/c.cur),auto;
               background-color:#fff">x</div>"#,
            &[],
        );

        assert!(!doc.to_lowercase().contains("url("));
        assert!(!doc.contains("t.example"));
        assert!(doc.contains("background-color:#fff"));
    }

    #[test]
    fn strips_encoded_url_in_style() {
        // A CSS-escaped "url(" must not slip a remote load past the filter.
        // background-image isn't in the allowlist, so the whole declaration
        // goes regardless of how the url token is spelled.
        let doc = srcdoc(
            r#"<div style="background-image:\75rl(https://t.example/p.png)">x</div>"#,
            &[],
        );

        assert!(!doc.contains("t.example"));
    }

    #[test]
    fn empties_the_style_attribute_when_nothing_survives() {
        // ammonia leaves an inert style="" rather than removing the attribute
        // — what matters is that the forbidden declaration is gone.
        let doc = srcdoc(r#"<p style="position:absolute">hi</p>"#, &[]);

        assert!(!doc.to_lowercase().contains("position"));
        assert!(!doc.contains("absolute"));
        assert!(doc.contains("hi"));
    }

    #[test]
    fn keeps_presentational_table_attributes() {
        // The legacy table-layout vocabulary of HTML email — none of it can
        // reference a URL, all of it is layout.
        let doc = srcdoc(
            r##"<table bgcolor="#eeeeee" width="600" cellpadding="10" cellspacing="0" border="0" align="center">
                <tr valign="top"><td width="50%" height="40" colspan="2" nowrap="nowrap">hi</td></tr></table>"##,
            &[],
        );

        assert!(doc.contains(r##"bgcolor="#eeeeee""##));
        assert!(doc.contains(r#"width="600""#));
        assert!(doc.contains(r#"cellpadding="10""#));
        assert!(doc.contains(r#"valign="top""#));
        assert!(doc.contains(r#"height="40""#));
        assert!(doc.contains(r#"colspan="2""#));
    }

    #[test]
    fn keeps_legacy_font_tag() {
        let doc = srcdoc(
            r##"<font color="#333333" face="Arial" size="4">hi</font>"##,
            &[],
        );

        assert!(doc.contains("<font"));
        assert!(doc.contains(r##"color="#333333""##));
        assert!(doc.contains(r#"face="Arial""#));
    }

    #[test]
    fn strips_url_bearing_background_attribute() {
        // `background="url"` on a table/cell is the attribute-level twin of
        // CSS background-image — a remote load, so it must never survive.
        let doc = srcdoc(
            r#"<table background="https://t.example/bg.png"><tr><td>hi</td></tr></table>"#,
            &[],
        );

        assert!(!doc.contains("background="));
        assert!(!doc.contains("t.example"));
    }

    #[test]
    fn hides_a_class_based_preheader_via_injected_style() {
        // The reported bug: preview text hidden by a <style> class showed in
        // the body because we stripped <style>. Now the sanitized rule is
        // injected into the head and the element keeps its class to match.
        let doc = srcdoc(
            r#"<style>.preheader{display:none}</style><span class="preheader">Preview</span><p>Body</p>"#,
            &[],
        );

        let (head, body) = doc.split_once("<body>").unwrap();
        assert!(head.contains(".preheader"));
        assert!(head.contains("display: none"));
        assert!(body.contains(r#"class="preheader""#));
        // The original <style> must not leak into the body as text.
        assert!(!body.contains("<style"));
    }

    #[test]
    fn keeps_class_and_id_for_stylesheet_selectors() {
        let doc = srcdoc(r#"<div class="col" id="hero">hi</div>"#, &[]);

        assert!(doc.contains(r#"class="col""#));
        assert!(doc.contains(r#"id="hero""#));
    }

    #[test]
    fn sanitizes_injected_stylesheets_like_inline_styles() {
        let doc = srcdoc(
            r#"<style>@import url(https://t.example/x.css);
               .a{color:red;background-image:url(https://t.example/p.png);position:fixed}</style>
               <p class="a">x</p>"#,
            &[],
        );

        assert!(!doc.contains("@import"));
        assert!(!doc.contains("t.example"));
        assert!(!doc.to_lowercase().contains("position:fixed"));
        assert!(doc.contains("color: red"));
    }
}
