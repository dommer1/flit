-- How an account signs in: 'password' (IMAP LOGIN / SMTP AUTH with a
-- keychain password) or 'google' (OAuth — the keychain item then holds the
-- refresh token instead). Only the kind lives here, never a secret.
ALTER TABLE accounts ADD COLUMN auth TEXT NOT NULL DEFAULT 'password';
