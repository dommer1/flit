-- Attachment metadata of cached bodies. Metadata only — the bytes stay on
-- the server (keeps the DB small) and are re-fetched on demand when the
-- user saves a file.
CREATE TABLE message_attachments (
    id           INTEGER PRIMARY KEY,
    message_id   INTEGER NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    -- Position in mail-parser's attachment enumeration of the raw message;
    -- the key that re-extracts this part from a fresh fetch.
    part_index   INTEGER NOT NULL,
    filename     TEXT    NOT NULL,
    content_type TEXT    NOT NULL,
    size         INTEGER NOT NULL
);

CREATE INDEX idx_message_attachments_message ON message_attachments (message_id);

-- Whether the cached body was parsed by a build that harvests attachment
-- metadata; 0 = cached before this table existed, so the next open rescans.
ALTER TABLE messages ADD COLUMN attachments_scanned INTEGER NOT NULL DEFAULT 0;
