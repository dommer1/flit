-- Recipients were never stored; search needs them for the to: operator.
ALTER TABLE messages ADD COLUMN to_addr TEXT NOT NULL DEFAULT '';

-- Full-text index over the searchable message fields. content='messages'
-- (an "external content" table) means FTS5 stores only the token index and
-- reads the original text from messages by rowid — nothing is duplicated.
-- remove_diacritics 2 folds accents at index AND query time, so a query
-- typed without diacritics ("faktura") still matches "faktúra".
CREATE VIRTUAL TABLE messages_fts USING fts5(
    from_addr,
    to_addr,
    subject,
    body_text,
    content='messages',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

-- External-content FTS is not maintained automatically — these three
-- triggers are the single write path keeping the index in lockstep with
-- messages. The 'delete' command must be given the old column values so
-- FTS5 can locate the index entries to remove.
CREATE TRIGGER messages_fts_after_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, from_addr, to_addr, subject, body_text)
    VALUES (new.id, new.from_addr, new.to_addr, new.subject, new.body_text);
END;

CREATE TRIGGER messages_fts_after_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, from_addr, to_addr, subject, body_text)
    VALUES ('delete', old.id, old.from_addr, old.to_addr, old.subject, old.body_text);
END;

CREATE TRIGGER messages_fts_after_update AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, from_addr, to_addr, subject, body_text)
    VALUES ('delete', old.id, old.from_addr, old.to_addr, old.subject, old.body_text);
    INSERT INTO messages_fts (rowid, from_addr, to_addr, subject, body_text)
    VALUES (new.id, new.from_addr, new.to_addr, new.subject, new.body_text);
END;

-- Index whatever was cached before this migration ran.
INSERT INTO messages_fts (messages_fts) VALUES ('rebuild');
