-- assign_thread_key's message-id probe must also read thread_key; without it
-- in the index, every probe walks the matched row — and the threading columns
-- (added in 0017) sit behind body_text/body_html, so rows with a cached body
-- drag the whole body's overflow-page chain into the read. Widening the index
-- makes the probe index-only. The old index is a prefix of the new one, so
-- keeping both would just double the insert cost.
DROP INDEX idx_messages_msgid;
CREATE INDEX idx_messages_msgid_thread ON messages (account_id, message_id_hdr, thread_key);
