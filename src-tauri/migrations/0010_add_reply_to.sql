-- Reply-To recipients, shown in the message detail alongside From/To/Cc.
-- Not indexed for search. Rows cached before this migration keep the empty
-- default (raw headers are gone) and heal on the next full resync.
ALTER TABLE messages ADD COLUMN reply_to_addr TEXT NOT NULL DEFAULT '';
