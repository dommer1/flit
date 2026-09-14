-- Cache of on-device summaries, keyed by a hash of everything that shaped
-- one (scope, model, language, prompt version, the message texts) — so a
-- reopened message shows its summary at once, and a conversation that grew
-- simply misses and is summarized again. Rows expire (see storage/summaries.rs).
CREATE TABLE summaries (
    key        TEXT    PRIMARY KEY,
    text       TEXT    NOT NULL,
    -- Unix seconds; drives expiry.
    created_at INTEGER NOT NULL
);
