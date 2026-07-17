-- Conversation threading: the raw RFC 5322 ids plus the computed thread key.
-- message_id_hdr stays NULL only on rows cached before this migration — new
-- inserts always store a value ('' when the sender set none), so the sync
-- backfill can target IS NULL and is guaranteed to terminate.
ALTER TABLE messages ADD COLUMN message_id_hdr TEXT;
ALTER TABLE messages ADD COLUMN in_reply_to_hdr TEXT NOT NULL DEFAULT '';
ALTER TABLE messages ADD COLUMN references_hdr TEXT NOT NULL DEFAULT '';
-- The conversation this message belongs to, scoped per account. NULL when the
-- message carries no Message-ID at all — such a row can never be threaded.
ALTER TABLE messages ADD COLUMN thread_key TEXT;

CREATE INDEX idx_messages_thread ON messages (account_id, thread_key);
CREATE INDEX idx_messages_msgid ON messages (account_id, message_id_hdr);
