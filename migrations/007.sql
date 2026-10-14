BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS marked(id INTEGER PRIMARY KEY,root BLOB NOT NULL,path BLOB NOT NULL,identity TEXT NOT NULL,kind TEXT NOT NULL,allocated INTEGER NOT NULL,added INTEGER NOT NULL,warning TEXT,birth TEXT,UNIQUE(root,path));
CREATE INDEX IF NOT EXISTS entry_large ON entries(scan_id,allocated DESC) WHERE kind='file' AND apparent>=104857600;
PRAGMA user_version=7;
COMMIT;
