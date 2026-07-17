-- A scheduled message keeps its chosen send-as alias. ON DELETE SET NULL:
-- if the alias is removed before delivery, the send falls back to the
-- account's own address instead of failing.
ALTER TABLE scheduled_messages ADD COLUMN alias_id INTEGER
    REFERENCES account_aliases(id) ON DELETE SET NULL;
