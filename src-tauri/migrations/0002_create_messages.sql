-- Local cache of fetched mail. Bodies are lazily filled on first open and
-- stored unsanitized — sanitization happens on every read (see mail/sanitize).
CREATE TABLE messages (
    id           INTEGER PRIMARY KEY,
    account_id   INTEGER NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    mailbox      TEXT    NOT NULL DEFAULT 'INBOX',
    uid          INTEGER NOT NULL,
    uid_validity INTEGER NOT NULL,
    from_addr    TEXT    NOT NULL DEFAULT '',
    subject      TEXT    NOT NULL DEFAULT '',
    date         TEXT    NOT NULL,
    snippet      TEXT    NOT NULL DEFAULT '',
    read         INTEGER NOT NULL DEFAULT 0,
    body_text    TEXT,
    body_html    TEXT,
    UNIQUE (account_id, mailbox, uid)
);

CREATE INDEX idx_messages_date ON messages (date DESC);
