-- Cache of sender-domain avatars, keyed by domain (never by address) so one
-- lookup serves every message that sender ever writes. That key is the whole
-- privacy argument: a domain is fetched at most once per expiry window, so
-- the icon host learns nothing about which mail was opened, or when.
--
-- A row with NULL data is a negative result — the domain was checked and had
-- no usable icon — which stops a plain domain being re-fetched on every sync.
-- Rows expire (see mail/avatars.rs); this is a cache, not an archive.
CREATE TABLE domain_avatars (
    domain       TEXT    PRIMARY KEY,
    content_type TEXT,
    data         BLOB,
    -- Unix seconds; drives expiry.
    fetched_at   INTEGER NOT NULL
);
