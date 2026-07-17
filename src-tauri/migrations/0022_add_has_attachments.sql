-- Whether a message carries attachments, known from BODYSTRUCTURE at header
-- sync time (before any body is cached); corrected by the body parse.
ALTER TABLE messages ADD COLUMN has_attachments INTEGER NOT NULL DEFAULT 0;
