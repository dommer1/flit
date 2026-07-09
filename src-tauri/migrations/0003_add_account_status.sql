-- Connection status, refreshed by every sync (which doubles as a health
-- check). last_error holds only the error message — never credentials.
ALTER TABLE accounts ADD COLUMN last_error TEXT;
ALTER TABLE accounts ADD COLUMN checked_at INTEGER;
