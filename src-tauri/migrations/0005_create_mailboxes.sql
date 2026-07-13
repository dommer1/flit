-- The server's folder list per account, refreshed on every sync. Messages
-- reference folders by name (messages.mailbox), not by row id, so this
-- table can be wholesale-replaced without breaking anything.
CREATE TABLE mailboxes (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    -- Special-use role from IMAP LIST attributes (RFC 6154):
    -- 'inbox' | 'drafts' | 'sent' | 'archive' | 'junk' | 'trash' | NULL.
    role       TEXT,
    UNIQUE (account_id, name)
);
