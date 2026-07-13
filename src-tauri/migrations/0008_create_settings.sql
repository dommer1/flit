-- App-wide preferences, one row per key. Values are plain strings; the
-- typed accessors in storage/settings.rs own their meaning and defaults,
-- so the table itself stays schema-less on purpose.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
