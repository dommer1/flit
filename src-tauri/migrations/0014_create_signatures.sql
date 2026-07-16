-- Reusable e-mail signatures, edited in Settings and inserted into compose
-- bodies. `body` holds editor HTML (the compose editor is rich text).
CREATE TABLE signatures (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT ''
);

-- Each account may point at its default signature ("Use this signature as
-- default for" in Settings). NULL = compose starts without a signature.
ALTER TABLE accounts
    ADD COLUMN signature_id INTEGER REFERENCES signatures (id) ON DELETE SET NULL;
