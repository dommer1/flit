-- Send-as aliases: extra addresses the account's mail server accepts as
-- sender (mail to them already lands in the account's inbox). Only stored
-- aliases may ever appear as From — see storage/aliases.rs.
CREATE TABLE account_aliases (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Display name for the From header; empty = address only.
    name TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL,
    UNIQUE (account_id, email)
);

-- The identity new mail from this account starts with;
-- NULL = the account's own address.
ALTER TABLE accounts ADD COLUMN default_alias_id INTEGER
    REFERENCES account_aliases(id) ON DELETE SET NULL;
