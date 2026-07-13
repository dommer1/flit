//! Known email-tracking hosts and URL shapes. Consulted whenever remote
//! images are about to load — under every policy, including "always" —
//! so a deliberate "show me the pictures" never also fires open trackers.
//!
//! Best-effort by design: a curated list of the major ESP tracking hosts
//! plus the common open-pixel URL patterns, shipped with the app (no list
//! downloads — no external calls). It cannot catch a tracker on the
//! sender's own domain; the real protection stays the block-by-default.

/// Hosts (and their subdomains) that exist to track email opens/clicks.
/// Kept to providers whose tracking hosts are well documented.
const TRACKER_DOMAINS: &[&str] = &[
    "list-manage.com",        // Mailchimp campaigns
    "mandrillapp.com",        // Mailchimp transactional
    "sendgrid.net",           // Twilio SendGrid
    "hubspotemail.net",       // HubSpot
    "mkt-email.com",          // HubSpot (alt)
    "exct.net",               // Salesforce Marketing Cloud
    "exacttarget.com",        // Salesforce Marketing Cloud (legacy)
    "marketo.com",            // Adobe Marketo
    "pardot.com",             // Salesforce Pardot
    "rs6.net",                // Constant Contact
    "createsend.com",         // Campaign Monitor
    "activehosted.com",       // ActiveCampaign
    "customeriomail.com",     // Customer.io
    "klaviyomail.com",        // Klaviyo
    "braze.com",              // Braze
    "appboy.com",             // Braze (legacy)
    "pstmrk.it",              // Postmark
    "emltrk.com",             // Litmus analytics
    "mailtrack.io",           // Mailtrack
    "yesware.com",            // Yesware
    "mixmax.com",             // Mixmax
    "bl-1.com",               // Bananatag
    "mailstat.us",            // Boomerang
    "getnotify.com",          // GetNotify
    "mailfoogae.appspot.com", // Streak
    "r.superhuman.com",       // Superhuman read receipts
];

/// Path fragments that only appear in open-tracking endpoints.
const TRACKER_PATH_MARKERS: &[&str] = &["/track/open", "/open.php", "/wf/open", "/e/open"];

/// Classic pixel filenames. Deliberately short of "blank.gif"/"spacer.gif",
/// which legitimate layouts still use.
const TRACKER_FILENAMES: &[&str] = &[
    "open.gif",
    "open.png",
    "pixel.gif",
    "pixel.png",
    "tracker.gif",
    "tracking.gif",
    "beacon.gif",
];

/// Whether this (https) image URL is a known open tracker. Errs toward
/// false — an unrecognized tracker just loads like any image, while a
/// false positive would silently eat a real picture.
pub fn is_tracker(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let (host, path) = split_host_path(&lower);

    if TRACKER_DOMAINS
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    {
        return true;
    }
    if TRACKER_PATH_MARKERS
        .iter()
        .any(|marker| path.contains(marker))
    {
        return true;
    }
    let filename = path
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    TRACKER_FILENAMES.contains(&filename)
}

/// `(host, path-and-after)` of an already-lowercased URL. Hand-rolled on
/// purpose: no url crate in the tree, and a wrong split here only changes
/// which side of best-effort a URL lands on.
fn split_host_path(lower: &str) -> (&str, &str) {
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(lower);
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    // why: host is what follows the LAST '@' — "https://good.com@evil.com/"
    // must classify by evil.com, not good.com.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    (host, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_esp_hosts_match_including_subdomains() {
        assert!(is_tracker(
            "https://acme.us1.list-manage.com/track/open.php?u=abc&id=1"
        ));
        assert!(is_tracker("https://u123.ct.sendgrid.net/anything.png"));
        assert!(is_tracker("https://t.hubspotemail.net/img.gif"));
        assert!(is_tracker("https://SENDGRID.NET/x.png"));
    }

    #[test]
    fn open_pixel_paths_and_filenames_match_anywhere() {
        assert!(is_tracker("https://mail.acme.com/wf/open?upn=xyz"));
        assert!(is_tracker("https://acme.com/newsletter/pixel.gif"));
        assert!(is_tracker("https://acme.com/img/open.gif?id=7"));
    }

    #[test]
    fn ordinary_images_pass() {
        assert!(!is_tracker("https://acme.com/press/team-photo.jpg"));
        assert!(!is_tracker("https://cdn.shopify.com/products/shoe.png"));
        // Substring must not be enough — only whole-label suffix matches.
        assert!(!is_tracker("https://notsendgrid.net.example.com/a.png"));
        assert!(!is_tracker("https://mysendgrid.net/a.png"));
    }

    #[test]
    fn userinfo_cannot_spoof_the_host() {
        // The host here is evil.example — a tracker domain in the userinfo
        // position must not trigger (nor shield) a match.
        assert!(!is_tracker("https://sendgrid.net@evil.example/a.png"));
        assert!(is_tracker("https://evil.example@sendgrid.net/a.png"));
    }

    #[test]
    fn ports_do_not_break_host_matching() {
        assert!(is_tracker("https://sendgrid.net:8443/a.png"));
    }
}
