## What changed

<!-- One change per PR. Describe it plainly; the diff shows the how. -->

## Why

<!-- The problem this solves and why this approach. Link the discussion if there was one. -->

Fixes #

## Security impact

<!-- Flit has hard rules (see CLAUDE.md). Tick what this PR touches, or the first box. -->

- [ ] None: no change to message rendering, credentials, storage of secrets, or network calls
- [ ] Message-body rendering or sanitising (`mail/sanitize.rs`, the iframe in `MessageCard.svelte`, compose quote)
- [ ] Credentials, tokens or keychain access
- [ ] Adds or changes a network call (say to which host and when it fires)

## Visual proof

<!-- Required for UI changes: before/after screenshots, in light AND dark mode. Drag them into this box, do not commit image files. Otherwise write N/A. -->

## Testing

<!-- What a reviewer can do to see it working. Which mail provider did you test against? -->

- [ ] Tests added or updated (test-first is the norm here), or explained below why not
- [ ] `npm run check`, `npm test`, `cargo test` and `cargo clippy -- -D warnings` pass locally

## Checklist

- [ ] Small and focused; unrelated cleanups are a separate PR
- [ ] Non-obvious choices have a short `// why:` comment
- [ ] No new abstraction layer without a present need (see CLAUDE.md, "Simplicity over cleverness")
- [ ] Svelte 5 runes only, no `$:`; idiomatic Rust, no `.unwrap()` on runtime paths
