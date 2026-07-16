-- A user-chosen accent color per account, so multiple accounts can be told
-- apart at a glance (sidebar tint + a dot on each message). NULL = no color.
-- Stored as a plain string (e.g. "#ff9f0a"); the UI owns the palette.
ALTER TABLE accounts ADD COLUMN color TEXT;
