# Security policy

Flit renders untrusted HTML from strangers all day, so security reports are
taken seriously and answered.

## Reporting

Please do not open a public issue. Use GitHub's private vulnerability
reporting: the "Report a vulnerability" button under this repository's
Security tab, or <https://github.com/dommer1/flit/security/advisories/new>.

You will get an acknowledgement, a fix will be prepared privately, and the
report will be credited in the release notes unless you prefer otherwise.

## What counts

Anything that breaks a rule from `CLAUDE.md`:

- JavaScript or remote resources executing or loading from a message body
- message HTML reaching the app's main frame unsanitised
- credentials or tokens written to disk, logs or the database in plaintext
- any network connection that is not TLS, or to a host other than the user's
  own mail servers (beyond the documented opt-in image and favicon fetches)

Bugs that only affect availability of your own client (a malformed mail that
crashes the parser, for example) are welcome as regular issues.

## Supported versions

Flit is pre-1.0. Only the latest commit on `main` is supported.
