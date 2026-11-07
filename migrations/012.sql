BEGIN IMMEDIATE;
-- A path is parent||name, so it is stored once and the table doubles as the child listing. Only the
-- published scan is copied: rows of every other scan are invisible and would be pruned anyway.
CREATE TABLE entries_compact(scan_id INTEGER NOT NULL REFERENCES scans(id) ON DELETE CASCADE,parent BLOB NOT NULL,name BLOB NOT NULL,kind TEXT NOT NULL,identity TEXT NOT NULL,apparent INTEGER NOT NULL,allocated INTEGER NOT NULL,links INTEGER NOT NULL,modified_ns INTEGER NOT NULL,changed_ns INTEGER NOT NULL,excluded TEXT,PRIMARY KEY(scan_id,parent,name)) WITHOUT ROWID;
INSERT INTO entries_compact SELECT scan_id,parent,substr(path,length(parent)+1),kind,identity,apparent,allocated,links,modified_ns,changed_ns,excluded FROM entries WHERE scan_id=(SELECT scan_id FROM active WHERE singleton=1) ORDER BY parent,path;
DROP TABLE entries;
ALTER TABLE entries_compact RENAME TO entries;
CREATE INDEX entry_large ON entries(scan_id,allocated DESC) WHERE kind='file' AND apparent>=104857600;
PRAGMA user_version=12;
COMMIT;
