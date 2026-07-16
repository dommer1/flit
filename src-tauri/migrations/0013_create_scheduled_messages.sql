-- Messages parked by "Send Later" until their delivery time. Unlike the
-- undo-send queue (in-memory, seconds-long) these must survive a restart,
-- so they live in SQLite. Rows are deleted on send/cancel — the delete is
-- the atomic claim that prevents double-sending (see storage/scheduled.rs).
CREATE TABLE scheduled_messages (
    id           INTEGER PRIMARY KEY,
    account_id   INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    to_addr      TEXT    NOT NULL,
    cc_addr      TEXT    NOT NULL DEFAULT '',
    bcc_addr     TEXT    NOT NULL DEFAULT '',
    subject      TEXT    NOT NULL DEFAULT '',
    body         TEXT    NOT NULL DEFAULT '',
    -- HTML version of the body; NULL = plain-text-only message.
    body_html    TEXT,
    -- Attachment references as a JSON array of {path, name}. Like drafts,
    -- only paths are stored — the bytes are read from disk at send time.
    attachments  TEXT    NOT NULL DEFAULT '[]',
    -- Unix seconds (UTC) when the message should leave.
    scheduled_at INTEGER NOT NULL,
    -- 'pending' = waiting for its time. 'missed' = the time passed while the
    -- app was closed (or asleep past the grace window); never auto-sent —
    -- the user decides in the catch-up dialog.
    status       TEXT    NOT NULL DEFAULT 'pending'
);
