BEGIN IMMEDIATE;
-- Per-target outcome of a permanent-delete operation. Written in the same transaction as the
-- operation's intent, so a failed intent write leaves no operation and no target rows.
CREATE TABLE IF NOT EXISTS delete_targets(
  id INTEGER PRIMARY KEY,
  operation INTEGER NOT NULL REFERENCES operations(id) ON DELETE CASCADE,
  item INTEGER NOT NULL,
  path BLOB NOT NULL,
  outcome TEXT NOT NULL,
  detail TEXT,
  updated INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS delete_targets_operation ON delete_targets(operation);
PRAGMA user_version=11;
COMMIT;
