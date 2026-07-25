-- Dates cached by earlier builds keep the sender's own zone
-- ("2026-07-24T23:03:32-06:00") because that is what mail-parser returns.
-- Every date comparison in the app is a plain string compare, so such a row
-- sorts as if it were from the 24th and can show up under the wrong day
-- header. New rows are stored in UTC (mail::parse); these catch up once.
--
-- SQLite's date functions read the offset and shift to UTC. Rows with an
-- empty or unparsable date yield NULL and are left exactly as they are —
-- "no date" must not become "epoch".
UPDATE messages
   SET date = strftime('%Y-%m-%dT%H:%M:%SZ', date)
 WHERE date NOT LIKE '%Z'
   AND strftime('%Y-%m-%dT%H:%M:%SZ', date) IS NOT NULL;
