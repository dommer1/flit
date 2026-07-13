-- Reply All must rebuild the original To/Cc split, but to_addr held both
-- lists joined. Cc moves to its own column; rows cached before this
-- migration keep the joined value in to_addr (raw headers are gone) and
-- heal on the next full resync.
ALTER TABLE messages ADD COLUMN cc_addr TEXT NOT NULL DEFAULT '';

-- FTS5 tables cannot be ALTERed — drop and recreate with the new column.
-- The index is derived data, so this loses nothing.
DROP TRIGGER messages_fts_after_insert;
DROP TRIGGER messages_fts_after_delete;
DROP TRIGGER messages_fts_after_update;
DROP TABLE messages_fts;

CREATE VIRTUAL TABLE messages_fts USING fts5(
    from_addr,
    to_addr,
    cc_addr,
    subject,
    body_text,
    content='messages',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER messages_fts_after_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, from_addr, to_addr, cc_addr, subject, body_text)
    VALUES (new.id, new.from_addr, new.to_addr, new.cc_addr, new.subject, new.body_text);
END;

CREATE TRIGGER messages_fts_after_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, from_addr, to_addr, cc_addr, subject, body_text)
    VALUES ('delete', old.id, old.from_addr, old.to_addr, old.cc_addr, old.subject, old.body_text);
END;

CREATE TRIGGER messages_fts_after_update AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, from_addr, to_addr, cc_addr, subject, body_text)
    VALUES ('delete', old.id, old.from_addr, old.to_addr, old.cc_addr, old.subject, old.body_text);
    INSERT INTO messages_fts (rowid, from_addr, to_addr, cc_addr, subject, body_text)
    VALUES (new.id, new.from_addr, new.to_addr, new.cc_addr, new.subject, new.body_text);
END;

INSERT INTO messages_fts (messages_fts) VALUES ('rebuild');
