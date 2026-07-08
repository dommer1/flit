-- Account rows hold server configuration only. Passwords live in the macOS
-- Keychain (hard rule: secrets never hit disk in plaintext).
CREATE TABLE accounts (
    id        INTEGER PRIMARY KEY,
    name      TEXT    NOT NULL,
    email     TEXT    NOT NULL,
    imap_host TEXT    NOT NULL,
    imap_port INTEGER NOT NULL,
    smtp_host TEXT    NOT NULL,
    smtp_port INTEGER NOT NULL,
    username  TEXT    NOT NULL
);
