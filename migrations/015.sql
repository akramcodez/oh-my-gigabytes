BEGIN IMMEDIATE;
-- Auto delete: an item the user keeps is never taken by an automatic run, and the size recorded
-- when the item entered OMG Trash tells whether it grew since.
ALTER TABLE marked ADD COLUMN hold INTEGER NOT NULL DEFAULT 0;
ALTER TABLE marked ADD COLUMN added_bytes INTEGER;
UPDATE marked SET added_bytes=allocated;
PRAGMA user_version=15;
COMMIT;
