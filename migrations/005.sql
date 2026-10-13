BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS dir_totals(scan_id INTEGER NOT NULL REFERENCES scans(id) ON DELETE CASCADE,path BLOB NOT NULL,parent BLOB NOT NULL,allocated INTEGER NOT NULL,apparent INTEGER NOT NULL,files INTEGER NOT NULL,dirs INTEGER NOT NULL,protected INTEGER NOT NULL,PRIMARY KEY(scan_id,path)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS dir_parent ON dir_totals(scan_id,parent,allocated DESC);
CREATE INDEX IF NOT EXISTS entry_parent ON entries(scan_id,parent);
PRAGMA user_version=5;
COMMIT;
