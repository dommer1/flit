-- The background prefetch (mail/sync.rs) takes each folder's newest cached
-- messages that still lack a body, and leaves out the ones over its size
-- cap. Nothing remembered that refusal, so the same large messages headed
-- the queue on every pass, and a folder whose newest 50 missing bodies were
-- all large was never prefetched any further. 1 = refused, don't offer it
-- again; opening the message still fetches its body on demand.
ALTER TABLE messages ADD COLUMN prefetch_skipped INTEGER NOT NULL DEFAULT 0;
