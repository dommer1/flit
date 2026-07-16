-- Addresses harvested from cached mail (senders and recipients) plus the
-- recipients of everything sent from Flit. Purely local — this is the whole
-- "address book" behind compose autocomplete; no external directory.
CREATE TABLE contacts (
    id         INTEGER PRIMARY KEY,
    email      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    -- Latest non-empty display name seen for this address.
    name       TEXT    NOT NULL DEFAULT '',
    -- How many messages this address appeared on — the primary rank.
    seen_count INTEGER NOT NULL DEFAULT 1,
    -- RFC3339 date of the newest message it appeared on — the tiebreaker.
    last_seen  TEXT    NOT NULL DEFAULT ''
);
