BEGIN IMMEDIATE;
-- Folder sizes after each complete scan, so the next one can show what grew. Only the last two of
-- each root are kept.
CREATE TABLE IF NOT EXISTS size_snapshots(
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,
  root BLOB NOT NULL,
  indexed_allocated INTEGER
);
CREATE TABLE IF NOT EXISTS size_snapshot_paths(
  snapshot INTEGER NOT NULL REFERENCES size_snapshots(id) ON DELETE CASCADE,
  path BLOB NOT NULL,
  allocated INTEGER NOT NULL,
  PRIMARY KEY(snapshot,path)
) WITHOUT ROWID;
PRAGMA user_version=13;
COMMIT;
