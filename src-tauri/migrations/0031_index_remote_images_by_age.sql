-- mail/remote.rs prunes expired images before every HTML body it shows,
-- and nothing indexed fetched_at: the DELETE read every cached row, and
-- since fetched_at is stored after the image blob, that meant reading the
-- blobs too. Measured on a real 320 MB cache: 52 ms warm, 1.3 s cold, paid
-- once per message of an opened conversation. With the index: 0.02 ms.
CREATE INDEX idx_remote_images_fetched_at ON remote_images (fetched_at);
