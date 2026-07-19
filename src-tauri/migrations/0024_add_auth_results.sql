-- SPF/DKIM/DMARC verdicts of the topmost Authentication-Results header,
-- cached as JSON alongside the body. NULL = body not fetched yet, fetched
-- by an older build, or the message carried no such header — all three
-- mean "unknown", and unknown never warns.
ALTER TABLE messages ADD COLUMN auth_results TEXT;
