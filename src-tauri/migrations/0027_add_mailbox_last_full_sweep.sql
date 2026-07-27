-- When this folder last had a full flag reconciliation (unix epoch seconds).
-- Ordinary passes only sweep the newest slice of a folder; the full sweep --
-- the one that can still spot a deletion far below that window -- runs on a
-- much longer cadence and records its time here. NULL = never swept, so the
-- next pass does a full one.
ALTER TABLE mailboxes ADD COLUMN last_full_sweep INTEGER;
