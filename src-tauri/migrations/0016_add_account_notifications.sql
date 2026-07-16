-- Per-account new-mail notification overrides. NULL = inherit the global
-- defaults from the settings table; a stored value wins over the default.
-- notify_enabled: 0/1. notify_sound: "none" or a macOS sound name ("Ping").
ALTER TABLE accounts ADD COLUMN notify_enabled INTEGER;
ALTER TABLE accounts ADD COLUMN notify_sound TEXT;
