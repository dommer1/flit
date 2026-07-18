-- How many messages the server reported in this folder (EXISTS of the last
-- SELECT). NULL until the folder's first sync. Drives the backfill progress
-- indicator: cached rows vs. this total.
ALTER TABLE mailboxes ADD COLUMN server_exists INTEGER;
