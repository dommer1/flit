import { openUrl } from "@tauri-apps/plugin-opener";

/**
 * Link handling for the sandboxed message-body iframe.
 *
 * SECURITY: the body sandbox blocks navigation inside the frame, so a clicked
 * link goes nowhere on its own. The parent intercepts the click here and hands
 * http(s) URLs to the OS default browser via the opener plugin. Every link
 * click is swallowed (`preventDefault`), even ones we don't open: nothing a
 * message links to may ever navigate anything inside the app.
 *
 * On WebKit (macOS) this listener never runs — a scripts-sandboxed frame gets
 * no script at all there, not even the parent's listeners (WebKit bug 218086).
 * The links still work: they carry `target="_blank"` from the sanitizer, so
 * the click becomes a new-window request that Rust denies and forwards to the
 * browser. This path is what handles the click on webviews without that bug,
 * where `preventDefault` also keeps the new-window request from firing twice.
 * The tooltip rewrite below is DOM work, not script, so it runs everywhere.
 */
export function hookBodyLinks(doc: Document, open: (url: string) => void) {
  // Anti-phishing: the tooltip always shows where a link really goes,
  // overwriting any title the sender chose ("Your bank" on an evil URL).
  for (const anchor of Array.from(doc.querySelectorAll("a[href]"))) {
    anchor.setAttribute("title", (anchor.getAttribute("href") ?? "").trim());
  }
  doc.addEventListener("click", (event) => {
    const anchor = (event.target as Element | null)?.closest?.("a[href]");
    if (!anchor) return;
    event.preventDefault();
    // why the raw attribute, not anchor.href: a srcdoc document resolves
    // relative URLs against the app's own base URL — the resolved property
    // would turn "/relative" into an app URL that passes the scheme check.
    const href = (anchor.getAttribute("href") ?? "").trim();
    if (/^https?:/i.test(href)) open(href);
  });
}

/**
 * Svelte action for the body iframe. Rehooks on every `load`: the srcdoc
 * replaces the initial about:blank document (and "Load Images" swaps it
 * again), and each new document needs its own listener.
 */
export function interceptBodyLinks(frame: HTMLIFrameElement) {
  const hook = () => {
    const doc = frame.contentDocument;
    if (doc) hookBodyLinks(doc, (url) => void openUrl(url));
  };
  frame.addEventListener("load", hook);
  hook();
  return {
    destroy() {
      frame.removeEventListener("load", hook);
    },
  };
}
