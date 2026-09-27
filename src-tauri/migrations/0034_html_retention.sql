-- Message HTML is kept for 60 days after it was last opened (or fetched,
-- if it never was), then dropped; the plain text stays for search, snippets
-- and an offline read, and opening the message fetches the HTML again. Asked
-- for 2026-09-27: HTML is ~90% of the cached body bytes (333 MB vs 32 MB of
-- text on a real mailbox) and nearly all of it is never read twice.
--
-- why a side table and not a column on messages: that column would sit
-- after the bodies, so every "opened now" would rewrite a whole row, HTML
-- and all, and the daily retention scan would read every body to reach it.
-- One small row per cached HTML body costs neither.
CREATE TABLE html_touches (
    message_id INTEGER PRIMARY KEY REFERENCES messages (id) ON DELETE CASCADE,
    -- Unix seconds: when the HTML was stored or last shown.
    touched_at INTEGER NOT NULL
);

-- 1 = retention removed this row's HTML; opening it should fetch it again.
ALTER TABLE messages ADD COLUMN html_dropped INTEGER NOT NULL DEFAULT 0;

-- HTML cached before today starts its 60 days now.
INSERT INTO html_touches (message_id, touched_at)
SELECT id, CAST(strftime('%s', 'now') AS INTEGER) FROM messages WHERE body_html IS NOT NULL;
