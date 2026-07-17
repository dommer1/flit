-- A scheduled reply keeps its threading identity, so it still threads into
-- its conversation when it finally leaves. NULL = fresh (unthreaded) mail.
ALTER TABLE scheduled_messages ADD COLUMN in_reply_to TEXT;
ALTER TABLE scheduled_messages ADD COLUMN references_hdr TEXT;
