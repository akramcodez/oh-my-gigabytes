BEGIN IMMEDIATE;
-- A plain record of what omg deleted and how big it was, shown in the interface. Rows older than
-- 20 days are dropped; the operations table stays the full audit trail.
CREATE TABLE IF NOT EXISTS deleted_log(
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,
  source TEXT NOT NULL,
  path BLOB NOT NULL,
  bytes INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS deleted_log_at ON deleted_log(at);
PRAGMA user_version=14;
COMMIT;
