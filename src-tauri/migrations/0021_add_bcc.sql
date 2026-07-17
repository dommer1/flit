-- Bcc recipients of a cached message. Usually empty — most transports strip
-- the header — but Sent copies written by other clients (and journaled or
-- bcc'd deliveries) can carry it, and the conversation view shows it.
ALTER TABLE messages ADD COLUMN bcc_addr TEXT NOT NULL DEFAULT '';
