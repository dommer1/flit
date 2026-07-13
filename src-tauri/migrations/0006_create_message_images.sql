-- Inline (cid:) images of cached bodies. Stored decoded at body-fetch time
-- because messages keeps only the parsed text/html, never the raw MIME —
-- without this table a cid: reference would have nothing to resolve to.
CREATE TABLE message_images (
    id           INTEGER PRIMARY KEY,
    message_id   INTEGER NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    -- Content-ID without angle brackets, exactly as cid: URLs reference it.
    content_id   TEXT    NOT NULL,
    content_type TEXT    NOT NULL,
    data         BLOB    NOT NULL
);

CREATE INDEX idx_message_images_message ON message_images (message_id);
