-- The update trigger fired on every UPDATE of messages, whatever changed:
-- marking a mail read, moving it, or setting its thread key deleted its row
-- from the search index and re-tokenized the whole body back in. Only the
-- five indexed columns can change what the index holds, so only they fire
-- it now. Measured on a copy of a real DB, 1,000 read-flag flips on rows
-- with bodies: 82-186 ms before, 7-17 ms after.
DROP TRIGGER messages_fts_after_update;

CREATE TRIGGER messages_fts_after_update
AFTER UPDATE OF from_addr, to_addr, cc_addr, subject, body_text ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, from_addr, to_addr, cc_addr, subject, body_text)
    VALUES ('delete', old.id, old.from_addr, old.to_addr, old.cc_addr, old.subject, old.body_text);
    INSERT INTO messages_fts (rowid, from_addr, to_addr, cc_addr, subject, body_text)
    VALUES (new.id, new.from_addr, new.to_addr, new.cc_addr, new.subject, new.body_text);
END;
