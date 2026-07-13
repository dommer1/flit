-- Cache of remote message images the user chose to load, keyed by URL so a
-- reopened message never re-pings the sender's server. Rows expire (see
-- mail/remote.rs) — this is a convenience cache, not an archive.
CREATE TABLE remote_images (
    url          TEXT    PRIMARY KEY,
    content_type TEXT    NOT NULL,
    data         BLOB    NOT NULL,
    -- Unix seconds; drives expiry.
    fetched_at   INTEGER NOT NULL
);
