//! SECURITY-CRITICAL (CLAUDE.md hard rule): every change here alters how
//! untrusted email HTML is rendered. The output of `build_srcdoc` is the ONLY
//! HTML form a message body may take on its way to the webview, and it is
//! rendered exclusively inside a fully sandboxed iframe (MessageView.svelte).

use std::borrow::Cow;
use std::collections::HashMap;
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
/// style-src 'unsafe-inline' is allowed above. Untrusted styles don't exist
/// at this point: ammonia strips style attributes and <style> tags.
const BODY_STYLE: &str = "body{font-family:system-ui,sans-serif;font-size:0.875rem;color:#1a1a1a;\
     margin:0.5rem;word-wrap:break-word}img{max-width:100%}";

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
    SanitizedBody {
        html: format!(
            "<!doctype html><html><head>\
             <meta charset=\"utf-8\">\
             <meta http-equiv=\"Content-Security-Policy\" content=\"{BODY_CSP}\">\
             <style>{BODY_STYLE}</style>\
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
        .attribute_filter(move |element, attribute, value| {
            if element == "img" && attribute == "src" {
                return img_src(value, &data_uris, &remote, &counter);
            }
            // Everywhere else (a href, blockquote cite, …) data: and cid:
            // are removed — a data: link in the sandbox is still a webview
            // navigation and has no legitimate use in mail.
            if scheme_is(value, "data") || scheme_is(value, "cid") {
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
}
