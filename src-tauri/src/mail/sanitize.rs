//! SECURITY-CRITICAL (CLAUDE.md hard rule): every change here alters how
//! untrusted email HTML is rendered. The output of `build_srcdoc` is the ONLY
//! HTML form a message body may take on its way to the webview, and it is
//! rendered exclusively inside a fully sandboxed iframe (MessageView.svelte).

/// CSP embedded in every rendered message document. `default-src 'none'`
/// blocks ALL remote loads — tracking pixels, fonts, media, CSS url().
/// The future "load remote images" opt-in extends this string (img-src) for
/// that single render; nothing else needs to change.
const BODY_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'";

/// Base styling for the message document — our own trusted CSS, the reason
/// style-src 'unsafe-inline' is allowed above. Untrusted styles don't exist
/// at this point: ammonia strips style attributes and <style> tags.
const BODY_STYLE: &str = "body{font-family:system-ui,sans-serif;font-size:0.875rem;color:#1a1a1a;\
     margin:0.5rem;word-wrap:break-word}";

/// Sanitize untrusted email HTML and wrap it in a locked-down standalone
/// document for iframe `srcdoc` rendering.
///
/// why: sanitization happens on every read, never at store time — the DB is
/// treated as untrusted and future sanitizer improvements apply retroactively
/// to already-cached mail.
pub fn build_srcdoc(untrusted_html: &str) -> String {
    let clean = ammonia::clean(untrusted_html);
    format!(
        "<!doctype html><html><head>\
         <meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{BODY_CSP}\">\
         <style>{BODY_STYLE}</style>\
         </head><body>{clean}</body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_tags() {
        let doc = build_srcdoc("<p>hi</p><script>alert(1)</script>");

        assert!(!doc.to_lowercase().contains("<script"));
        assert!(doc.contains("<p>hi</p>"));
    }

    #[test]
    fn strips_event_handlers() {
        let doc = build_srcdoc(r#"<img src="cid:x" onerror="alert(1)">"#);

        assert!(!doc.contains("onerror"));
    }

    #[test]
    fn strips_javascript_urls() {
        let doc = build_srcdoc(r#"<a href="javascript:alert(1)">click</a>"#);

        assert!(!doc.contains("javascript:"));
    }

    #[test]
    fn keeps_img_src_for_structure() {
        // The tag survives so the layout is intact, but the embedded CSP
        // (default-src 'none') prevents it from ever loading.
        let doc = build_srcdoc(r#"<img src="https://example.com/pixel.png" alt="x">"#);

        assert!(doc.contains("<img"));
        assert!(doc.contains("pixel.png"));
    }

    #[test]
    fn embeds_the_lockdown_csp() {
        let doc = build_srcdoc("<p>hi</p>");

        assert!(doc.contains(
            r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'">"#
        ));
        assert!(doc.starts_with("<!doctype html>"));
    }

    #[test]
    fn strips_iframes_and_forms() {
        let doc = build_srcdoc(
            r#"<iframe src="https://evil"></iframe><form action="https://evil"><input></form>"#,
        );

        assert!(!doc.contains("<iframe"));
        assert!(!doc.contains("<form"));
    }
}
